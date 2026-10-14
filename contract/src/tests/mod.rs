use std::collections::HashMap;

use near_contract_standards::fungible_token::metadata::{FT_METADATA_SPEC, FungibleTokenMetadata};
use near_sdk::test_utils::VMContextBuilder;
use near_sdk::{AccountId, Gas, NearToken, PromiseResult, testing_env};

use crate::LiquidStakingToken;
use crate::pool::{StakeMessage, UnstakeMessage, WithdrawTokens};

mod concurrent_stakes;
mod holder_count;
mod protected_accounts;
mod requests;
mod settlement_lock;
mod staking_gas;
mod transfer_accounting;
mod withdrawal_gas;

pub fn id(name: &str) -> AccountId {
    name.parse().unwrap()
}

pub fn context(
    sender: &str,
    gas: u64,
    deposit: NearToken,
    results: Vec<PromiseResult>,
    epoch: u64,
) {
    testing_env!(
        VMContextBuilder::new()
            .current_account_id(id("pool.near"))
            .predecessor_account_id(id(sender))
            .signer_account_id(id(sender))
            .account_balance(NearToken::from_near(1000))
            .account_locked_balance(NearToken::from_near(10))
            .prepaid_gas(Gas::from_tgas(gas))
            .attached_deposit(deposit)
            .epoch_height(epoch)
            .build(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        HashMap::default(),
        results,
    );
}

pub fn pool() -> LiquidStakingToken {
    context("owner.near", 300, NearToken::ZERO, vec![], 1);
    let mut pool = LiquidStakingToken::new(
        id("owner.near"),
        id("wrap.near"),
        id("treasury.near"),
        "ed25519:11111111111111111111111111111111".parse().unwrap(),
        FungibleTokenMetadata {
            spec: FT_METADATA_SPEC.to_owned(),
            name: "LST".to_owned(),
            symbol: "LST".to_owned(),
            icon: None,
            reference: None,
            reference_hash: None,
            decimals: 24,
        },
    );
    pool.statistics.last_epoch_synced = 1;
    for name in ["alice.near", "bob.near", "receiver.near"] {
        pool.token.internal_register_account(&id(name));
    }
    pool
}

pub fn stake_args() -> StakeMessage {
    StakeMessage {
        receiver_id: id("alice.near"),
        msg: None,
        memo: None,
        is_storage_deposit: false,
        min_gas: None,
        refund_message: None,
    }
}

pub fn withdrawal(tokens: WithdrawTokens) -> UnstakeMessage {
    UnstakeMessage {
        receiver_id: id("alice.near"),
        withdraw_tokens: tokens,
    }
}

pub fn locked_withdrawal(pool: &mut LiquidStakingToken, args: &UnstakeMessage, amount: NearToken) {
    pool.withdrawal_requests
        .append_request(1, args.hash().unwrap(), amount);
    pool.statistics.increase_pending_withdrawals(amount);
    context("alice.near", 300, NearToken::ZERO, vec![], 5);
    drop(pool.withdraw(args.clone()));
}

pub fn success(bytes: Vec<u8>) -> Vec<PromiseResult> {
    vec![PromiseResult::Successful(bytes)]
}
