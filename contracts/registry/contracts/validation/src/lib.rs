#![no_std]
//! # Hypha Validation Registry
//!
//! Soroban port of the ERC-8004 **Validation Registry**. Two-phase and mechanism-agnostic:
//!
//! 1. The agent owner/operator calls [`validation_request`], naming a validator and committing to a
//!    request payload via `request_hash`.
//! 2. The named validator calls [`validation_response`] with a graded `response` in `0..=100`. It may
//!    be called repeatedly for the same request (e.g. "soft finality" then "hard finality").
//!
//! The contract is agnostic to *how* validation is performed — the `tag` carries the mechanism
//! (`"stake"`, `"tee"`, `"zkml"`). Incentives and slashing are intentionally out of scope (handled
//! by external protocols, e.g. Hypha's `staking` contract for the stake-secured variant).

use soroban_sdk::{
    contract, contractclient, contractevent, contractimpl, contracttype, Address, BytesN, Env,
    String, Vec,
};

const LEDGER_THRESHOLD: u32 = 100_000;
const LEDGER_BUMP: u32 = 500_000;

// --- Events ---

#[contractevent]
#[derive(Clone)]
pub struct ValidationRequest {
    #[topic]
    pub validator: Address,
    #[topic]
    pub agent_id: u64,
    pub request_hash: BytesN<32>,
    pub request_uri: String,
}

#[contractevent]
#[derive(Clone)]
pub struct ValidationResponse {
    #[topic]
    pub validator: Address,
    #[topic]
    pub agent_id: u64,
    pub request_hash: BytesN<32>,
    pub response: u32,
    pub response_uri: String,
    pub response_hash: BytesN<32>,
    pub tag: String,
}

/// Sentinel `response` meaning "request recorded, awaiting validator response".
pub const PENDING: u32 = u32::MAX;

#[contractclient(name = "IdentityClient")]
pub trait IdentityInterface {
    fn owner_of(env: Env, agent_id: u64) -> Address;
    fn is_approved_for_all(env: Env, owner: Address, operator: Address) -> bool;
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Identity,
    /// request_hash -> validation status.
    Request(BytesN<32>),
    /// agent_id -> request hashes targeting it.
    AgentValidations(u64),
    /// validator -> request hashes assigned to it.
    ValidatorRequests(Address),
}

#[contracttype]
#[derive(Clone)]
pub struct ValidationStatus {
    pub validator: Address,
    pub agent_id: u64,
    /// `0..=100` once responded, or [`PENDING`] while awaiting a response.
    pub response: u32,
    pub response_hash: BytesN<32>,
    pub tag: String,
    pub last_update: u64,
}

#[contract]
pub struct ValidationRegistry;

#[contractimpl]
impl ValidationRegistry {
    pub fn __constructor(env: Env, identity_registry: Address) {
        env.storage()
            .instance()
            .set(&DataKey::Identity, &identity_registry);
    }

    pub fn get_identity_registry(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Identity).unwrap()
    }

    // --- Phase 1: request ---

    /// Request validation of `agent_id`'s work from `validator`. Caller must be the agent's owner or
    /// an approved operator. `request_hash` commits to the off-chain request payload at `request_uri`.
    pub fn validation_request(
        env: Env,
        requester: Address,
        agent_id: u64,
        validator: Address,
        request_uri: String,
        request_hash: BytesN<32>,
    ) {
        requester.require_auth();

        let id_client = Self::identity(&env);
        let owner = id_client.owner_of(&agent_id);
        let authorized = requester == owner
            || id_client.is_approved_for_all(&owner, &requester);
        assert!(authorized, "requester is not owner or operator of agent");

        assert!(
            !env.storage().persistent().has(&DataKey::Request(request_hash.clone())),
            "request hash already used"
        );

        let status = ValidationStatus {
            validator: validator.clone(),
            agent_id,
            response: PENDING,
            response_hash: BytesN::from_array(&env, &[0u8; 32]),
            tag: String::from_str(&env, ""),
            last_update: env.ledger().sequence() as u64,
        };
        Self::set(&env, &DataKey::Request(request_hash.clone()), &status);

        // Index under both the agent and the validator.
        let mut agent_list: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&DataKey::AgentValidations(agent_id))
            .unwrap_or(Vec::new(&env));
        agent_list.push_back(request_hash.clone());
        Self::set(&env, &DataKey::AgentValidations(agent_id), &agent_list);

        let mut val_list: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&DataKey::ValidatorRequests(validator.clone()))
            .unwrap_or(Vec::new(&env));
        val_list.push_back(request_hash.clone());
        Self::set(&env, &DataKey::ValidatorRequests(validator.clone()), &val_list);

        ValidationRequest {
            validator,
            agent_id,
            request_hash,
            request_uri,
        }
        .publish(&env);
    }

    // --- Phase 2: response ---

    /// Record a graded validation response. Only the validator named in the request may call this; it
    /// may be called multiple times to refine the score (soft -> hard finality).
    pub fn validation_response(
        env: Env,
        validator: Address,
        request_hash: BytesN<32>,
        response: u32,
        response_uri: String,
        response_hash: BytesN<32>,
        tag: String,
    ) {
        validator.require_auth();
        assert!(response <= 100, "response must be 0..=100");

        let mut status: ValidationStatus = env
            .storage()
            .persistent()
            .get(&DataKey::Request(request_hash.clone()))
            .unwrap_or_else(|| panic!("validation request does not exist"));
        assert!(status.validator == validator, "caller is not the named validator");

        status.response = response;
        status.response_hash = response_hash.clone();
        status.tag = tag.clone();
        status.last_update = env.ledger().sequence() as u64;
        let agent_id = status.agent_id;
        Self::set(&env, &DataKey::Request(request_hash.clone()), &status);

        ValidationResponse {
            validator,
            agent_id,
            request_hash,
            response,
            response_uri,
            response_hash,
            tag,
        }
        .publish(&env);
    }

    // --- Reads ---

    pub fn get_validation_status(env: Env, request_hash: BytesN<32>) -> Option<ValidationStatus> {
        env.storage().persistent().get(&DataKey::Request(request_hash))
    }

    pub fn get_agent_validations(env: Env, agent_id: u64) -> Vec<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&DataKey::AgentValidations(agent_id))
            .unwrap_or(Vec::new(&env))
    }

    pub fn get_validator_requests(env: Env, validator: Address) -> Vec<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&DataKey::ValidatorRequests(validator))
            .unwrap_or(Vec::new(&env))
    }

    /// Aggregate responded validations for `agent_id` from the given `validators`, optionally filtered
    /// by `tag` (empty = any). Returns `(count, average_response)` over `0..=100`; pending requests are
    /// excluded.
    pub fn get_summary(
        env: Env,
        agent_id: u64,
        validators: Vec<Address>,
        tag: String,
    ) -> (u64, u32) {
        let hashes = Self::get_agent_validations(env.clone(), agent_id);
        let empty = String::from_str(&env, "");
        let mut count: u64 = 0;
        let mut sum: u64 = 0;
        for hash in hashes.iter() {
            if let Some(status) = env
                .storage()
                .persistent()
                .get::<_, ValidationStatus>(&DataKey::Request(hash.clone()))
            {
                if status.response == PENDING {
                    continue;
                }
                let validator_ok = validators.is_empty() || validators.contains(&status.validator);
                let tag_ok = tag == empty || tag == status.tag;
                if validator_ok && tag_ok {
                    count += 1;
                    sum += status.response as u64;
                }
            }
        }
        if count == 0 {
            (0, 0)
        } else {
            (count, (sum / count) as u32)
        }
    }

    // --- internal ---

    fn identity(env: &Env) -> IdentityClient {
        let addr: Address = env.storage().instance().get(&DataKey::Identity).unwrap();
        IdentityClient::new(env, &addr)
    }

    fn set<V: soroban_sdk::IntoVal<Env, soroban_sdk::Val>>(env: &Env, key: &DataKey, value: &V) {
        env.storage().persistent().set(key, value);
        env.storage()
            .persistent()
            .extend_ttl(key, LEDGER_THRESHOLD, LEDGER_BUMP);
    }
}

mod test;
