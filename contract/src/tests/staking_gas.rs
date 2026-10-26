use crate::pool::{DepositToken, StakeMessage};
use crate::tests::{context, id, pool, stake_args};
use near_contract_standards::fungible_token::FungibleTokenCore;
use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;
use near_sdk::json_types::U128;
use near_sdk::test_utils::get_created_receipts;
use near_sdk::{Gas, NearToken, PromiseResult};
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
fn native_stake_rejects_low_gas_before_state_or_promises() {
    let mut pool = pool();
    let balance = pool.get_total_balance();
    let supply = pool.ft_total_supply();
    for gas in [26, 30, 58] {
        context("alice.near", gas, NearToken::from_near(100), vec![], 1);
        assert!(catch_unwind(AssertUnwindSafe(|| pool.stake(stake_args()))).is_err());
        assert_eq!(pool.get_total_balance(), balance);
        assert_eq!(pool.ft_total_supply(), supply);
        assert!(get_created_receipts().is_empty());
    }
}

#[test]
fn wnear_stake_rejects_low_gas_before_unwrapping() {
    let mut pool = pool();
    let msg = near_sdk::serde_json::to_string(&stake_args()).unwrap();
    for gas in [14, 26, 50, 70] {
        context("wrap.near", gas, NearToken::ZERO, vec![], 1);
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                pool.ft_on_transfer(id("alice.near"), U128(100), msg.clone())
            }))
            .is_err()
        );
        assert!(get_created_receipts().is_empty());
        assert_eq!(
            pool.ft_total_supply(),
            U128(NearToken::from_near(10).as_yoctonear())
        );
    }
}

#[test]
fn stake_checks_custom_receiver_gas_and_message_size_before_unwrapping() {
    let mut pool = pool();
    for args in [
        StakeMessage {
            msg: Some("0".to_owned()),
            min_gas: Some(Gas::from_tgas(280)),
            ..stake_args()
        },
        StakeMessage {
            msg: Some("x".repeat(4096)),
            ..stake_args()
        },
    ] {
        context("wrap.near", 300, NearToken::ZERO, vec![], 1);
        let msg = near_sdk::serde_json::to_string(&args).unwrap();
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                pool.ft_on_transfer(id("alice.near"), U128(100), msg)
            }))
            .is_err()
        );
        assert!(get_created_receipts().is_empty());
    }
}

#[test]
fn failed_validator_stake_rolls_back_supply_and_schedules_native_refund() {
    let mut pool = pool();
    let amount = NearToken::from_near(100);
    let args = stake_args();
    let balance = pool.get_total_balance();
    let stake = pool.get_total_staked_balance();
    let supply = pool.ft_total_supply();
    pool.modify_state_before_stake(&id("alice.near"), &args.receiver_id, amount, amount, amount);
    context(
        "pool.near",
        50,
        NearToken::ZERO,
        vec![PromiseResult::Failed],
        1,
    );
    drop(pool.on_restake_or_refund(id("alice.near"), amount, amount, args, DepositToken::Native));
    assert_eq!(pool.get_total_balance(), balance);
    assert_eq!(pool.get_total_staked_balance(), stake);
    assert_eq!(pool.ft_total_supply(), supply);
    assert!(get_created_receipts().iter().any(|receipt| {
        receipt.receiver_id == id("alice.near") && receipt.actions.iter().any(|action| {
            matches!(action, near_sdk::mock::MockAction::Transfer { deposit, .. } if *deposit == amount)
        })
    }));
}
