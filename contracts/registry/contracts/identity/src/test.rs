#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, vec, Bytes, Env, String, Vec};

fn setup(env: &Env) -> IdentityRegistryClient {
    let id = env.register(IdentityRegistry, (String::from_str(env, "testnet"),));
    IdentityRegistryClient::new(env, &id)
}

fn no_meta(env: &Env) -> Vec<MetadataEntry> {
    Vec::new(env)
}

#[test]
fn test_register_assigns_incrementing_ids() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup(&env);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    let id1 = client.register(&alice, &String::from_str(&env, "ipfs://alice"), &no_meta(&env));
    let id2 = client.register(&bob, &String::from_str(&env, "ipfs://bob"), &no_meta(&env));

    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    assert_eq!(client.owner_of(&id1), alice);
    assert_eq!(client.owner_of(&id2), bob);
    assert_eq!(client.agent_uri(&id1), String::from_str(&env, "ipfs://alice"));
    assert_eq!(client.balance_of(&alice), 1);
    assert_eq!(client.total_agents(), 2);
    // Operational wallet defaults to the owner.
    assert_eq!(client.get_agent_wallet(&id1), alice);
}

#[test]
fn test_metadata_roundtrip() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup(&env);
    let owner = Address::generate(&env);

    let entry = MetadataEntry {
        key: String::from_str(&env, "agentName"),
        value: Bytes::from_slice(&env, b"Hypha Oracle"),
    };
    let id = client.register(
        &owner,
        &String::from_str(&env, "ipfs://x"),
        &vec![&env, entry],
    );

    assert_eq!(
        client.get_metadata(&id, &String::from_str(&env, "agentName")),
        Some(Bytes::from_slice(&env, b"Hypha Oracle"))
    );

    client.set_metadata(
        &id,
        &String::from_str(&env, "endpoint"),
        &Bytes::from_slice(&env, b"https://hypha.network"),
    );
    assert_eq!(
        client.get_metadata(&id, &String::from_str(&env, "endpoint")),
        Some(Bytes::from_slice(&env, b"https://hypha.network"))
    );
}

#[test]
fn test_set_agent_wallet_requires_dual_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup(&env);
    let owner = Address::generate(&env);
    let wallet = Address::generate(&env);

    let id = client.register(&owner, &String::from_str(&env, ""), &no_meta(&env));
    client.set_agent_wallet(&id, &wallet);
    assert_eq!(client.get_agent_wallet(&id), wallet);

    client.unset_agent_wallet(&id);
    assert_eq!(client.get_agent_wallet(&id), owner);
}

#[test]
fn test_transfer_moves_ownership_and_clears_wallet() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let wallet = Address::generate(&env);

    let id = client.register(&alice, &String::from_str(&env, ""), &no_meta(&env));
    client.set_agent_wallet(&id, &wallet);
    assert_eq!(client.get_agent_wallet(&id), wallet);

    client.transfer(&alice, &bob, &id);

    assert_eq!(client.owner_of(&id), bob);
    assert_eq!(client.balance_of(&alice), 0);
    assert_eq!(client.balance_of(&bob), 1);
    // Wallet was cleared on transfer and now defaults to the new owner.
    assert_eq!(client.get_agent_wallet(&id), bob);
}

#[test]
fn test_transfer_from_with_approval() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup(&env);
    let alice = Address::generate(&env);
    let operator = Address::generate(&env);
    let bob = Address::generate(&env);

    let id = client.register(&alice, &String::from_str(&env, ""), &no_meta(&env));
    client.approve(&alice, &operator, &id);
    assert_eq!(client.get_approved(&id), Some(operator.clone()));

    client.transfer_from(&operator, &alice, &bob, &id);
    assert_eq!(client.owner_of(&id), bob);
    // Approval is cleared after transfer.
    assert_eq!(client.get_approved(&id), None);
}

#[test]
fn test_approval_for_all() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup(&env);
    let alice = Address::generate(&env);
    let operator = Address::generate(&env);
    let bob = Address::generate(&env);

    let id = client.register(&alice, &String::from_str(&env, ""), &no_meta(&env));
    client.set_approval_for_all(&alice, &operator, &true);
    assert!(client.is_approved_for_all(&alice, &operator));

    client.transfer_from(&operator, &alice, &bob, &id);
    assert_eq!(client.owner_of(&id), bob);
}

#[test]
#[should_panic(expected = "agent does not exist")]
fn test_owner_of_nonexistent_panics() {
    let env = Env::default();
    let client = setup(&env);
    client.owner_of(&999);
}
