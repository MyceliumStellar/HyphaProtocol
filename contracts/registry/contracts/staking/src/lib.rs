#![no_std]
//! # Hypha Staking (Stellar-native extension)
//!
//! NOT part of ERC-8004. ERC-8004 deliberately leaves validator incentives and slashing to external
//! protocols. This contract is the Stellar-native economic-security layer that backs the
//! `"stake"` mechanism of the Validation Registry: agents lock SEP-41 collateral that downstream
//! consumers (or a future slashing module) can weigh when deciding whether to trust an agent.

use soroban_sdk::{contract, contractevent, contractimpl, contracttype, token, Address, Env};

const LEDGER_THRESHOLD: u32 = 100_000;
const LEDGER_BUMP: u32 = 500_000;

#[contractevent]
#[derive(Clone)]
pub struct Staked {
    #[topic]
    pub agent: Address,
    pub amount: i128,
    pub total: i128,
}

#[contractevent]
#[derive(Clone)]
pub struct Unstaked {
    #[topic]
    pub agent: Address,
    pub amount: i128,
    pub total: i128,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// agent -> total staked collateral.
    Stake(Address),
}

#[contract]
pub struct Staking;

#[contractimpl]
impl Staking {
    /// Lock `amount` of `token` as collateral, transferred from the agent into this contract.
    pub fn deposit_stake(env: Env, agent: Address, token: Address, amount: i128) {
        agent.require_auth();
        assert!(amount > 0, "amount must be positive");

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&agent, &env.current_contract_address(), &amount);

        let key = DataKey::Stake(agent.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_stake = current + amount;
        env.storage().persistent().set(&key, &new_stake);
        env.storage()
            .persistent()
            .extend_ttl(&key, LEDGER_THRESHOLD, LEDGER_BUMP);

        Staked {
            agent,
            amount,
            total: new_stake,
        }
        .publish(&env);
    }

    /// Withdraw `amount` of previously-staked collateral back to the agent.
    pub fn unstake(env: Env, agent: Address, token: Address, amount: i128) {
        agent.require_auth();
        assert!(amount > 0, "amount must be positive");

        let key = DataKey::Stake(agent.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        assert!(current >= amount, "insufficient staked balance");

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &agent, &amount);

        let new_stake = current - amount;
        if new_stake == 0 {
            env.storage().persistent().remove(&key);
        } else {
            env.storage().persistent().set(&key, &new_stake);
            env.storage()
                .persistent()
                .extend_ttl(&key, LEDGER_THRESHOLD, LEDGER_BUMP);
        }

        Unstaked {
            agent,
            amount,
            total: new_stake,
        }
        .publish(&env);
    }

    pub fn get_stake(env: Env, agent: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Stake(agent))
            .unwrap_or(0)
    }
}

mod test;
