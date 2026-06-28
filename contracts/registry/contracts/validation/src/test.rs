#![cfg(test)]

use super::*;
use identity::{IdentityRegistry, IdentityRegistryClient};
use soroban_sdk::{testutils::Address as _, vec, BytesN, Env, String};

struct Harness<'a> {
    env: Env,
    identity: IdentityRegistryClient<'a>,
    validation: ValidationRegistryClient<'a>,
}

fn setup<'a>() -> Harness<'a> {
    let env = Env::default();
    env.mock_all_auths();
    let id_addr = env.register(IdentityRegistry, (String::from_str(&env, "testnet"),));
    let identity = IdentityRegistryClient::new(&env, &id_addr);
    let val_addr = env.register(ValidationRegistry, (id_addr.clone(),));
    let validation = ValidationRegistryClient::new(&env, &val_addr);
    Harness {
        env,
        identity,
        validation,
    }
}

fn s(env: &Env, v: &str) -> String {
    String::from_str(env, v)
}

fn hash(env: &Env, b: u8) -> BytesN<32> {
    BytesN::from_array(env, &[b; 32])
}

#[test]
fn test_two_phase_validation() {
    let H = setup();
    let owner = Address::generate(&H.env);
    let validator = Address::generate(&H.env);
    let agent_id = H.identity.register(&owner, &s(&H.env, ""), &vec![&H.env]);
    let req = hash(&H.env, 1);

    H.validation.validation_request(
        &owner,
        &agent_id,
        &validator,
        &s(&H.env, "ipfs://request"),
        &req,
    );

    // Pending until the validator responds.
    let status = H.validation.get_validation_status(&req).unwrap();
    assert_eq!(status.response, PENDING);
    assert_eq!(status.validator, validator);

    H.validation.validation_response(
        &validator,
        &req,
        &80u32,
        &s(&H.env, "ipfs://resp"),
        &hash(&H.env, 2),
        &s(&H.env, "stake"),
    );
    let status = H.validation.get_validation_status(&req).unwrap();
    assert_eq!(status.response, 80);
    assert_eq!(status.tag, s(&H.env, "stake"));

    assert_eq!(H.validation.get_agent_validations(&agent_id).len(), 1);
    assert_eq!(H.validation.get_validator_requests(&validator).len(), 1);
}

#[test]
fn test_multi_response_finality() {
    let H = setup();
    let owner = Address::generate(&H.env);
    let validator = Address::generate(&H.env);
    let agent_id = H.identity.register(&owner, &s(&H.env, ""), &vec![&H.env]);
    let req = hash(&H.env, 7);

    H.validation
        .validation_request(&owner, &agent_id, &validator, &s(&H.env, ""), &req);
    // soft finality
    H.validation.validation_response(
        &validator, &req, &60u32, &s(&H.env, ""), &hash(&H.env, 8), &s(&H.env, "tee"),
    );
    // hard finality (re-called with refined score)
    H.validation.validation_response(
        &validator, &req, &95u32, &s(&H.env, ""), &hash(&H.env, 9), &s(&H.env, "tee"),
    );
    assert_eq!(H.validation.get_validation_status(&req).unwrap().response, 95);
}

#[test]
#[should_panic(expected = "requester is not owner or operator of agent")]
fn test_request_requires_owner() {
    let H = setup();
    let owner = Address::generate(&H.env);
    let stranger = Address::generate(&H.env);
    let validator = Address::generate(&H.env);
    let agent_id = H.identity.register(&owner, &s(&H.env, ""), &vec![&H.env]);
    H.validation.validation_request(
        &stranger,
        &agent_id,
        &validator,
        &s(&H.env, ""),
        &hash(&H.env, 1),
    );
}

#[test]
#[should_panic(expected = "caller is not the named validator")]
fn test_only_named_validator_responds() {
    let H = setup();
    let owner = Address::generate(&H.env);
    let validator = Address::generate(&H.env);
    let imposter = Address::generate(&H.env);
    let agent_id = H.identity.register(&owner, &s(&H.env, ""), &vec![&H.env]);
    let req = hash(&H.env, 3);
    H.validation
        .validation_request(&owner, &agent_id, &validator, &s(&H.env, ""), &req);
    H.validation.validation_response(
        &imposter, &req, &50u32, &s(&H.env, ""), &hash(&H.env, 4), &s(&H.env, ""),
    );
}

#[test]
fn test_summary_averages_responses() {
    let H = setup();
    let owner = Address::generate(&H.env);
    let v1 = Address::generate(&H.env);
    let v2 = Address::generate(&H.env);
    let agent_id = H.identity.register(&owner, &s(&H.env, ""), &vec![&H.env]);

    let r1 = hash(&H.env, 10);
    let r2 = hash(&H.env, 11);
    H.validation
        .validation_request(&owner, &agent_id, &v1, &s(&H.env, ""), &r1);
    H.validation
        .validation_request(&owner, &agent_id, &v2, &s(&H.env, ""), &r2);
    H.validation.validation_response(
        &v1, &r1, &100u32, &s(&H.env, ""), &hash(&H.env, 12), &s(&H.env, "zkml"),
    );
    H.validation.validation_response(
        &v2, &r2, &50u32, &s(&H.env, ""), &hash(&H.env, 13), &s(&H.env, "zkml"),
    );

    let (count, avg) = H.validation.get_summary(
        &agent_id,
        &vec![&H.env, v1.clone(), v2.clone()],
        &s(&H.env, ""),
    );
    assert_eq!(count, 2);
    assert_eq!(avg, 75);
}
