#![no_std]
//! # Hypha Reputation Registry
//!
//! Soroban port of the ERC-8004 **Reputation Registry**. Faithful to the spec, this registry is
//! **permissionless and fully independent of the Validation Registry**: anyone except the agent's
//! owner/operator may leave graded feedback, and no validation record is required. Sybil resistance
//! is handled downstream — `get_summary` requires the caller to pass an explicit, non-empty set of
//! client addresses to aggregate over, so consumers filter by reviewers they trust.
//!
//! Feedback carries a signed fixed-point `value` + `value_decimals` (graded scores, not a boolean),
//! two free-form tags, and emitted-only `endpoint` / `feedback_uri` / `feedback_hash` pointers to
//! the off-chain feedback file. Feedback can be revoked by its author and responded to by anyone.

use soroban_sdk::{
    contract, contractclient, contractevent, contractimpl, contracttype, Address, BytesN, Env,
    String, Vec,
};

const LEDGER_THRESHOLD: u32 = 100_000;
const LEDGER_BUMP: u32 = 500_000;

// --- Events ---

#[contractevent]
#[derive(Clone)]
pub struct NewFeedback {
    #[topic]
    pub agent_id: u64,
    #[topic]
    pub client: Address,
    pub index: u64,
    pub value: i128,
    pub value_decimals: u32,
    pub tag1: String,
    pub tag2: String,
    pub endpoint: String,
    pub feedback_uri: String,
    pub feedback_hash: BytesN<32>,
}

#[contractevent]
#[derive(Clone)]
pub struct FeedbackRevoked {
    #[topic]
    pub agent_id: u64,
    #[topic]
    pub client: Address,
    pub index: u64,
}

#[contractevent]
#[derive(Clone)]
pub struct ResponseAppended {
    #[topic]
    pub agent_id: u64,
    #[topic]
    pub client: Address,
    pub index: u64,
    pub responder: Address,
    pub response_uri: String,
    pub response_hash: BytesN<32>,
}

/// Minimal cross-contract view of the Identity Registry needed to resolve agent ownership.
#[contractclient(name = "IdentityClient")]
pub trait IdentityInterface {
    fn owner_of(env: Env, agent_id: u64) -> Address;
    fn is_approved_for_all(env: Env, owner: Address, operator: Address) -> bool;
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Identity,
    /// (agent_id, client, index) -> feedback entry.
    Feedback(u64, Address, u64),
    /// (agent_id, client) -> last 1-indexed feedback index issued by that client.
    LastIndex(u64, Address),
    /// agent_id -> distinct clients that have left feedback.
    Clients(u64),
    /// (agent_id, client, index) -> number of appended responses.
    ResponseCount(u64, Address, u64),
}

/// Stored feedback. `endpoint`, `feedback_uri`, and `feedback_hash` are emitted in `NewFeedback`
/// but deliberately not stored on-chain (they live in the off-chain feedback file).
#[contracttype]
#[derive(Clone)]
pub struct FeedbackEntry {
    pub value: i128,
    pub value_decimals: u32,
    pub tag1: String,
    pub tag2: String,
    pub is_revoked: bool,
}

/// Read-model row returned by `read_all_feedback` (struct vector in place of the spec's parallel
/// arrays — functionally identical, more idiomatic on Soroban).
#[contracttype]
#[derive(Clone)]
pub struct FeedbackView {
    pub client: Address,
    pub index: u64,
    pub value: i128,
    pub value_decimals: u32,
    pub tag1: String,
    pub tag2: String,
    pub is_revoked: bool,
}

#[contract]
pub struct ReputationRegistry;

#[contractimpl]
impl ReputationRegistry {
    pub fn __constructor(env: Env, identity_registry: Address) {
        env.storage()
            .instance()
            .set(&DataKey::Identity, &identity_registry);
    }

    pub fn get_identity_registry(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Identity).unwrap()
    }

    // --- Write path ---

    /// Leave graded feedback for `agent_id`. The submitter MUST NOT be the agent owner or one of its
    /// operators. Returns the new 1-indexed feedback index for this client.
    #[allow(clippy::too_many_arguments)]
    pub fn give_feedback(
        env: Env,
        client: Address,
        agent_id: u64,
        value: i128,
        value_decimals: u32,
        tag1: String,
        tag2: String,
        endpoint: String,
        feedback_uri: String,
        feedback_hash: BytesN<32>,
    ) -> u64 {
        client.require_auth();
        assert!(value_decimals <= 18, "value_decimals must be 0..=18");

        // Resolve ownership (also asserts the agent exists). Owners/operators cannot self-review.
        let id_client = Self::identity(&env);
        let owner = id_client.owner_of(&agent_id);
        assert!(client != owner, "owner cannot review own agent");
        assert!(
            !id_client.is_approved_for_all(&owner, &client),
            "operator cannot review managed agent"
        );

        let last: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::LastIndex(agent_id, client.clone()))
            .unwrap_or(0);
        let index = last + 1;

        if last == 0 {
            // First time this client reviews this agent: record it in the client set.
            let mut clients: Vec<Address> = env
                .storage()
                .persistent()
                .get(&DataKey::Clients(agent_id))
                .unwrap_or(Vec::new(&env));
            clients.push_back(client.clone());
            Self::set(&env, &DataKey::Clients(agent_id), &clients);
        }

        let entry = FeedbackEntry {
            value,
            value_decimals,
            tag1: tag1.clone(),
            tag2: tag2.clone(),
            is_revoked: false,
        };
        Self::set(&env, &DataKey::Feedback(agent_id, client.clone(), index), &entry);
        Self::set(&env, &DataKey::LastIndex(agent_id, client.clone()), &index);

        NewFeedback {
            agent_id,
            client: client.clone(),
            index,
            value,
            value_decimals,
            tag1,
            tag2,
            endpoint,
            feedback_uri,
            feedback_hash,
        }
        .publish(&env);
        index
    }

    /// Revoke previously-submitted feedback. Only the original author may revoke.
    pub fn revoke_feedback(env: Env, client: Address, agent_id: u64, index: u64) {
        client.require_auth();
        let key = DataKey::Feedback(agent_id, client.clone(), index);
        let mut entry: FeedbackEntry = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic!("feedback does not exist"));
        entry.is_revoked = true;
        Self::set(&env, &key, &entry);
        FeedbackRevoked {
            agent_id,
            client,
            index,
        }
        .publish(&env);
    }

    /// Append a response to an existing feedback (e.g. the agent issuing a refund, or anyone flagging
    /// spam). Callable by anyone; only a pointer + hash are emitted.
    pub fn append_response(
        env: Env,
        responder: Address,
        agent_id: u64,
        client: Address,
        index: u64,
        response_uri: String,
        response_hash: BytesN<32>,
    ) {
        responder.require_auth();
        assert!(
            env.storage()
                .persistent()
                .has(&DataKey::Feedback(agent_id, client.clone(), index)),
            "feedback does not exist"
        );
        let rkey = DataKey::ResponseCount(agent_id, client.clone(), index);
        let count: u64 = env.storage().persistent().get(&rkey).unwrap_or(0);
        Self::set(&env, &rkey, &(count + 1));
        ResponseAppended {
            agent_id,
            client,
            index,
            responder,
            response_uri,
            response_hash,
        }
        .publish(&env);
    }

    // --- Read path ---

    pub fn read_feedback(
        env: Env,
        agent_id: u64,
        client: Address,
        index: u64,
    ) -> Option<FeedbackEntry> {
        env.storage()
            .persistent()
            .get(&DataKey::Feedback(agent_id, client, index))
    }

    pub fn get_last_index(env: Env, agent_id: u64, client: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::LastIndex(agent_id, client))
            .unwrap_or(0)
    }

    pub fn get_clients(env: Env, agent_id: u64) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::Clients(agent_id))
            .unwrap_or(Vec::new(&env))
    }

    pub fn get_response_count(env: Env, agent_id: u64, client: Address, index: u64) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::ResponseCount(agent_id, client, index))
            .unwrap_or(0)
    }

    /// Return every non-revoked (unless `include_revoked`) feedback for `agent_id` from the given
    /// `clients`, optionally filtered by tag. An empty `tag1`/`tag2` means "any".
    pub fn read_all_feedback(
        env: Env,
        agent_id: u64,
        clients: Vec<Address>,
        tag1: String,
        tag2: String,
        include_revoked: bool,
    ) -> Vec<FeedbackView> {
        let mut out: Vec<FeedbackView> = Vec::new(&env);
        let empty = String::from_str(&env, "");
        for client in clients.iter() {
            let last = env
                .storage()
                .persistent()
                .get(&DataKey::LastIndex(agent_id, client.clone()))
                .unwrap_or(0u64);
            let mut i = 1u64;
            while i <= last {
                if let Some(entry) = env
                    .storage()
                    .persistent()
                    .get::<_, FeedbackEntry>(&DataKey::Feedback(agent_id, client.clone(), i))
                {
                    let revoked_ok = include_revoked || !entry.is_revoked;
                    let tag1_ok = tag1 == empty || tag1 == entry.tag1;
                    let tag2_ok = tag2 == empty || tag2 == entry.tag2;
                    if revoked_ok && tag1_ok && tag2_ok {
                        out.push_back(FeedbackView {
                            client: client.clone(),
                            index: i,
                            value: entry.value,
                            value_decimals: entry.value_decimals,
                            tag1: entry.tag1,
                            tag2: entry.tag2,
                            is_revoked: entry.is_revoked,
                        });
                    }
                }
                i += 1;
            }
        }
        out
    }

    /// Aggregate feedback for `agent_id` from an explicit, non-empty `clients` set (Sybil mitigation
    /// per the spec). Returns `(count, summary_value, summary_value_decimals)`, where `summary_value`
    /// is the mean of matching non-revoked values normalized to 18 decimals.
    pub fn get_summary(
        env: Env,
        agent_id: u64,
        clients: Vec<Address>,
        tag1: String,
        tag2: String,
    ) -> (u64, i128, u32) {
        assert!(!clients.is_empty(), "clients must be non-empty");
        let rows = Self::read_all_feedback(env.clone(), agent_id, clients, tag1, tag2, false);
        let count = rows.len() as u64;
        if count == 0 {
            return (0, 0, 18);
        }
        let mut sum: i128 = 0;
        for r in rows.iter() {
            // Normalize each value to 18-decimal fixed point before averaging.
            sum += r.value * pow10(18 - r.value_decimals);
        }
        (count, sum / (count as i128), 18)
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

/// 10^n for n in 0..=18.
fn pow10(n: u32) -> i128 {
    let mut acc: i128 = 1;
    let mut i = 0;
    while i < n {
        acc *= 10;
        i += 1;
    }
    acc
}

mod test;
