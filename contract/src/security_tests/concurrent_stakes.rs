use super::DepositToken;
use crate::LiquidStakingToken;
use crate::pool::StakeMessage;
use crate::test_utils::{context, id, pool, stake_args, success};
use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;
use near_sdk::json_types::U128;
use near_sdk::mock::MockAction;
use near_sdk::test_utils::{VMContextBuilder, get_created_receipts};
use near_sdk::{Gas, NearToken, PromiseOrValue, PromiseResult, testing_env};
use std::collections::HashMap;

fn accept_wnear_stake(
    pool: &mut LiquidStakingToken,
    sender: &str,
    amount: NearToken,
    args: &StakeMessage,
) {
    context("wrap.near", 300, NearToken::ZERO, vec![], 1);
    let msg = near_sdk::serde_json::to_string(args).unwrap();
    drop(pool.ft_on_transfer(id(sender), U128(amount.as_yoctonear()), msg));
}

fn callback_context(balance: NearToken, epoch: u64, results: Vec<PromiseResult>) {
    testing_env!(
        VMContextBuilder::new()
            .current_account_id(id("pool.near"))
            .predecessor_account_id(id("pool.near"))
            .account_balance(balance)
            .account_locked_balance(NearToken::from_near(10))
            .prepaid_gas(Gas::from_tgas(300))
            .epoch_height(epoch)
            .build(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        HashMap::default(),
        results,
    );
}

fn scheduled_call(method: &str) -> Option<(near_sdk::serde_json::Value, NearToken)> {
    get_created_receipts()
        .into_iter()
        .flat_map(|receipt| receipt.actions)
        .find_map(|action| match action {
            MockAction::FunctionCallWeight {
                method_name,
                args,
                attached_deposit,
                ..
            } if method_name == method.as_bytes() => Some((
                // Calls without arguments carry an empty body.
                near_sdk::serde_json::from_slice(&args).unwrap_or_default(),
                attached_deposit,
            )),
            _ => None,
        })
}

fn booked_by_scheduled_stake() -> NearToken {
    let (args, _) = scheduled_call("modify_state_before_stake").unwrap();
    near_sdk::serde_json::from_value(args["deposit_amount"].clone()).unwrap()
}

#[test]
fn concurrent_wnear_deposits_are_booked_before_their_near_arrives() {
    let mut pool = pool();
    let tracked = pool.get_total_balance();
    let staked = pool.get_total_staked_balance();
    let amount = NearToken::from_near(100);
    let both = amount.saturating_mul(2);

    accept_wnear_stake(&mut pool, "alice.near", amount, &stake_args());
    accept_wnear_stake(&mut pool, "bob.near", amount, &stake_args());
    assert_eq!(pool.get_total_balance(), tracked.saturating_add(both));

    // A new epoch starts after Alice's NEAR arrived but before Bob's did.
    callback_context(NearToken::from_near(1100), 2, success(vec![]));
    drop(pool.on_near_withdraw(id("alice.near"), U128(amount.as_yoctonear()), stake_args()));
    assert_eq!(pool.get_total_staked_balance(), staked);
    assert_eq!(booked_by_scheduled_stake(), NearToken::ZERO);
    pool.modify_state_before_stake(
        &id("alice.near"),
        &id("alice.near"),
        amount,
        NearToken::ZERO,
        amount,
    );

    callback_context(NearToken::from_near(1200), 2, success(vec![]));
    drop(pool.on_near_withdraw(id("bob.near"), U128(amount.as_yoctonear()), stake_args()));
    pool.modify_state_before_stake(
        &id("bob.near"),
        &id("alice.near"),
        amount,
        NearToken::ZERO,
        amount,
    );
    assert_eq!(pool.get_total_staked_balance(), staked.saturating_add(both));
    assert_eq!(pool.get_total_balance(), tracked.saturating_add(both));

    // Once both deposits arrived, only the validator reward is recognized.
    let reward = NearToken::from_near(5);
    callback_context(NearToken::from_near(1205), 2, vec![]);
    assert_eq!(pool.sync_rewards_internal(None), reward);
    assert_eq!(
        pool.get_total_staked_balance(),
        staked.saturating_add(both).saturating_add(reward)
    );
}

#[test]
fn native_deposits_are_still_booked_with_their_stake() {
    let mut pool = pool();
    let amount = NearToken::from_near(100);
    context("alice.near", 300, amount, vec![], 1);
    drop(pool.stake(stake_args()));
    assert_eq!(booked_by_scheduled_stake(), amount);
}

#[test]
fn failed_unwrap_reverses_the_booking() {
    let mut pool = pool();
    let tracked = pool.get_total_balance();
    let amount = NearToken::from_near(100);
    accept_wnear_stake(&mut pool, "alice.near", amount, &stake_args());

    callback_context(NearToken::from_near(1000), 2, vec![PromiseResult::Failed]);
    assert!(matches!(
        pool.on_near_withdraw(id("alice.near"), U128(amount.as_yoctonear()), stake_args()),
        PromiseOrValue::Value(U128(refund)) if refund == amount.as_yoctonear()
    ));
    assert_eq!(pool.get_total_balance(), tracked);
}

#[test]
fn refunded_wnear_deposits_leave_the_tracker() {
    let mut pool = pool();
    let tracked = pool.get_total_balance();

    // The deposit cannot pay for the requested registration, so staking fails.
    let tiny = NearToken::from_yoctonear(1);
    let args = StakeMessage {
        is_storage_deposit: true,
        ..stake_args()
    };
    accept_wnear_stake(&mut pool, "alice.near", tiny, &args);
    callback_context(
        NearToken::from_near(1000).saturating_add(tiny),
        2,
        success(vec![]),
    );
    drop(pool.on_near_withdraw(id("alice.near"), U128(tiny.as_yoctonear()), args));
    assert_eq!(pool.get_total_balance(), tracked);
    assert_eq!(scheduled_call("near_deposit").unwrap().1, tiny);

    // A failed state batch returns the whole booked deposit.
    let amount = NearToken::from_near(100);
    accept_wnear_stake(&mut pool, "alice.near", amount, &stake_args());
    callback_context(NearToken::from_near(1100), 2, vec![PromiseResult::Failed]);
    drop(pool.restake_or_refund(
        id("alice.near"),
        amount,
        amount,
        amount,
        stake_args(),
        DepositToken::Wnear,
    ));
    assert_eq!(pool.get_total_balance(), tracked);
    assert_eq!(scheduled_call("near_deposit").unwrap().1, amount);
}

#[test]
fn balance_below_the_tracker_postpones_rewards_without_failing() {
    let mut pool = pool();
    let tracked = pool.get_total_balance();
    callback_context(NearToken::from_near(990), 2, vec![]);
    assert_eq!(pool.sync_rewards_internal(None), NearToken::ZERO);
    assert_eq!(pool.get_total_balance(), tracked);
    assert_eq!(pool.statistics.last_epoch_synced, 1);
}
