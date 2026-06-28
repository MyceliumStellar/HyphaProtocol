#![cfg(test)]

use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::Env;

#[test]
fn test_deposit_and_unstake() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &contract_id);

    let agent = Address::generate(&env);
    let token_admin = Address::generate(&env);

    // Deploy a Stellar Asset Contract (e.g. testnet USDC) to back the stake.
    let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_addr = sac.address();
    let token_admin_client = StellarAssetClient::new(&env, &token_addr);
    let token_client = TokenClient::new(&env, &token_addr);

    token_admin_client.mint(&agent, &1000);
    assert_eq!(token_client.balance(&agent), 1000);

    client.deposit_stake(&agent, &token_addr, &400);
    assert_eq!(token_client.balance(&agent), 600);
    assert_eq!(token_client.balance(&contract_id), 400);
    assert_eq!(client.get_stake(&agent), 400);

    client.unstake(&agent, &token_addr, &150);
    assert_eq!(token_client.balance(&agent), 750);
    assert_eq!(token_client.balance(&contract_id), 250);
    assert_eq!(client.get_stake(&agent), 250);

    // Fully unstake clears the balance.
    client.unstake(&agent, &token_addr, &250);
    assert_eq!(client.get_stake(&agent), 0);
}

#[test]
#[should_panic(expected = "insufficient staked balance")]
fn test_unstake_too_much_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(Staking, ());
    let client = StakingClient::new(&env, &contract_id);

    let agent = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_addr = sac.address();
    StellarAssetClient::new(&env, &token_addr).mint(&agent, &100);

    client.deposit_stake(&agent, &token_addr, &100);
    client.unstake(&agent, &token_addr, &101);
}
