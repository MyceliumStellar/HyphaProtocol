#![no_std]
//! # Hypha Identity Registry
//!
//! Soroban port of the ERC-8004 **Identity Registry**. ERC-8004 models agent identity as an
//! ERC-721 NFT: a registry-assigned numeric `agentId`, an owner, transferability, an `agentURI`
//! (the ERC-721 `tokenURI`) pointing to the off-chain Agent Registration File, an arbitrary
//! on-chain key/value metadata store, and a separately-proven `agentWallet`.
//!
//! Stellar has no native ERC-721, so this contract implements the equivalent semantics directly:
//! incrementing `agent_id`, ownership + transfer/approve, `agent_uri`, key/value metadata, and an
//! `agent_wallet` whose control is proven via **Soroban-native auth** in place of the spec's
//! EIP-712 / ERC-1271 signature (`set_agent_wallet` requires auth from *both* the owner and the
//! new wallet, which is the Soroban-idiomatic proof that the new wallet is controlled).

use soroban_sdk::{
    contract, contractclient, contractevent, contractimpl, contracttype, Address, Bytes, Env,
    String, Vec,
};

const LEDGER_THRESHOLD: u32 = 100_000;
const LEDGER_BUMP: u32 = 500_000;

// --- Events (ERC-8004 indexer surface) ---

#[contractevent]
#[derive(Clone)]
pub struct Registered {
    #[topic]
    pub agent_id: u64,
    pub owner: Address,
    pub agent_uri: String,
}

#[contractevent]
#[derive(Clone)]
pub struct Transfer {
    #[topic]
    pub agent_id: u64,
    pub from: Option<Address>,
    pub to: Address,
}

#[contractevent]
#[derive(Clone)]
pub struct MetadataSet {
    #[topic]
    pub agent_id: u64,
    pub key: String,
    pub value: Bytes,
}

#[contractevent]
#[derive(Clone)]
pub struct UriUpdated {
    #[topic]
    pub agent_id: u64,
    pub new_uri: String,
    pub updated_by: Address,
}

#[contractevent]
#[derive(Clone)]
pub struct WalletSet {
    #[topic]
    pub agent_id: u64,
    pub wallet: Address,
}

#[contractevent]
#[derive(Clone)]
pub struct WalletUnset {
    #[topic]
    pub agent_id: u64,
    pub owner: Address,
}

#[contractevent]
#[derive(Clone)]
pub struct Approval {
    #[topic]
    pub agent_id: u64,
    pub owner: Address,
    pub spender: Address,
}

#[contractevent]
#[derive(Clone)]
pub struct ApprovalForAll {
    #[topic]
    pub owner: Address,
    pub operator: Address,
    pub approved: bool,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Network label (e.g. "testnet") captured at construction, used to build the global namespace.
    Network,
    /// Monotonic `agent_id` counter (next id to assign).
    NextId,
    /// agent_id -> owner address.
    Owner(u64),
    /// owner address -> number of agents owned.
    Balance(Address),
    /// agent_id -> agent_uri (resolves to the off-chain Agent Registration File).
    AgentUri(u64),
    /// agent_id -> single-token approved operator.
    Approved(u64),
    /// (owner, operator) -> approved-for-all flag.
    Operator(Address, Address),
    /// (agent_id, key) -> arbitrary metadata value.
    Meta(u64, String),
    /// agent_id -> operational wallet (defaults to owner until explicitly set).
    AgentWallet(u64),
}

/// A single on-chain metadata key/value pair supplied at registration time.
#[contracttype]
#[derive(Clone)]
pub struct MetadataEntry {
    pub key: String,
    pub value: Bytes,
}

/// Cross-contract view of this registry, consumed by the reputation and validation contracts to
/// resolve agent ownership. Generated client: `IdentityClient`.
#[contractclient(name = "IdentityClient")]
pub trait IdentityInterface {
    fn owner_of(env: Env, agent_id: u64) -> Address;
    fn is_approved_for_all(env: Env, owner: Address, operator: Address) -> bool;
    fn agent_exists(env: Env, agent_id: u64) -> bool;
}

#[contract]
pub struct IdentityRegistry;

#[contractimpl]
impl IdentityRegistry {
    /// Initialize the registry with the network label used in the global `agentRegistry` namespace.
    pub fn __constructor(env: Env, network: String) {
        env.storage().instance().set(&DataKey::Network, &network);
        env.storage().instance().set(&DataKey::NextId, &1u64);
    }

    // --- Registration & identity (ERC-721 mint) ---

    /// Register a new agent. `agent_uri` may be empty and `metadata` may be empty, covering the
    /// spec's three `register` overloads with a single variadic entry point. Returns the new
    /// `agent_id`. The agent's operational wallet is initialized to `owner`.
    pub fn register(
        env: Env,
        owner: Address,
        agent_uri: String,
        metadata: Vec<MetadataEntry>,
    ) -> u64 {
        owner.require_auth();

        let id: u64 = env.storage().instance().get(&DataKey::NextId).unwrap_or(1);
        env.storage().instance().set(&DataKey::NextId, &(id + 1));

        Self::set_persistent(&env, &DataKey::Owner(id), &owner);
        Self::set_persistent(&env, &DataKey::AgentUri(id), &agent_uri);
        Self::set_persistent(&env, &DataKey::AgentWallet(id), &owner);

        let balance: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(owner.clone()))
            .unwrap_or(0);
        Self::set_persistent(&env, &DataKey::Balance(owner.clone()), &(balance + 1));

        for entry in metadata.iter() {
            Self::set_persistent(&env, &DataKey::Meta(id, entry.key.clone()), &entry.value);
            MetadataSet {
                agent_id: id,
                key: entry.key.clone(),
                value: entry.value.clone(),
            }
            .publish(&env);
        }

        Registered {
            agent_id: id,
            owner: owner.clone(),
            agent_uri,
        }
        .publish(&env);
        // ERC-721 mint Transfer: from is None (no prior owner).
        Transfer {
            agent_id: id,
            from: None,
            to: owner,
        }
        .publish(&env);
        id
    }

    /// Owner of `agent_id`. Panics if the agent does not exist (mirrors ERC-721 `ownerOf`).
    pub fn owner_of(env: Env, agent_id: u64) -> Address {
        env.storage()
            .persistent()
            .get(&DataKey::Owner(agent_id))
            .unwrap_or_else(|| panic!("agent does not exist"))
    }

    pub fn agent_exists(env: Env, agent_id: u64) -> bool {
        env.storage().persistent().has(&DataKey::Owner(agent_id))
    }

    pub fn balance_of(env: Env, owner: Address) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(owner))
            .unwrap_or(0)
    }

    /// Total number of agents ever minted (next id minus one).
    pub fn total_agents(env: Env) -> u64 {
        let next: u64 = env.storage().instance().get(&DataKey::NextId).unwrap_or(1);
        next - 1
    }

    // --- Agent URI (ERC-721 tokenURI) ---

    pub fn agent_uri(env: Env, agent_id: u64) -> String {
        env.storage()
            .persistent()
            .get(&DataKey::AgentUri(agent_id))
            .unwrap_or_else(|| panic!("agent does not exist"))
    }

    pub fn set_agent_uri(env: Env, agent_id: u64, new_uri: String) {
        let owner = Self::owner_of(env.clone(), agent_id);
        owner.require_auth();
        Self::set_persistent(&env, &DataKey::AgentUri(agent_id), &new_uri);
        UriUpdated {
            agent_id,
            new_uri,
            updated_by: owner,
        }
        .publish(&env);
    }

    // --- On-chain metadata ---

    pub fn get_metadata(env: Env, agent_id: u64, key: String) -> Option<Bytes> {
        env.storage().persistent().get(&DataKey::Meta(agent_id, key))
    }

    pub fn set_metadata(env: Env, agent_id: u64, key: String, value: Bytes) {
        let owner = Self::owner_of(env.clone(), agent_id);
        owner.require_auth();
        Self::set_persistent(&env, &DataKey::Meta(agent_id, key.clone()), &value);
        MetadataSet {
            agent_id,
            key,
            value,
        }
        .publish(&env);
    }

    // --- Agent wallet (spec's EIP-712 / ERC-1271 proof -> dual Soroban auth) ---

    /// Set the operational wallet for `agent_id`. Requires auth from BOTH the owner (authority over
    /// the identity) AND `new_wallet` (proof the new wallet is controlled) — the Soroban-native
    /// equivalent of ERC-8004's EIP-712 / ERC-1271 signature requirement.
    pub fn set_agent_wallet(env: Env, agent_id: u64, new_wallet: Address) {
        let owner = Self::owner_of(env.clone(), agent_id);
        owner.require_auth();
        new_wallet.require_auth();
        Self::set_persistent(&env, &DataKey::AgentWallet(agent_id), &new_wallet);
        WalletSet {
            agent_id,
            wallet: new_wallet,
        }
        .publish(&env);
    }

    /// Operational wallet for `agent_id`. Falls back to the owner if none has been explicitly set.
    pub fn get_agent_wallet(env: Env, agent_id: u64) -> Address {
        env.storage()
            .persistent()
            .get(&DataKey::AgentWallet(agent_id))
            .unwrap_or_else(|| Self::owner_of(env.clone(), agent_id))
    }

    /// Clear an explicit wallet, reverting `get_agent_wallet` to the owner.
    pub fn unset_agent_wallet(env: Env, agent_id: u64) {
        let owner = Self::owner_of(env.clone(), agent_id);
        owner.require_auth();
        env.storage()
            .persistent()
            .remove(&DataKey::AgentWallet(agent_id));
        WalletUnset { agent_id, owner }.publish(&env);
    }

    // --- Transfer / approval (ERC-721) ---

    pub fn approve(env: Env, owner: Address, spender: Address, agent_id: u64) {
        owner.require_auth();
        let actual = Self::owner_of(env.clone(), agent_id);
        assert!(actual == owner, "only owner can approve");
        Self::set_persistent(&env, &DataKey::Approved(agent_id), &spender);
        Approval {
            agent_id,
            owner,
            spender,
        }
        .publish(&env);
    }

    pub fn get_approved(env: Env, agent_id: u64) -> Option<Address> {
        env.storage().persistent().get(&DataKey::Approved(agent_id))
    }

    pub fn set_approval_for_all(env: Env, owner: Address, operator: Address, approved: bool) {
        owner.require_auth();
        Self::set_persistent(
            &env,
            &DataKey::Operator(owner.clone(), operator.clone()),
            &approved,
        );
        ApprovalForAll {
            owner,
            operator,
            approved,
        }
        .publish(&env);
    }

    pub fn is_approved_for_all(env: Env, owner: Address, operator: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Operator(owner, operator))
            .unwrap_or(false)
    }

    /// Transfer directly by the owner.
    pub fn transfer(env: Env, from: Address, to: Address, agent_id: u64) {
        from.require_auth();
        let owner = Self::owner_of(env.clone(), agent_id);
        assert!(owner == from, "from is not the owner");
        Self::do_transfer(&env, &from, &to, agent_id);
    }

    /// Transfer by an approved spender or operator (ERC-721 `transferFrom`).
    pub fn transfer_from(env: Env, spender: Address, from: Address, to: Address, agent_id: u64) {
        spender.require_auth();
        let owner = Self::owner_of(env.clone(), agent_id);
        assert!(owner == from, "from is not the owner");
        let approved = env
            .storage()
            .persistent()
            .get::<_, Address>(&DataKey::Approved(agent_id));
        let is_op = Self::is_approved_for_all(env.clone(), from.clone(), spender.clone());
        assert!(
            spender == owner || approved == Some(spender.clone()) || is_op,
            "caller is not owner nor approved"
        );
        Self::do_transfer(&env, &from, &to, agent_id);
    }

    pub fn registry_namespace(env: Env) -> String {
        env.storage()
            .instance()
            .get(&DataKey::Network)
            .unwrap_or_else(|| String::from_str(&env, "testnet"))
    }

    // --- internal helpers ---

    fn do_transfer(env: &Env, from: &Address, to: &Address, agent_id: u64) {
        Self::set_persistent(env, &DataKey::Owner(agent_id), to);

        let from_bal: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(from.clone()))
            .unwrap_or(0);
        if from_bal > 0 {
            Self::set_persistent(env, &DataKey::Balance(from.clone()), &(from_bal - 1));
        }
        let to_bal: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(to.clone()))
            .unwrap_or(0);
        Self::set_persistent(env, &DataKey::Balance(to.clone()), &(to_bal + 1));

        // Clear single-token approval and the operational wallet — the new owner must re-verify it
        // (ERC-8004: agentWallet is auto-cleared on transfer).
        env.storage().persistent().remove(&DataKey::Approved(agent_id));
        env.storage()
            .persistent()
            .remove(&DataKey::AgentWallet(agent_id));
        Self::set_persistent(env, &DataKey::AgentWallet(agent_id), to);

        Transfer {
            agent_id,
            from: Some(from.clone()),
            to: to.clone(),
        }
        .publish(env);
    }

    fn set_persistent<V: soroban_sdk::IntoVal<Env, soroban_sdk::Val>>(
        env: &Env,
        key: &DataKey,
        value: &V,
    ) {
        env.storage().persistent().set(key, value);
        env.storage()
            .persistent()
            .extend_ttl(key, LEDGER_THRESHOLD, LEDGER_BUMP);
    }
}

mod test;
