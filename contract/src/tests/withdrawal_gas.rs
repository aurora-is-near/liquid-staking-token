use crate::pool::WithdrawTokens;
use crate::tests::{context, pool, withdrawal};
use near_sdk::NearToken;
use near_sdk::test_utils::get_created_receipts;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
fn low_gas_withdrawal_does_not_lock_or_send_claim() {
    let mut pool = pool();
    let args = withdrawal(WithdrawTokens::Native);
    let hash = args.hash().unwrap();
    pool.withdrawal_requests
        .append_request(1, hash, NearToken::from_near(100));
    context("alice.near", 9, NearToken::ZERO, vec![], 5);
    assert!(catch_unwind(AssertUnwindSafe(|| pool.withdraw(args))).is_err());
    assert!(get_created_receipts().is_empty());
    assert_eq!(pool.get_hashes_available_for_withdrawal(0, 10), vec![hash]);
}
