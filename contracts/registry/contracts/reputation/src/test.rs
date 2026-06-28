#![cfg(test)]

use super::*;
use identity::{IdentityRegistry, IdentityRegistryClient};
use soroban_sdk::{testutils::Address as _, vec, BytesN, Env, String};

struct Harness<'a> {
    env: Env,
    identity: IdentityRegistryClient<'a>,
    reputation: ReputationRegistryClient<'a>,
}

fn setup<'a>() -> Harness<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let id_addr = env.register(IdentityRegistry, (String::from_str(&env, "testnet"),));
    let identity = IdentityRegistryClient::new(&env, &id_addr);

    let rep_addr = env.register(ReputationRegistry, (id_addr.clone(),));
    let reputation = ReputationRegistryClient::new(&env, &rep_addr);

    Harness {
        env,
        identity,
        reputation,
    }
}

fn s(env: &Env, v: &str) -> String {
    String::from_str(env, v)
}

fn h(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

#[test]
fn test_permissionless_feedback_needs_no_validation() {
    let H = setup();
    let owner = Address::generate(&H.env);
    let client = Address::generate(&H.env);

    let agent_id = H
        .identity
        .register(&owner, &s(&H.env, "ipfs://agent"), &vec![&H.env]);

    // No validation record exists anywhere — feedback still succeeds (core decoupling fix).
    let idx = H.reputation.give_feedback(
        &client,
        &agent_id,
        &5i128,
        &0u32,
        &s(&H.env, "quality"),
        &s(&H.env, ""),
        &s(&H.env, ""),
        &s(&H.env, ""),
        &h(&H.env),
    );
    assert_eq!(idx, 1);

    let entry = H.reputation.read_feedback(&agent_id, &client, &1).unwrap();
    assert_eq!(entry.value, 5);
    assert_eq!(entry.is_revoked, false);
    assert_eq!(H.reputation.get_clients(&agent_id).len(), 1);
}

#[test]
#[should_panic(expected = "owner cannot review own agent")]
fn test_owner_cannot_self_review() {
    let H = setup();
    let owner = Address::generate(&H.env);
    let agent_id = H.identity.register(&owner, &s(&H.env, ""), &vec![&H.env]);
    H.reputation.give_feedback(
        &owner,
        &agent_id,
        &5i128,
        &0u32,
        &s(&H.env, ""),
        &s(&H.env, ""),
        &s(&H.env, ""),
        &s(&H.env, ""),
        &h(&H.env),
    );
}

#[test]
fn test_revoke_feedback() {
    let H = setup();
    let owner = Address::generate(&H.env);
    let client = Address::generate(&H.env);
    let agent_id = H.identity.register(&owner, &s(&H.env, ""), &vec![&H.env]);

    H.reputation.give_feedback(
        &client,
        &agent_id,
        &4i128,
        &0u32,
        &s(&H.env, ""),
        &s(&H.env, ""),
        &s(&H.env, ""),
        &s(&H.env, ""),
        &h(&H.env),
    );
    H.reputation.revoke_feedback(&client, &agent_id, &1);
    let entry = H.reputation.read_feedback(&agent_id, &client, &1).unwrap();
    assert!(entry.is_revoked);

    // Revoked feedback is excluded from the summary.
    let (count, _, _) = H.reputation.get_summary(
        &agent_id,
        &vec![&H.env, client.clone()],
        &s(&H.env, ""),
        &s(&H.env, ""),
    );
    assert_eq!(count, 0);
}

#[test]
fn test_summary_averages_across_clients() {
    let H = setup();
    let owner = Address::generate(&H.env);
    let c1 = Address::generate(&H.env);
    let c2 = Address::generate(&H.env);
    let agent_id = H.identity.register(&owner, &s(&H.env, ""), &vec![&H.env]);

    H.reputation.give_feedback(
        &c1, &agent_id, &5i128, &0u32, &s(&H.env, "q"), &s(&H.env, ""),
        &s(&H.env, ""), &s(&H.env, ""), &h(&H.env),
    );
    H.reputation.give_feedback(
        &c2, &agent_id, &3i128, &0u32, &s(&H.env, "q"), &s(&H.env, ""),
        &s(&H.env, ""), &s(&H.env, ""), &h(&H.env),
    );

    let (count, summary, decimals) = H.reputation.get_summary(
        &agent_id,
        &vec![&H.env, c1.clone(), c2.clone()],
        &s(&H.env, ""),
        &s(&H.env, ""),
    );
    assert_eq!(count, 2);
    assert_eq!(decimals, 18);
    // mean(5, 3) = 4, normalized to 18 decimals.
    assert_eq!(summary, 4i128 * 1_000_000_000_000_000_000i128);
}

#[test]
fn test_append_response() {
    let H = setup();
    let owner = Address::generate(&H.env);
    let client = Address::generate(&H.env);
    let responder = Address::generate(&H.env);
    let agent_id = H.identity.register(&owner, &s(&H.env, ""), &vec![&H.env]);

    H.reputation.give_feedback(
        &client, &agent_id, &2i128, &0u32, &s(&H.env, ""), &s(&H.env, ""),
        &s(&H.env, ""), &s(&H.env, ""), &h(&H.env),
    );
    H.reputation.append_response(
        &responder,
        &agent_id,
        &client,
        &1,
        &s(&H.env, "ipfs://refund"),
        &h(&H.env),
    );
    assert_eq!(H.reputation.get_response_count(&agent_id, &client, &1), 1);
}
