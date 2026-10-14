use crate::pool::WithdrawTokens;
use crate::tests::{context, id, locked_withdrawal, pool, success, withdrawal};
use near_sdk::{NearToken, PromiseResult};
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
fn native_delivery_failure_unlocks_the_unpaid_claim() {
    let mut pool = pool();
    let args = withdrawal(WithdrawTokens::Native);
    let amount = NearToken::from_near(100);
    locked_withdrawal(&mut pool, &args, amount);
    let balance = pool.get_total_balance();
    context(
        "pool.near",
        15,
        NearToken::ZERO,
        vec![PromiseResult::Failed],
        5,
    );
    pool.on_withdraw_native(args.hash().unwrap(), amount);
    assert_eq!(pool.get_total_balance(), balance);
    assert_eq!(pool.get_total_pending_withdrawals(), amount);
    assert_eq!(
        pool.get_hashes_available_for_withdrawal(0, 10),
        vec![args.hash().unwrap()]
    );
}

#[test]
fn successive_native_attempts_settle_each_claim_once() {
    assert_successive_attempts_settle_once(WithdrawTokens::Native);
}

#[test]
fn successive_wnear_attempts_settle_each_claim_once() {
    assert_successive_attempts_settle_once(WithdrawTokens::Wnear {
        is_storage_deposit: false,
        msg: None,
        memo: None,
        min_gas: None,
    });
}

fn assert_successive_attempts_settle_once(tokens: WithdrawTokens) {
    let mut pool = pool();
    let is_native = matches!(tokens, WithdrawTokens::Native);
    let args = withdrawal(tokens);
    let hash = args.hash().unwrap();
    let first = NearToken::from_near(1);
    let second = NearToken::from_near(100);
    let mut victim_args = args.clone();
    victim_args.receiver_id = id("bob.near");
    let victim_hash = victim_args.hash().unwrap();
    pool.withdrawal_requests.append_request(1, hash, first);
    pool.withdrawal_requests.append_request(2, hash, second);
    pool.withdrawal_requests
        .append_request(1, victim_hash, second);
    pool.statistics
        .increase_pending_withdrawals(first.saturating_add(second.saturating_mul(2)));

    context("alice.near", 300, NearToken::ZERO, vec![], 5);
    drop(pool.withdraw(args.clone()));
    context("pool.near", 15, NearToken::ZERO, success(vec![]), 5);
    if is_native {
        pool.on_withdraw_native(hash, first);
    } else {
        pool.on_withdraw_wnear(hash, first, first, NearToken::ZERO, false);
    }

    // Only the successor's own settlement may consume or unlock its claim.
    context("alice.near", 300, NearToken::ZERO, vec![], 6);
    drop(pool.withdraw(args.clone()));
    assert!(
        !pool
            .get_hashes_available_for_withdrawal(0, 10)
            .contains(&hash)
    );
    assert_eq!(
        pool.withdrawal_requests
            .locked_tranche(&hash)
            .withdrawal_amount,
        second
    );

    context("pool.near", 15, NearToken::ZERO, success(vec![]), 6);
    if is_native {
        pool.on_withdraw_native(hash, second);
    } else {
        pool.on_withdraw_wnear(hash, second, second, NearToken::ZERO, false);
    }
    assert!(pool.get_withdrawal_request_tranches(hash).is_none());
    assert_eq!(pool.get_total_pending_withdrawals(), second);
    assert_eq!(
        pool.get_withdrawal_request_tranches(victim_hash).unwrap()[0].withdrawal_amount,
        second
    );
    context("alice.near", 300, NearToken::ZERO, vec![], 6);
    assert!(catch_unwind(AssertUnwindSafe(|| pool.withdraw(args))).is_err());
}

#[test]
fn partial_wnear_settlement_unlocks_only_its_residual_before_retry() {
    let mut pool = pool();
    let args = withdrawal(WithdrawTokens::Wnear {
        is_storage_deposit: false,
        msg: Some("0".to_owned()),
        memo: None,
        min_gas: None,
    });
    let hash = args.hash().unwrap();
    let amount = NearToken::from_near(100);
    let consumed = NearToken::from_near(40);
    let remaining = amount.saturating_sub(consumed);
    locked_withdrawal(&mut pool, &args, amount);
    context(
        "pool.near",
        15,
        NearToken::ZERO,
        success(
            near_sdk::serde_json::to_vec(&near_sdk::json_types::U128(consumed.as_yoctonear()))
                .unwrap(),
        ),
        5,
    );
    pool.on_withdraw_wnear(hash, amount, amount, NearToken::ZERO, true);
    assert_eq!(pool.get_total_pending_withdrawals(), remaining);
    assert!(
        pool.get_hashes_available_for_withdrawal(0, 10)
            .contains(&hash)
    );
    assert_eq!(
        pool.get_withdrawal_request_tranches(hash).unwrap()[0].wnear_residual,
        remaining
    );

    context("alice.near", 300, NearToken::ZERO, vec![], 5);
    drop(pool.withdraw(args));
    assert!(
        !pool
            .get_hashes_available_for_withdrawal(0, 10)
            .contains(&hash)
    );
    context(
        "pool.near",
        15,
        NearToken::ZERO,
        success(
            near_sdk::serde_json::to_vec(&near_sdk::json_types::U128(remaining.as_yoctonear()))
                .unwrap(),
        ),
        5,
    );
    pool.on_withdraw_wnear(hash, remaining, NearToken::ZERO, NearToken::ZERO, true);
    assert!(pool.get_withdrawal_request_tranches(hash).is_none());
    assert_eq!(pool.get_total_pending_withdrawals(), NearToken::ZERO);
}

#[test]
fn failed_plain_wnear_delivery_unlocks_without_a_cleanup_callback() {
    let mut pool = pool();
    let args = withdrawal(WithdrawTokens::Wnear {
        is_storage_deposit: false,
        msg: None,
        memo: None,
        min_gas: None,
    });
    let amount = NearToken::from_near(100);
    locked_withdrawal(&mut pool, &args, amount);
    let balance = pool.get_total_balance();
    context(
        "pool.near",
        15,
        NearToken::ZERO,
        vec![PromiseResult::Failed],
        5,
    );
    pool.on_withdraw_wnear(args.hash().unwrap(), amount, amount, NearToken::ZERO, false);
    assert_eq!(pool.get_total_balance(), balance);
    assert_eq!(pool.get_total_pending_withdrawals(), amount);
    assert!(
        pool.get_hashes_available_for_withdrawal(0, 10)
            .contains(&args.hash().unwrap())
    );
}

#[test]
fn native_settlement_rejects_missing_or_mismatched_locked_claim_before_accounting() {
    let mut pool = pool();
    let args = withdrawal(WithdrawTokens::Native);
    let hash = args.hash().unwrap();
    let amount = NearToken::from_near(100);
    locked_withdrawal(&mut pool, &args, amount);
    let balance = pool.get_total_balance();
    context("pool.near", 15, NearToken::ZERO, success(vec![]), 5);
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            pool.on_withdraw_native(hash, NearToken::from_near(1));
        }))
        .is_err()
    );
    assert_eq!(pool.get_total_balance(), balance);
    assert_eq!(pool.get_total_pending_withdrawals(), amount);
    assert!(
        !pool
            .get_hashes_available_for_withdrawal(0, 10)
            .contains(&hash)
    );

    pool.withdrawal_requests.release_lock(&hash);
    assert!(catch_unwind(AssertUnwindSafe(|| pool.on_withdraw_native(hash, amount))).is_err());
    assert_eq!(pool.get_total_balance(), balance);
    assert_eq!(pool.get_total_pending_withdrawals(), amount);
}

#[test]
fn malformed_successful_wnear_result_keeps_the_claim_locked() {
    let mut pool = pool();
    let args = withdrawal(WithdrawTokens::Wnear {
        is_storage_deposit: false,
        msg: Some("0".to_owned()),
        memo: None,
        min_gas: None,
    });
    let amount = NearToken::from_near(100);
    locked_withdrawal(&mut pool, &args, amount);
    let balance = pool.get_total_balance();
    for bytes in [b"null".to_vec(), vec![b'x'; 4096]] {
        context("pool.near", 15, NearToken::ZERO, success(bytes), 5);
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                pool.on_withdraw_wnear(args.hash().unwrap(), amount, amount, NearToken::ZERO, true);
            }))
            .is_err()
        );
        assert!(pool.get_hashes_available_for_withdrawal(0, 10).is_empty());
        assert_eq!(pool.get_total_balance(), balance);
        assert_eq!(pool.get_total_pending_withdrawals(), amount);
    }
}
