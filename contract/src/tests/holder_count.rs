use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::ONE_YOCTO;
use crate::pool::WithdrawTokens;
use crate::tests::{context, id, pool, success, withdrawal};
use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;
use near_contract_standards::fungible_token::{FungibleTokenCore, FungibleTokenResolver};
use near_contract_standards::storage_management::StorageManagement;
use near_sdk::NearToken;
use near_sdk::json_types::U128;

#[test]
fn resolver_counts_only_refund_transitions_after_interleaved_transfer() {
    let mut pool = pool();
    pool.internal_deposit(&id("alice.near"), NearToken::from_yoctonear(100));
    pool.internal_deposit(&id("bob.near"), NearToken::from_yoctonear(100));
    context("alice.near", 300, ONE_YOCTO, vec![], 1);
    drop(pool.ft_transfer_call(id("receiver.near"), U128(100), None, "0".to_owned()));
    assert_eq!(pool.get_number_of_accounts(), 3);

    context("bob.near", 300, ONE_YOCTO, vec![], 1);
    pool.ft_transfer(id("alice.near"), U128(100), None);
    context(
        "pool.near",
        15,
        NearToken::ZERO,
        success(b"\"100\"".to_vec()),
        1,
    );
    assert_eq!(
        pool.ft_resolve_transfer(id("alice.near"), id("receiver.near"), U128(100)),
        U128(0)
    );
    assert_eq!(pool.get_number_of_accounts(), 2);
    assert_eq!(pool.ft_balance_of(id("alice.near")), U128(200));
}

#[test]
fn repeated_full_unstakes_do_not_decrement_empty_contract_in_resolver() {
    let mut pool = pool();
    let message = withdrawal(WithdrawTokens::Native);
    let msg = near_sdk::serde_json::to_string(&message).unwrap();
    for _ in 0..10 {
        context("pool.near", 300, NearToken::ZERO, vec![], 1);
        pool.internal_deposit(&id("alice.near"), NearToken::from_yoctonear(100));
        context("alice.near", 300, ONE_YOCTO, vec![], 1);
        drop(pool.ft_transfer_call(id("pool.near"), U128(100), None, msg.clone()));
        context("pool.near", 100, NearToken::ZERO, vec![], 1);
        drop(pool.ft_on_transfer(id("alice.near"), U128(100), msg.clone()));
        context(
            "pool.near",
            15,
            NearToken::ZERO,
            success(b"\"0\"".to_vec()),
            1,
        );
        assert_eq!(
            pool.ft_resolve_transfer(id("alice.near"), id("pool.near"), U128(100)),
            U128(100)
        );
        assert_eq!(pool.get_number_of_accounts(), 1);
    }
}

#[test]
fn zero_mint_and_force_unregister_keep_holder_count_correct() {
    let mut pool = pool();
    pool.internal_deposit(&id("alice.near"), NearToken::ZERO);
    assert_eq!(pool.get_number_of_accounts(), 1);
    pool.internal_deposit(&id("alice.near"), NearToken::from_yoctonear(100));
    assert_eq!(pool.get_number_of_accounts(), 2);
    context("alice.near", 300, ONE_YOCTO, vec![], 1);
    assert!(pool.storage_unregister(Some(true)));
    assert_eq!(pool.get_number_of_accounts(), 1);
    assert_eq!(pool.ft_balance_of(id("alice.near")), U128(0));
    assert!(!pool.storage_unregister(Some(true)));
    assert_eq!(pool.get_number_of_accounts(), 1);
}

#[test]
fn unregistering_zero_balances_keeps_holder_count_for_all_force_options() {
    let mut pool = pool();
    let supply = pool.ft_total_supply();
    for (account, force) in [
        ("alice.near", None),
        ("bob.near", Some(false)),
        ("receiver.near", Some(true)),
    ] {
        context(account, 300, ONE_YOCTO, vec![], 1);
        assert!(pool.storage_unregister(force));
        assert_eq!(pool.get_number_of_accounts(), 1);
        assert_eq!(pool.ft_total_supply(), supply);
        assert!(pool.storage_balance_of(id(account)).is_none());
    }
}

#[test]
fn non_forced_unregister_keeps_a_positive_balance_and_holder_count() {
    let mut pool = pool();
    pool.internal_deposit(&id("alice.near"), NearToken::from_yoctonear(100));
    let supply = pool.ft_total_supply();
    for force in [None, Some(false)] {
        context("alice.near", 300, ONE_YOCTO, vec![], 1);
        assert!(catch_unwind(AssertUnwindSafe(|| pool.storage_unregister(force))).is_err());
        assert_eq!(pool.get_number_of_accounts(), 2);
        assert_eq!(pool.ft_balance_of(id("alice.near")), U128(100));
        assert_eq!(pool.ft_total_supply(), supply);
        assert!(pool.storage_balance_of(id("alice.near")).is_some());
    }
}

#[test]
fn an_already_corrupted_holder_count_does_not_block_burning() {
    let mut pool = pool();
    pool.statistics.num_delegators = 0;
    pool.internal_withdraw(&id("treasury.near"), NearToken::from_near(10));
    assert_eq!(pool.get_number_of_accounts(), 0);
}
