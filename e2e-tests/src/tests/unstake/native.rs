use crate::env::INIT_BALANCE;
use crate::env::{Env, INIT_LOCK};
use liquid_staking_token::pool::{Tranche, WithdrawTokens};
use near_api::NearToken;
use near_api::types::transaction::result::TransactionResultError;
use testresult::TestResult;

use crate::env::ft::FungibleToken;
use crate::env::intents::{Intents, IntentsSigner};
use crate::env::mt::MultiToken;
use crate::env::native::Native;
use crate::env::pool::StakingPool;
use crate::tests::{ONE_YOCTO, STAKE_AMOUNT, ZERO_AMOUNT, stake_message, unstake_message};

const DISTRIBUTION_NOT_FOUND_ERROR: &str =
    "There are no available tokens for withdrawal for this message hash";
const INSUFFICIENT_BALANCE_ERROR: &str = "The account doesn't have enough balance";
const ZERO_AMOUNT_ERROR: &str = "The amount should be a positive number";

fn assert_transaction_failure_contains<T>(result: anyhow::Result<T>, expected: &str) {
    let Err(error) = result else {
        panic!("Expected transaction to fail");
    };
    let tx_error = error
        .downcast_ref::<TransactionResultError>()
        .expect("Expected transaction result error");

    match tx_error {
        TransactionResultError::Failure(failure) => {
            let failure = failure.to_string();
            assert!(
                failure.contains(expected),
                "Expected transaction failure to contain `{expected}`, got `{failure}`"
            );
        }
        TransactionResultError::Pending(status) => {
            panic!("Expected transaction failure: {status:?}");
        }
    }
}

#[tokio::test]
async fn test_withdraw_before_cooldown_fails() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;

    let unstake_msg = unstake_message(alice.id(), &WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &unstake_msg)
        .await?;

    // Cooldown not elapsed — withdrawal must fail.
    let result = env.lst.withdraw(alice, &unstake_msg).await;
    assert!(
        result.is_err(),
        "Expected withdrawal to fail before cooldown"
    );

    // The unstake queue entry is still intact; waiting and retrying must succeed.
    env.wait_unstake_cooldown().await?;
    env.lst.withdraw(alice, &unstake_msg).await?;

    assert_eq!(
        alice
            .near_balance()
            .await?
            .total
            .saturating_add(NearToken::from_yoctonear(2)), // add_public_key + ft_transfer_call
        INIT_BALANCE
    );

    Ok(())
}

#[tokio::test]
async fn test_withdraw_nonexistent_stake_fails() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    // Alice never staked or unstaked, so the queue has no matching entry.
    let unstake_msg = unstake_message(alice.id(), &WithdrawTokens::Native);
    let result = env.lst.withdraw(alice, &unstake_msg).await;
    assert!(
        result.is_err(),
        "Expected withdrawal to fail when no matching unstake entry exists"
    );

    // State unchanged.
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(env.lst.near_balance().await?.locked, INIT_LOCK);

    Ok(())
}

#[tokio::test]
async fn test_withdraw_with_modified_unstake_message_fails() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&String>),
        )
        .await?;

    let original_unstake_message = unstake_message(alice.id(), &WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &original_unstake_message)
        .await?;

    assert_eq!(env.lst.get_total_pending_withdrawals().await?, STAKE_AMOUNT);

    env.wait_unstake_cooldown().await?;

    let modified_unstake_message = unstake_message(bob.id(), &WithdrawTokens::Native);
    let result = env.lst.withdraw(alice, &modified_unstake_message).await;
    assert_transaction_failure_contains(result, DISTRIBUTION_NOT_FOUND_ERROR);

    assert_eq!(env.lst.get_total_pending_withdrawals().await?, STAKE_AMOUNT);

    env.lst.withdraw(alice, &original_unstake_message).await?;

    assert_eq!(env.lst.get_total_pending_withdrawals().await?, ZERO_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_unstake_native_by_withdrawing_lst_from_intents() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(env.intents.id(), None, Some(alice.id())),
        )
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let intents_lst_balance = env.intents.mt_balance_of(alice.id(), env.lst.id()).await?;
    assert_eq!(intents_lst_balance, STAKE_AMOUNT);

    let unstake_message = unstake_message(alice.id(), &WithdrawTokens::Native);
    let withdraw_intent = alice
        .sign_withdraw_intent(
            env.intents.id(),
            env.lst.id(),
            env.lst.id(),
            STAKE_AMOUNT,
            Some(unstake_message.clone()),
        )
        .await;

    env.intents
        .execute_intents(alice.id(), vec![withdraw_intent])
        .await?;

    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);

    env.wait_unstake_cooldown().await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked.as_near(), INIT_LOCK.as_near());

    env.lst.withdraw(alice, &unstake_message).await?;

    assert_eq!(
        alice.near_balance().await?.total.saturating_add(ONE_YOCTO),
        INIT_BALANCE
    );

    Ok(())
}

#[tokio::test]
async fn test_unstake_native_by_sending_lst_back() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&String>),
        )
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let lst_balance = env.lst.ft_balance_of(alice.id()).await?;
    assert_eq!(lst_balance, STAKE_AMOUNT);

    let unstake_message = unstake_message(alice.id(), &WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &unstake_message)
        .await?;

    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);

    env.wait_unstake_cooldown().await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked.as_near(), INIT_LOCK.as_near());

    env.lst.withdraw(alice, &unstake_message).await?;

    assert_eq!(
        alice
            .near_balance()
            .await?
            .total
            .saturating_add(NearToken::from_yoctonear(2)), // add_public_key + ft_transfer_call
        INIT_BALANCE
    );

    Ok(())
}

#[tokio::test]
async fn test_partial_unstake_preserves_remaining_lst() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&String>),
        )
        .await?;

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);

    let partial = STAKE_AMOUNT.saturating_div(4); // unstake 25%
    let remaining = STAKE_AMOUNT.saturating_sub(partial);

    let unstake_msg = unstake_message(alice.id(), &WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(alice, env.lst.id(), partial, &unstake_msg)
        .await?;

    // 75% of LST must remain with alice; total supply tracks it.
    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, remaining);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(remaining)
    );

    env.wait_unstake_cooldown().await?;

    // Locked balance reflects only the staked portion.
    assert_eq!(
        env.lst.near_balance().await?.locked,
        INIT_LOCK
            .saturating_add(remaining)
            .saturating_add(ONE_YOCTO)
    );

    env.lst.withdraw(alice, &unstake_msg).await?;

    // After withdrawal the 75% of LST is untouched.
    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, remaining);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(remaining)
    );

    Ok(())
}

#[tokio::test]
async fn test_two_unstakes_to_native_by_sending_lst_from_wnear() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&String>),
        )
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let lst_balance = env.lst.ft_balance_of(alice.id()).await?;
    assert_eq!(lst_balance, STAKE_AMOUNT);

    let half_stake_amount = STAKE_AMOUNT.saturating_div(2);

    let unstake_message = unstake_message(alice.id(), &WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(alice, env.lst.id(), half_stake_amount, &unstake_message)
        .await?;
    env.lst
        .ft_transfer_call(alice, env.lst.id(), half_stake_amount, &unstake_message)
        .await?;

    let lst_balance = env.lst.ft_balance_of(alice.id()).await?;
    assert_eq!(lst_balance, ZERO_AMOUNT);

    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);

    env.wait_unstake_cooldown().await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked.as_near(), INIT_LOCK.as_near());

    env.lst.withdraw(alice, &unstake_message).await?;

    let wnear_intents_balance = env.wnear.ft_balance_of(env.intents.id()).await?;
    assert_eq!(wnear_intents_balance, ZERO_AMOUNT);

    assert_eq!(
        alice
            .near_balance()
            .await?
            .total
            .saturating_add(NearToken::from_yoctonear(3)), // add_public_key + ft_transfer_call + ft_transfer_call
        INIT_BALANCE
    );
    let intents_balance = env
        .intents
        .mt_balance_of(alice.id(), env.wnear.id())
        .await?;
    assert_eq!(intents_balance, ZERO_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_unstake_more_than_staked_amount_fails() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&String>),
        )
        .await?;

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);

    let unstake_message = unstake_message(alice.id(), &WithdrawTokens::Native);
    let result = env
        .lst
        .ft_transfer_call(
            alice,
            env.lst.id(),
            STAKE_AMOUNT.saturating_add(ONE_YOCTO),
            &unstake_message,
        )
        .await;
    assert_transaction_failure_contains(result, INSUFFICIENT_BALANCE_ERROR);

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, ZERO_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_unstake_with_invalid_message_format_refunds_and_skips_queue() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&String>),
        )
        .await?;

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);

    // The receiver panic is resolved by NEP-141 refund, so the transfer call succeeds
    // while the unstake itself leaves no queue entry behind.
    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, "invalid message")
        .await?;

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, ZERO_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_unstake_zero_tokens_fails() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&String>),
        )
        .await?;

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);

    let unstake_message = unstake_message(alice.id(), &WithdrawTokens::Native);
    let result = env
        .lst
        .ft_transfer_call(alice, env.lst.id(), ZERO_AMOUNT, &unstake_message)
        .await;
    assert_transaction_failure_contains(result, ZERO_AMOUNT_ERROR);

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, ZERO_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_stake_native_near_by_itself_and_unstake_native_to_itself() -> TestResult {
    let env = Env::builder().build().await?;
    let lst_init_balance = env.lst.near_balance().await?;

    env.lst
        .stake(
            &env.lst.as_account(),
            STAKE_AMOUNT,
            stake_message(env.lst.id(), None, None::<&String>),
        )
        .await?;

    let locked_balance = env.lst.near_balance().await?.locked;
    assert_eq!(locked_balance, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let lst_balance = env.lst.ft_balance_of(env.lst.id()).await?;
    assert_eq!(lst_balance, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let unstake_message = unstake_message(env.lst.id(), &WithdrawTokens::Native);

    env.lst
        .ft_on_transfer(
            &env.lst.as_account(),
            env.lst.id(),
            STAKE_AMOUNT,
            &unstake_message,
        )
        .await?;

    assert_eq!(env.lst.ft_balance_of(env.lst.id()).await?, INIT_LOCK);
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);

    env.wait_unstake_cooldown().await?;

    env.lst
        .withdraw(&env.lst.as_account(), &unstake_message)
        .await?;

    assert_eq!(env.lst.near_balance().await?, lst_init_balance);

    Ok(())
}

#[tokio::test]
async fn test_stake_native_near_by_itself_and_unstake_native_to_alice() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    let alice_init_balance = alice.near_balance().await?;
    let lst_init_balance = env.lst.near_balance().await?;

    env.lst
        .stake(
            &env.lst.as_account(),
            STAKE_AMOUNT,
            stake_message(env.lst.id(), None, None::<&String>),
        )
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let lst_balance = env.lst.ft_balance_of(env.lst.id()).await?;
    assert_eq!(lst_balance, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let unstake_message = unstake_message(alice.id(), &WithdrawTokens::Native);

    env.lst
        .ft_on_transfer(
            &env.lst.as_account(),
            env.lst.id(),
            STAKE_AMOUNT,
            &unstake_message,
        )
        .await?;

    assert_eq!(env.lst.ft_balance_of(env.lst.id()).await?, INIT_LOCK);
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);

    env.wait_unstake_cooldown().await?;

    env.lst
        .withdraw(&env.lst.as_account(), &unstake_message)
        .await?;

    assert_eq!(
        env.lst.near_balance().await?.total,
        lst_init_balance.total.saturating_sub(STAKE_AMOUNT)
    );

    assert_eq!(
        alice.near_balance().await?.total,
        alice_init_balance.total.saturating_add(STAKE_AMOUNT)
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_native_near_by_alice_and_bob_and_unstake_using_withdraw() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();
    let alice_init_balance = alice.near_balance().await?;
    let bob_init_balance = bob.near_balance().await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&String>),
        )
        .await?;
    env.lst
        .stake(
            bob,
            STAKE_AMOUNT,
            stake_message(bob.id(), None, None::<&String>),
        )
        .await?;

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);
    assert_eq!(env.lst.ft_balance_of(bob.id()).await?, STAKE_AMOUNT);
    assert_eq!(
        env.lst.get_total_staked_balance().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT.saturating_mul(2))
    );
    assert_eq!(
        env.lst.get_total_balance().await?,
        INIT_BALANCE.saturating_add(STAKE_AMOUNT.saturating_mul(2))
    );

    env.wait_for_epochs(1).await?;

    assert_eq!(
        env.lst.near_balance().await?.locked,
        INIT_LOCK.saturating_add(STAKE_AMOUNT.saturating_mul(2))
    );

    assert_eq!(
        env.lst
            .get_hashes_available_for_withdrawal(0, 10)
            .await?
            .len(),
        0
    );

    let alice_unstake_message = unstake_message(alice.id(), &WithdrawTokens::Native);
    let bob_unstake_message = unstake_message(bob.id(), &WithdrawTokens::Native);

    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &alice_unstake_message)
        .await?;
    env.lst
        .ft_transfer_call(bob, env.lst.id(), STAKE_AMOUNT, &bob_unstake_message)
        .await?;
    let unstake_epoch = env.epoch_height(None).await?;

    assert_eq!(env.lst.get_withdrawal_requests_count().await?, 2);
    assert_eq!(env.lst.get_withdrawal_requests(0, 10).await?.len(), 2);

    env.wait_unstake_cooldown().await?;

    assert_eq!(
        env.lst.get_hashes_available_for_withdrawal(0, 10).await?,
        vec![alice_unstake_message.hash()?, bob_unstake_message.hash()?]
    );

    let alice_tranche = env
        .lst
        .get_withdrawal_request_tranches(alice_unstake_message.hash()?)
        .await?;
    let bob_tranche = env
        .lst
        .get_withdrawal_request_tranches(bob_unstake_message.hash()?)
        .await?;
    assert_eq!(
        alice_tranche,
        Some(vec![Tranche {
            withdrawal_amount: STAKE_AMOUNT,
            unstake_epoch,
            wnear_residual: NearToken::default(),
            storage_was_paid: false,
        }])
    );
    assert_eq!(
        bob_tranche,
        Some(vec![Tranche {
            withdrawal_amount: STAKE_AMOUNT,
            unstake_epoch,
            wnear_residual: NearToken::default(),
            storage_was_paid: false,
        }])
    );

    let alice_withdrawal_request = env.lst.withdraw(alice, alice_unstake_message);
    let bob_withdrawal_request = env.lst.withdraw(bob, bob_unstake_message);

    tokio::try_join!(alice_withdrawal_request, bob_withdrawal_request)?;

    assert_eq!(
        alice.near_balance().await?.total,
        alice_init_balance.total.saturating_sub(ONE_YOCTO)
    );
    assert_eq!(
        bob.near_balance().await?.total,
        bob_init_balance.total.saturating_sub(ONE_YOCTO)
    );

    assert_eq!(env.lst.get_withdrawal_requests_count().await?, 0);
    assert_eq!(env.lst.get_withdrawal_requests(0, 10).await?.len(), 0);

    let hashes = env.lst.get_hashes_available_for_withdrawal(0, 10).await?;
    assert!(hashes.is_empty());

    Ok(())
}
