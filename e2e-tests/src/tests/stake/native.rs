use liquid_staking_token::pool::WithdrawTokens;
use near_api::types::transaction::result::TransactionResultError;
use near_api::{NearToken, PublicKey};
use std::str::FromStr;
use testresult::TestResult;

use crate::env::ft::FT_STORAGE_DEPOSIT;
use crate::env::pool::StakingPool;
use crate::env::{Env, INIT_BALANCE, INIT_LOCK, ft::FungibleToken, mt::MultiToken, native::Native};
use crate::tests::stake::HALF_OF_STAKE;
use crate::tests::{
    ONE_YOCTO, STAKE_AMOUNT, ZERO_AMOUNT, stake_message, stake_message_with_refund, unstake_message,
};

const STAKE_AMOUNT_ERROR: &str = "The amount of NEAR tokens for staking must be more than 0";
const PARTIAL_REFUND_AMOUNT: NearToken = NearToken::from_near(250);

fn assert_stake_amount_error<T>(result: anyhow::Result<T>) {
    let Err(error) = result else {
        panic!("Expected stake to fail");
    };
    let tx_error = error
        .downcast_ref::<TransactionResultError>()
        .expect("Expected transaction result error");

    match tx_error {
        TransactionResultError::Failure(failure) => {
            let failure = failure.to_string();
            assert!(
                failure.contains(STAKE_AMOUNT_ERROR),
                "Expected transaction failure to contain `{STAKE_AMOUNT_ERROR}`, got `{failure}`"
            );
        }
        TransactionResultError::Pending(status) => {
            panic!("Expected transaction failure: {status:?}");
        }
    }
}

fn partial_refund_message(refund_amount: NearToken) -> String {
    refund_amount.as_yoctonear().to_string()
}

#[tokio::test]
async fn test_stake_with_native_near_and_get_on_intents() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_native_balance_before = alice.near_balance().await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(env.intents.id(), None, Some(alice.id())),
        )
        .await?;

    assert_eq!(
        env.lst.get_total_balance().await?,
        INIT_BALANCE.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(env.lst.ft_balance_of(env.intents.id()).await?, STAKE_AMOUNT);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );

    let staked_tokens = env.intents.mt_balance_of(alice.id(), env.lst.id()).await?;
    assert_eq!(staked_tokens, STAKE_AMOUNT);

    let alice_native_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_native_balance_before
            .total
            .saturating_sub(alice_native_balance_after.total),
        STAKE_AMOUNT
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_native_near_and_attempt_to_send_on_intents_with_bad_account() -> TestResult
{
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_native_balance_before = alice.near_balance().await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(env.intents.id(), None, Some("a2933a$$%$1!@!#@!@")), // Triggers a panic in `ft_on_transfer` on intents.
        )
        .await?;

    assert_eq!(
        env.lst.get_total_balance().await?,
        INIT_BALANCE.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(env.lst.ft_balance_of(env.intents.id()).await?, STAKE_AMOUNT); // Tokens stuck on the contract balance
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );

    assert_eq!(
        env.intents.mt_balance_of(alice.id(), env.lst.id()).await?,
        ZERO_AMOUNT
    ); // No tokens on intents minted

    let alice_native_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_native_balance_before.total,
        alice_native_balance_after
            .total
            .saturating_add(STAKE_AMOUNT)
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_native_near_and_to_send_on_intents_with_bad_account_with_native_refund()
-> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let lst_balance_before = env.lst.near_balance().await?;
    let refund_message = unstake_message(alice.id(), &WithdrawTokens::Native);

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message_with_refund(
                env.intents.id(),
                None,
                Some("a2933a$$%$1!@!#@!@"),
                Some(&refund_message),
            ), // Triggers a panic in `ft_on_transfer` on intents.
        )
        .await?;

    assert_eq!(
        env.lst.get_total_balance().await?,
        INIT_BALANCE.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(env.lst.ft_balance_of(env.intents.id()).await?, ZERO_AMOUNT);
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(alice, &refund_message).await?;

    assert_eq!(alice_balance_before, alice.near_balance().await?);
    assert_eq!(lst_balance_before, env.lst.near_balance().await?);

    Ok(())
}

#[tokio::test]
async fn test_stake_with_native_near_and_partial_nep141_refund_with_refund_message() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let ft_receiver = env.deploy_ft_receiver().await?;
    let alice_native_balance_before = alice.near_balance().await?;
    let consumed_amount = STAKE_AMOUNT.saturating_sub(PARTIAL_REFUND_AMOUNT);
    let refund_message = unstake_message(alice.id(), &WithdrawTokens::Native);

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message_with_refund(
                ft_receiver.id(),
                None,
                Some(partial_refund_message(PARTIAL_REFUND_AMOUNT)),
                Some(&refund_message),
            ),
        )
        .await?;

    assert_eq!(
        env.lst.get_total_balance().await?,
        INIT_BALANCE.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(
        env.lst.ft_balance_of(ft_receiver.id()).await?,
        consumed_amount
    );
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(consumed_amount)
    );
    assert_eq!(
        env.lst.get_total_pending_withdrawals().await?,
        PARTIAL_REFUND_AMOUNT
    );
    assert_eq!(
        alice_native_balance_before
            .total
            .saturating_sub(alice.near_balance().await?.total),
        STAKE_AMOUNT
    );

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(alice, &refund_message).await?;

    assert_eq!(env.lst.get_total_pending_withdrawals().await?, ZERO_AMOUNT);
    assert_eq!(
        env.lst.ft_balance_of(ft_receiver.id()).await?,
        consumed_amount
    );
    assert_eq!(
        alice_native_balance_before
            .total
            .saturating_sub(alice.near_balance().await?.total),
        consumed_amount
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_native_near_and_partial_nep141_refund_without_refund_message() -> TestResult
{
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let ft_receiver = env.deploy_ft_receiver().await?;
    let alice_native_balance_before = alice.near_balance().await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(
                ft_receiver.id(),
                None,
                Some(partial_refund_message(PARTIAL_REFUND_AMOUNT)),
            ),
        )
        .await?;

    assert_eq!(
        env.lst.get_total_balance().await?,
        INIT_BALANCE.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(env.lst.ft_balance_of(ft_receiver.id()).await?, STAKE_AMOUNT);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, ZERO_AMOUNT);
    assert_eq!(
        alice_native_balance_before
            .total
            .saturating_sub(alice.near_balance().await?.total),
        STAKE_AMOUNT
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_native_near_and_get_on_intents_bob() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();

    let alice_native_balance_before = alice.near_balance().await?;
    let bob_native_balance_before = bob.near_balance().await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(env.intents.id(), None, Some(bob.id())),
        )
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    assert_eq!(env.lst.ft_balance_of(env.intents.id()).await?, STAKE_AMOUNT);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );

    let alice_intents_balance = env.intents.mt_balance_of(alice.id(), env.lst.id()).await?;
    assert_eq!(alice_intents_balance, ZERO_AMOUNT);
    let bob_intents_balance = env.intents.mt_balance_of(bob.id(), env.lst.id()).await?;
    assert_eq!(bob_intents_balance, STAKE_AMOUNT);

    let alice_native_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_native_balance_before
            .total
            .saturating_sub(alice_native_balance_after.total),
        STAKE_AMOUNT
    );
    let bob_native_balance_after = bob.near_balance().await?;
    assert_eq!(bob_native_balance_before, bob_native_balance_after);

    Ok(())
}

#[tokio::test]
async fn test_stake_with_native_near_and_get_on_nep141() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_native_balance_before = alice.near_balance().await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    assert_eq!(env.lst.ft_balance_of(env.intents.id()).await?, ZERO_AMOUNT);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );

    let alice_lst_balance = env.lst.ft_balance_of(alice.id()).await?;
    assert_eq!(alice_lst_balance, STAKE_AMOUNT);

    let alice_native_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_native_balance_before
            .total
            .saturating_sub(alice_native_balance_after.total),
        STAKE_AMOUNT
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_native_near_and_get_on_nep141_to_bob() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();
    let alice_native_balance_before = alice.near_balance().await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(bob.id(), None, None::<&str>),
        )
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    assert_eq!(env.lst.ft_balance_of(env.intents.id()).await?, ZERO_AMOUNT);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );

    let alice_lst_balance = env.lst.ft_balance_of(alice.id()).await?;
    assert_eq!(alice_lst_balance, ZERO_AMOUNT);

    let bob_lst_balance = env.lst.ft_balance_of(bob.id()).await?;
    assert_eq!(bob_lst_balance, STAKE_AMOUNT);

    let alice_native_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_native_balance_before
            .total
            .saturating_sub(alice_native_balance_after.total),
        STAKE_AMOUNT
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_native_near_and_get_on_nep141_without_registration() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;
    let alice = env.alice();
    let alice_native_balance_before = alice.near_balance().await?;

    let result = env
        .lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await;
    assert!(result.is_err());

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK);

    assert_eq!(env.lst.ft_balance_of(env.intents.id()).await?, ZERO_AMOUNT);
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);

    let alice_lst_balance = env.lst.ft_balance_of(alice.id()).await?;
    assert_eq!(alice_lst_balance, ZERO_AMOUNT);

    let alice_native_balance_after = alice.near_balance().await?;
    assert_eq!(alice_native_balance_before, alice_native_balance_after);

    Ok(())
}

#[tokio::test]
async fn test_stake_with_native_near_and_get_on_nep141_with_registration() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;
    let alice = env.alice();
    let alice_native_balance_before = alice.near_balance().await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT.saturating_add(FT_STORAGE_DEPOSIT),
            stake_message(alice.id(), Some(FT_STORAGE_DEPOSIT), None::<&str>),
        )
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    assert_eq!(env.lst.ft_balance_of(env.intents.id()).await?, ZERO_AMOUNT);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);

    let alice_native_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_native_balance_before.total,
        alice_native_balance_after
            .total
            .saturating_add(STAKE_AMOUNT)
            .saturating_add(FT_STORAGE_DEPOSIT)
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_zero_native_near_fails() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_native_balance_before = alice.near_balance().await?;

    let result = env
        .lst
        .stake(
            alice,
            ZERO_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await;

    assert_stake_amount_error(result);

    assert_eq!(env.lst.near_balance().await?.locked, INIT_LOCK);
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
    assert_eq!(alice_native_balance_before, alice.near_balance().await?);

    Ok(())
}

#[tokio::test]
async fn test_stake_with_only_storage_deposit_fails() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;
    let alice = env.alice();
    let alice_native_balance_before = alice.near_balance().await?;

    let result = env
        .lst
        .stake(
            alice,
            FT_STORAGE_DEPOSIT,
            stake_message(alice.id(), Some(FT_STORAGE_DEPOSIT), None::<&str>),
        )
        .await;

    assert_stake_amount_error(result);

    assert_eq!(env.lst.near_balance().await?.locked, INIT_LOCK);
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
    assert_eq!(alice_native_balance_before, alice.near_balance().await?);

    Ok(())
}

#[tokio::test]
async fn test_stake_with_storage_deposit_exceeding_amount_fails() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    // Attach 1 NEAR but request a 2 NEAR storage deposit — contract must panic.
    let deposit = NearToken::from_near(1);
    let oversized_storage_deposit = NearToken::from_near(2);

    let result = env
        .lst
        .stake(
            alice,
            deposit,
            stake_message(alice.id(), Some(oversized_storage_deposit), None::<&str>),
        )
        .await;

    assert!(
        result.is_err(),
        "Expected stake to fail when storage_deposit exceeds the attached amount"
    );

    // No tokens minted, locked balance unchanged.
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(env.lst.near_balance().await?.locked, INIT_LOCK);

    Ok(())
}

#[tokio::test]
async fn test_stake_with_attempt_to_get_shared_tokens_on_contract() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();
    let refund_message = unstake_message(alice.id(), &WithdrawTokens::Native);

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message_with_refund(alice.id(), None, Some(bob.id()), Some(&refund_message)),
        )
        .await?;

    // No tokens minted, total supply unchanged.
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(
        env.lst.get_total_balance().await?,
        INIT_BALANCE.saturating_add(STAKE_AMOUNT)
    );

    assert_eq!(
        alice.near_balance().await?.total,
        INIT_BALANCE
            .saturating_sub(STAKE_AMOUNT)
            .saturating_sub(ONE_YOCTO)
    );

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(alice, &refund_message).await?;

    assert_eq!(env.lst.near_balance().await?.locked, INIT_LOCK);

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
    assert_eq!(
        alice.near_balance().await?.total,
        INIT_BALANCE.saturating_sub(ONE_YOCTO)
    );

    assert_eq!(env.lst.ft_balance_of(bob.id()).await?, ZERO_AMOUNT);
    assert_eq!(
        bob.near_balance().await?.total,
        INIT_BALANCE.saturating_sub(ONE_YOCTO)
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_native_with_storage_deposit_less_than_needed() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let lst_balance_before = env.lst.near_balance().await?;

    // Now stake should fail because of the storage_deposit is less than needed.
    let result = env
        .lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(
                alice.id(),
                Some(NearToken::from_micronear(1200)), // should more than 1250 microNEAR
                None::<&str>,
            ),
        )
        .await;
    assert!(result.is_err());

    env.lst.ping().await?;

    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(env.lst.near_balance().await?, lst_balance_before);
    assert_eq!(alice.near_balance().await?, alice_balance_before);

    Ok(())
}

#[tokio::test]
async fn test_stake_native_with_using_wrong_validator_public_key() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let lst_balance_before = env.lst.near_balance().await?;

    env.lst
        .set_validator_public_key(
            PublicKey::from_str("ed25519:5dAFYwUqY6dB5sh1grQbdu95CiiYyWeJoMVtumMoZW1").unwrap(),
        )
        .await?;

    // Now stake should fail because of the validator public key.
    let result = env
        .lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await;
    assert!(result.is_err());

    env.lst.ping().await?;

    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(env.lst.near_balance().await?, lst_balance_before);
    assert_eq!(alice.near_balance().await?, alice_balance_before);

    Ok(())
}

#[tokio::test]
async fn test_stake_native_and_sending_lst_tokens_to_contract_with_refund() -> TestResult {
    let env = Env::builder().build().await?;
    let lst_receiver = env.deploy_ft_receiver().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let refund_message = unstake_message(alice.id(), &WithdrawTokens::Native);

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message_with_refund(
                lst_receiver.id(),
                None,
                Some(HALF_OF_STAKE),
                Some(&refund_message),
            ),
        )
        .await?;

    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(HALF_OF_STAKE)
    );
    assert_eq!(
        alice
            .near_balance()
            .await?
            .total
            .saturating_add(STAKE_AMOUNT),
        alice_balance_before.total
    );

    env.wait_unstake_cooldown().await?;
    env.lst.withdraw(alice, &refund_message).await?;

    assert_eq!(
        alice
            .near_balance()
            .await?
            .total
            .saturating_add(HALF_OF_STAKE),
        alice_balance_before.total
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_native_and_sending_lst_tokens_to_lst_contract() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(env.lst.id(), None, None::<&str>),
        )
        .await?;

    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(
        env.lst.ft_balance_of(env.lst.id()).await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(
        alice.near_balance().await?.total,
        alice_balance_before.total.saturating_sub(STAKE_AMOUNT)
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_native_and_sending_lst_tokens_to_lst_contract_with_random_msg() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(env.lst.id(), None, Some("random message")),
        )
        .await?;

    // `ft_on_transfer` panics on the contract, since the message isn't recognizable.
    // No refund message, so the tokens should be left on the receiver contract (LST).

    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(
        env.lst.ft_balance_of(env.lst.id()).await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(
        alice.near_balance().await?.total,
        alice_balance_before.total.saturating_sub(STAKE_AMOUNT)
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_native_and_sending_lst_tokens_to_lst_contract_with_random_msg_and_refund_back()
-> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let refund_message = unstake_message(alice.id(), &WithdrawTokens::Native);

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message_with_refund(
                env.lst.id(),
                None,
                Some("random message"),
                Some(&refund_message),
            ),
        )
        .await?;

    // `ft_on_transfer` panics on the contract, since the message isn't recognizable.

    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(env.lst.ft_balance_of(env.lst.id()).await?, INIT_LOCK);
    assert_eq!(
        alice.near_balance().await?.total,
        alice_balance_before.total.saturating_sub(STAKE_AMOUNT)
    );

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(alice, &refund_message).await?;

    assert_eq!(alice.near_balance().await?, alice_balance_before);

    Ok(())
}

#[tokio::test]
async fn test_stake_native_and_sending_lst_tokens_to_lst_contract_with_random_msg_and_refund_to_lst()
-> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let refund_message = unstake_message(
        env.lst.id(),
        &WithdrawTokens::Wnear {
            storage_deposit: None,
            msg: None,
            memo: None,
            min_gas: None,
        },
    );
    let stake_message = stake_message_with_refund(
        env.lst.id(),
        None,
        Some("random message"),
        Some(&refund_message),
    );

    env.lst.stake(alice, STAKE_AMOUNT, stake_message).await?;

    // `ft_on_transfer` panics on the contract, since the message isn't recognizable.

    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(env.lst.ft_balance_of(env.lst.id()).await?, INIT_LOCK);
    assert_eq!(
        alice.near_balance().await?.total,
        alice_balance_before.total.saturating_sub(STAKE_AMOUNT)
    );

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(alice, &refund_message).await?;

    assert_eq!(
        alice.near_balance().await?.total,
        alice_balance_before.total.saturating_sub(STAKE_AMOUNT)
    );
    assert_eq!(env.wnear.ft_balance_of(env.lst.id()).await?, STAKE_AMOUNT);

    Ok(())
}

#[tokio::test]
#[ignore = "Requires panic in the unstake flow; cannot be triggered in e2e"]
async fn test_stake_native_and_sending_lst_tokens_to_lst_contract_with_random_msg_and_refund_to_lst_and_panic_in_unstake()
-> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let refund_message = unstake_message(
        env.lst.id(),
        &WithdrawTokens::Wnear {
            storage_deposit: None,
            msg: None,
            memo: None,
            min_gas: None,
        },
    );
    let stake_message = stake_message_with_refund(
        env.lst.id(),
        None,
        Some("random message"),
        Some(&refund_message),
    );

    env.lst.stake(alice, STAKE_AMOUNT, stake_message).await?;

    // `ft_on_transfer` panics on the contract, since the message isn't recognizable.

    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(
        env.lst.ft_balance_of(env.lst.id()).await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(
        alice.near_balance().await?.total,
        alice_balance_before.total.saturating_sub(STAKE_AMOUNT)
    );
    assert_eq!(env.wnear.ft_balance_of(env.lst.id()).await?, ZERO_AMOUNT);
    assert_eq!(
        env.lst.get_total_balance().await?,
        INIT_BALANCE.saturating_add(STAKE_AMOUNT)
    );

    Ok(())
}

#[tokio::test]
#[ignore = "Requires panic in the unstake flow; cannot be triggered in e2e"]
async fn test_stake_native_and_sending_lst_tokens_to_contract_with_refund_and_unstake_panic()
-> TestResult {
    let env = Env::builder().build().await?;
    let lst_receiver = env.deploy_ft_receiver().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let refund_message = unstake_message(alice.id(), &WithdrawTokens::Native);

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message_with_refund(
                lst_receiver.id(),
                None,
                Some(HALF_OF_STAKE),
                Some(&refund_message),
            ),
        )
        .await?;

    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(
        env.lst.ft_balance_of(lst_receiver.id()).await?,
        HALF_OF_STAKE
    );
    assert_eq!(
        env.lst.ft_balance_of(env.lst.id()).await?,
        INIT_LOCK.saturating_add(HALF_OF_STAKE) // Refunded tokens are stuck on the contract because of the panic in the unstake flow.
    );

    assert_eq!(
        alice
            .near_balance()
            .await?
            .total
            .saturating_add(STAKE_AMOUNT),
        alice_balance_before.total
    );

    env.wait_unstake_cooldown().await?;

    let result = env.lst.withdraw(alice, &refund_message).await;
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Account is not found in the unstake queue")
    );

    assert_eq!(
        alice
            .near_balance()
            .await?
            .total
            .saturating_add(STAKE_AMOUNT),
        alice_balance_before.total
    );

    Ok(())
}
