use crate::ONE_YOCTO;
use crate::pool::WithdrawTokens;
use crate::test_utils::{context, locked_withdrawal, pool, success, withdrawal};
use near_sdk::NearToken;
use near_sdk::json_types::U128;

#[test]
fn wnear_partial_delivery_tracks_the_outgoing_yocto_deposit() {
    let mut pool = pool();
    let amount = NearToken::from_near(100);
    let consumed = NearToken::from_near(40);
    let args = withdrawal(WithdrawTokens::Wnear {
        is_storage_deposit: true,
        msg: Some("0".to_owned()),
        memo: None,
        min_gas: None,
    });
    let storage = crate::storage::FT_STORAGE_DEPOSIT;
    let send = amount.saturating_sub(storage);
    locked_withdrawal(&mut pool, &args, amount);
    let balance = pool.get_total_balance();
    context(
        "pool.near",
        15,
        NearToken::ZERO,
        success(near_sdk::serde_json::to_vec(&U128(consumed.as_yoctonear())).unwrap()),
        5,
    );
    pool.on_withdraw_wnear(
        args.hash().unwrap(),
        send,
        send.saturating_add(ONE_YOCTO),
        storage,
        true,
    );
    assert_eq!(
        pool.get_total_balance(),
        balance.saturating_sub(amount).saturating_sub(ONE_YOCTO)
    );
    let residual = send.saturating_sub(consumed);
    assert_eq!(pool.get_total_pending_withdrawals(), residual);
    let prior = pool
        .get_withdrawal_request_tranches(args.hash().unwrap())
        .unwrap();
    assert_eq!(prior[0].wnear_residual, residual);
    assert!(prior[0].storage_was_paid);

    context("pool.near", 5, NearToken::ZERO, success(vec![]), 5);
    pool.remove_lock(args.hash().unwrap());
    context("alice.near", 300, NearToken::ZERO, vec![], 5);
    drop(pool.withdraw(args.clone()));
    context(
        "pool.near",
        15,
        NearToken::ZERO,
        success(near_sdk::serde_json::to_vec(&U128(residual.as_yoctonear())).unwrap()),
        5,
    );
    pool.on_withdraw_wnear(
        args.hash().unwrap(),
        residual,
        ONE_YOCTO,
        NearToken::ZERO,
        true,
    );
    assert_eq!(
        pool.get_total_balance(),
        balance
            .saturating_sub(amount)
            .saturating_sub(ONE_YOCTO.saturating_mul(2))
    );
    assert_eq!(pool.get_total_pending_withdrawals(), NearToken::ZERO);
    assert!(
        pool.get_withdrawal_request_tranches(args.hash().unwrap())
            .is_none()
    );
}
