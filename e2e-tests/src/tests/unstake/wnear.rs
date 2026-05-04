use liquid_staking_token::pool::WithdrawTokens;
use near_api::NearToken;
use near_api::types::transaction::result::TransactionResultError;
use testresult::TestResult;

use crate::env::ft::{FT_STORAGE_DEPOSIT, FungibleToken};
use crate::env::intents::{Intents, IntentsSigner};
use crate::env::mt::MultiToken;
use crate::env::native::Native;
use crate::env::pool::StakingPool;
use crate::env::wnear::WNear;
use crate::env::{Env, INIT_BALANCE, INIT_LOCK};
use crate::tests::{ONE_YOCTO, STAKE_AMOUNT, ZERO_AMOUNT, stake_message, unstake_message};

const STORAGE_DEPOSIT_EXCEEDS_WITHDRAWAL_ERROR: &str =
    "Storage deposit exceeds the withdrawal amount";
const SELF_WITHDRAW_STORAGE_DEPOSIT_ERROR: &str =
    "There couldn't be a storage_deposit for the current account withdrawal";

fn refund_once_message(refund_amount: NearToken) -> String {
    near_sdk::serde_json::json!({
        "refund_once": true,
        "refund_amount": refund_amount,
    })
    .to_string()
}

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
async fn test_unstake_by_withdrawing_lst_from_intents() -> TestResult {
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

    let unstake_message = unstake_message(
        env.intents.id(),
        &WithdrawTokens::Wnear {
            storage_deposit: None,
            msg: Some(alice.id().to_string()),
            memo: None,
            min_gas: None,
        },
    );
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

    let wnear_intents_balance = env.wnear.ft_balance_of(env.intents.id()).await?;
    assert_eq!(wnear_intents_balance, STAKE_AMOUNT);

    let intents_balance = env
        .intents
        .mt_balance_of(alice.id(), env.wnear.id())
        .await?;
    assert_eq!(intents_balance, STAKE_AMOUNT);
    assert_eq!(
        alice.near_balance().await?.total.as_millinear() + 1,
        INIT_BALANCE.saturating_sub(STAKE_AMOUNT).as_millinear()
    );

    Ok(())
}

#[tokio::test]
async fn test_unstake_by_withdrawing_lst_from_intents_without_storage_deposit() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;
    let alice = env.alice();
    let near_balance = alice.near_balance().await?;

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

    let unstake_message = unstake_message(
        alice.id(),
        &WithdrawTokens::Wnear {
            storage_deposit: Some(FT_STORAGE_DEPOSIT),
            msg: None,
            memo: None,
            min_gas: None,
        },
    );
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

    let wnear_intents_balance = env.wnear.ft_balance_of(alice.id()).await?;

    assert_eq!(
        wnear_intents_balance,
        STAKE_AMOUNT.saturating_sub(FT_STORAGE_DEPOSIT)
    );

    env.wnear
        .near_withdraw(alice, STAKE_AMOUNT.saturating_sub(FT_STORAGE_DEPOSIT))
        .await?;

    assert_eq!(
        near_balance.total,
        alice
            .near_balance()
            .await?
            .total
            .saturating_add(FT_STORAGE_DEPOSIT)
    );

    Ok(())
}

#[tokio::test]
async fn test_unstake_by_sending_lst_from_wnear() -> TestResult {
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

    let unstake_message = unstake_message(
        env.intents.id(),
        &WithdrawTokens::Wnear {
            storage_deposit: None,
            msg: Some(alice.id().to_string()),
            memo: None,
            min_gas: None,
        },
    );
    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &unstake_message)
        .await?;

    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);

    env.wait_unstake_cooldown().await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked.as_near(), INIT_LOCK.as_near());

    env.lst.withdraw(alice, &unstake_message).await?;

    let wnear_intents_balance = env.wnear.ft_balance_of(env.intents.id()).await?;
    assert_eq!(wnear_intents_balance, STAKE_AMOUNT);

    assert_eq!(
        alice.near_balance().await?.total.as_millinear() + 1,
        INIT_BALANCE.saturating_sub(STAKE_AMOUNT).as_millinear()
    );
    let intents_balance = env
        .intents
        .mt_balance_of(alice.id(), env.wnear.id())
        .await?;
    assert_eq!(intents_balance, STAKE_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_two_unstakes_by_sending_lst_from_wnear() -> TestResult {
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

    let unstake_message = unstake_message(
        env.intents.id(),
        &WithdrawTokens::Wnear {
            storage_deposit: None,
            msg: Some(alice.id().to_string()),
            memo: None,
            min_gas: None,
        },
    );
    env.lst
        .ft_transfer_call(alice, env.lst.id(), half_stake_amount, &unstake_message)
        .await?;
    env.lst
        .ft_transfer_call(alice, env.lst.id(), half_stake_amount, &unstake_message)
        .await?;

    env.lst.ping().await?;

    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(
        env.lst.get_total_balance().await?,
        INIT_BALANCE
            .saturating_add(STAKE_AMOUNT)
            .saturating_add(ONE_YOCTO)
    );
    assert_eq!(
        env.lst.get_total_staked_balance().await?,
        INIT_LOCK.saturating_add(ONE_YOCTO)
    );

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(alice, &unstake_message).await?;

    assert_eq!(
        env.wnear.ft_balance_of(env.intents.id()).await?,
        STAKE_AMOUNT
    );
    assert_eq!(
        env.lst.get_total_balance().await?,
        INIT_BALANCE.saturating_add(ONE_YOCTO)
    );

    assert_eq!(
        alice.near_balance().await?.total.as_millinear() + 1,
        INIT_BALANCE.saturating_sub(STAKE_AMOUNT).as_millinear()
    );
    let intents_balance = env
        .intents
        .mt_balance_of(alice.id(), env.wnear.id())
        .await?;
    assert_eq!(intents_balance, STAKE_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_reunstake_with_same_wnear_message_after_partial_refund() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let ft_receiver = env.deploy_ft_receiver().await?;
    let half_stake_amount = STAKE_AMOUNT.saturating_div(2);
    let first_refund_amount = half_stake_amount.saturating_div(2);
    let first_consumed_amount = half_stake_amount.saturating_sub(first_refund_amount);

    env.wnear
        .ft_storage_deposit(&env.wnear.as_account(), ft_receiver.id())
        .await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&String>),
        )
        .await?;

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);

    let unstake_message = unstake_message(
        ft_receiver.id(),
        &WithdrawTokens::Wnear {
            storage_deposit: None,
            msg: Some(refund_once_message(first_refund_amount)),
            memo: None,
            min_gas: None,
        },
    );

    env.lst
        .ft_transfer_call(alice, env.lst.id(), half_stake_amount, &unstake_message)
        .await?;

    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(half_stake_amount)
    );
    assert_eq!(
        env.lst.get_total_pending_withdrawals().await?,
        half_stake_amount
    );

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(alice, &unstake_message).await?;

    assert_eq!(
        env.wnear.ft_balance_of(ft_receiver.id()).await?,
        first_consumed_amount
    );
    assert_eq!(
        env.wnear.ft_balance_of(env.lst.id()).await?,
        first_refund_amount
    );
    assert_eq!(
        env.lst.get_total_pending_withdrawals().await?,
        first_refund_amount
    );

    env.lst
        .ft_transfer_call(alice, env.lst.id(), half_stake_amount, &unstake_message)
        .await?;

    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(
        env.lst.get_total_pending_withdrawals().await?,
        half_stake_amount.saturating_add(first_refund_amount)
    );

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(alice, &unstake_message).await?;

    assert_eq!(
        env.wnear.ft_balance_of(ft_receiver.id()).await?,
        STAKE_AMOUNT
    );
    assert_eq!(env.wnear.ft_balance_of(env.lst.id()).await?, ZERO_AMOUNT);
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, ZERO_AMOUNT);
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);

    Ok(())
}

#[tokio::test]
async fn test_withdraw_wnear_with_storage_deposit_to_wnear_unregistered_receiver() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;
    let alice = env.alice();
    let bob = env.bob();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT.saturating_add(FT_STORAGE_DEPOSIT),
            stake_message(alice.id(), Some(FT_STORAGE_DEPOSIT), None::<&String>),
        )
        .await?;

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);

    let unstake_message = unstake_message(
        bob.id(),
        &WithdrawTokens::Wnear {
            storage_deposit: Some(FT_STORAGE_DEPOSIT),
            msg: None,
            memo: None,
            min_gas: None,
        },
    );

    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &unstake_message)
        .await?;

    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, STAKE_AMOUNT);

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(alice, &unstake_message).await?;

    assert_eq!(
        env.wnear.ft_balance_of(bob.id()).await?,
        STAKE_AMOUNT.saturating_sub(FT_STORAGE_DEPOSIT)
    );
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, ZERO_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_withdraw_wnear_with_storage_deposit_exceeding_amount_fails() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let tiny_unstake_amount = FT_STORAGE_DEPOSIT.saturating_div(2);

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&String>),
        )
        .await?;

    let unstake_message = unstake_message(
        alice.id(),
        &WithdrawTokens::Wnear {
            storage_deposit: Some(FT_STORAGE_DEPOSIT),
            msg: None,
            memo: None,
            min_gas: None,
        },
    );

    env.lst
        .ft_transfer_call(alice, env.lst.id(), tiny_unstake_amount, &unstake_message)
        .await?;

    assert_eq!(
        env.lst.get_total_pending_withdrawals().await?,
        tiny_unstake_amount
    );

    env.wait_unstake_cooldown().await?;

    let result = env.lst.withdraw(alice, &unstake_message).await;
    assert_transaction_failure_contains(result, STORAGE_DEPOSIT_EXCEEDS_WITHDRAWAL_ERROR);

    assert_eq!(
        env.lst.get_total_pending_withdrawals().await?,
        tiny_unstake_amount
    );
    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_withdraw_wnear_to_current_account_with_storage_deposit_fails() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&String>),
        )
        .await?;

    let unstake_message = unstake_message(
        env.lst.id(),
        &WithdrawTokens::Wnear {
            storage_deposit: Some(FT_STORAGE_DEPOSIT),
            msg: None,
            memo: None,
            min_gas: None,
        },
    );

    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &unstake_message)
        .await?;

    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, STAKE_AMOUNT);

    env.wait_unstake_cooldown().await?;

    let result = env.lst.withdraw(alice, &unstake_message).await;
    assert_transaction_failure_contains(result, SELF_WITHDRAW_STORAGE_DEPOSIT_ERROR);

    assert_eq!(env.lst.get_total_pending_withdrawals().await?, STAKE_AMOUNT);
    assert_eq!(env.wnear.ft_balance_of(env.lst.id()).await?, ZERO_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_stake_native_near_by_itself_and_unstake_wnear_to_itself() -> TestResult {
    let env = Env::builder().build().await?;
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

    let unstake_message = unstake_message(
        env.lst.id(),
        &WithdrawTokens::Wnear {
            storage_deposit: None,
            msg: None,
            memo: None,
            min_gas: None,
        },
    );

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

    let wnear_balance = env.wnear.ft_balance_of(env.lst.id()).await?;
    assert_eq!(wnear_balance, STAKE_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_stake_native_near_by_itself_and_unstake_wnear_to_alice() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
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

    let unstake_message = unstake_message(
        alice.id(),
        &WithdrawTokens::Wnear {
            storage_deposit: None,
            msg: None,
            memo: None,
            min_gas: None,
        },
    );

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
        lst_init_balance
            .total
            .saturating_sub(STAKE_AMOUNT)
            .saturating_sub(ONE_YOCTO) // ft_transfer
    );

    let wnear_balance = env.wnear.ft_balance_of(alice.id()).await?;
    assert_eq!(wnear_balance, STAKE_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_stake_native_near_by_alice_and_unstake_wnear_to_bad_account() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_init_balance = alice.near_balance().await?;

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

    let unstake_message = unstake_message(
        env.intents.id(),
        &WithdrawTokens::Wnear {
            storage_deposit: None,
            msg: Some("bad%$#account".to_string()),
            memo: None,
            min_gas: None,
        },
    );

    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &unstake_message)
        .await?;

    assert_eq!(env.lst.ft_balance_of(env.lst.id()).await?, INIT_LOCK);
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(alice, &unstake_message).await?;

    assert_eq!(
        alice.near_balance().await?.total,
        alice_init_balance
            .total
            .saturating_sub(STAKE_AMOUNT)
            .saturating_sub(ONE_YOCTO) // ft_transfer_call
    );

    assert_eq!(env.wnear.ft_balance_of(env.lst.id()).await?, STAKE_AMOUNT);
    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);

    Ok(())
}

#[tokio::test]
#[allow(clippy::as_conversions)]
async fn test_stake_native_near_by_alice_and_try_to_bloat_storage() -> TestResult {
    const NUM_TRANSACTIONS: usize = 10;
    let env = Env::builder()
        .with_initial_balance(INIT_LOCK.saturating_add(NearToken::from_near(1)))
        .build()
        .await?;

    let alice = env.alice();
    let alice_init_balance = alice.near_balance().await?;

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

    let unstake_messages =
        vec![STAKE_AMOUNT.saturating_div(NUM_TRANSACTIONS as u128); NUM_TRANSACTIONS]
            .into_iter()
            .enumerate()
            .map(|(i, amount)| {
                (
                    amount,
                    unstake_message(
                        alice.id(),
                        &WithdrawTokens::Wnear {
                            storage_deposit: None,
                            msg: None,
                            memo: Some(format!("unstake number: {i}")),
                            min_gas: None,
                        },
                    ),
                )
            })
            .collect::<Vec<_>>();

    for (amount, unstake_message) in &unstake_messages {
        env.lst
            .ft_transfer_call(alice, env.lst.id(), *amount, unstake_message)
            .await?;
    }

    assert_eq!(env.lst.ft_balance_of(env.lst.id()).await?, INIT_LOCK);
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);

    env.wait_unstake_cooldown().await?;

    for (_, unstake_message) in &unstake_messages {
        env.lst.withdraw(alice, unstake_message).await?;
    }

    assert_eq!(
        alice.near_balance().await?.total,
        alice_init_balance
            .total
            .saturating_sub(STAKE_AMOUNT)
            .saturating_sub(ONE_YOCTO.saturating_mul(NUM_TRANSACTIONS as u128)) // ft_transfer_calls
    );

    assert_eq!(env.wnear.ft_balance_of(env.lst.id()).await?, ZERO_AMOUNT);
    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked.as_micronear(), INIT_LOCK.as_micronear());

    Ok(())
}
