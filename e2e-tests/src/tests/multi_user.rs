use liquid_staking_token::pool::WithdrawTokens;
use near_api::NearToken;
use near_sdk::serde_json::{Value, json};
use testresult::TestResult;

use crate::env::ft::FungibleToken;
use crate::env::native::Native;
use crate::env::pool::StakingPool;
use crate::env::types::Account;
use crate::env::{Env, INIT_BALANCE, INIT_LOCK};
use crate::tests::asserts::assert_same_block_success;
use crate::tests::{ONE_YOCTO, STAKE_AMOUNT, ZERO_AMOUNT, stake_message, unstake_message};

async fn stake_in_same_block(
    env: &Env,
    alice: &Account,
    bob: &Account,
    bob_initial_stake: NearToken,
) -> TestResult {
    let alice_stake_tx = env
        .lst
        .inner
        .call_function(
            "stake",
            json!({
                "args": stake_message(alice.id(), None, None::<&str>),
            }),
        )
        .transaction()
        .deposit(STAKE_AMOUNT)
        .max_gas()
        .with_signer(alice.id().clone(), alice.signer());
    let bob_stake_tx = env
        .lst
        .inner
        .call_function(
            "stake",
            json!({
                "args": stake_message(bob.id(), None, None::<&str>),
            }),
        )
        .transaction()
        .deposit(bob_initial_stake)
        .max_gas()
        .with_signer(bob.id().clone(), bob.signer());

    let (alice_stake_result, bob_stake_result) = tokio::try_join!(
        alice_stake_tx.send_to(env.lst.config()),
        bob_stake_tx.send_to(env.lst.config()),
    )?;

    assert_same_block_success(
        alice_stake_result,
        bob_stake_result,
        "Alice stake transaction is pending",
        "Bob stake transaction is pending",
    )
}

async fn unstake_and_stake_in_same_block(
    env: &Env,
    alice: &Account,
    bob: &Account,
    alice_unstake_message: &Value,
    bob_second_stake: NearToken,
) -> TestResult {
    let alice_unstake_tx = env
        .lst
        .inner
        .call_function(
            "ft_transfer_call",
            json!({
                "receiver_id": env.lst.id(),
                "amount": STAKE_AMOUNT,
                "msg": alice_unstake_message.to_string(),
            }),
        )
        .transaction()
        .deposit(ONE_YOCTO)
        .max_gas()
        .with_signer(alice.id().clone(), alice.signer());
    let bob_stake_tx = env
        .lst
        .inner
        .call_function(
            "stake",
            json!({
                "args": stake_message(bob.id(), None, None::<&str>),
            }),
        )
        .transaction()
        .deposit(bob_second_stake)
        .max_gas()
        .with_signer(bob.id().clone(), bob.signer());

    let (alice_unstake_result, bob_stake_result) = tokio::try_join!(
        alice_unstake_tx.send_to(env.lst.config()),
        bob_stake_tx.send_to(env.lst.config()),
    )?;

    assert_same_block_success(
        alice_unstake_result,
        bob_stake_result,
        "Alice unstake transaction is pending",
        "Bob second stake transaction is pending",
    )
}

/// Alice and Bob stake independently, unstake with different messages (different
/// receiver_ids produce different queue keys), and each withdraws their own funds
/// without interfering with the other.
#[tokio::test]
async fn test_two_users_stake_and_unstake_independently() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();

    let bob_stake = STAKE_AMOUNT.saturating_div(2);

    // Both users stake.
    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;
    env.lst
        .stake(bob, bob_stake, stake_message(bob.id(), None, None::<&str>))
        .await?;

    // Locked balance grows by the sum of both stakes.
    assert_eq!(
        env.lst.near_balance().await?.locked,
        INIT_LOCK
            .saturating_add(STAKE_AMOUNT)
            .saturating_add(bob_stake)
    );
    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);
    assert_eq!(env.lst.ft_balance_of(bob.id()).await?, bob_stake);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK
            .saturating_add(STAKE_AMOUNT)
            .saturating_add(bob_stake)
    );

    // Each user unstakes using a message keyed to their own receiver_id, so the
    // two entries in the unstake queue are distinct.
    let alice_unstake_msg = unstake_message(alice.id(), &WithdrawTokens::Native);
    let bob_unstake_msg = unstake_message(bob.id(), &WithdrawTokens::Native);

    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &alice_unstake_msg)
        .await?;
    env.lst
        .ft_transfer_call(bob, env.lst.id(), bob_stake, &bob_unstake_msg)
        .await?;

    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);

    env.wait_unstake_cooldown().await?;

    assert_eq!(
        env.lst.near_balance().await?.locked,
        INIT_LOCK.saturating_add(ONE_YOCTO)
    );

    // Each user withdraws their own entry independently.
    env.lst.withdraw(alice, &alice_unstake_msg).await?;
    env.lst.withdraw(bob, &bob_unstake_msg).await?;

    assert_eq!(
        alice
            .near_balance()
            .await?
            .total
            .saturating_add(NearToken::from_yoctonear(2)),
        INIT_BALANCE
    );
    assert_eq!(
        bob.near_balance()
            .await?
            .total
            .saturating_add(NearToken::from_yoctonear(2)),
        INIT_BALANCE
    );

    Ok(())
}

/// Alice and Bob stake in the same block, then Alice unstakes while Bob stakes
/// again in the same block. The pool must account both concurrent deltas.
#[tokio::test]
async fn test_concurrent_stakes_then_same_block_unstake_and_stake() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();

    let bob_initial_stake = STAKE_AMOUNT.saturating_div(2);
    let bob_second_stake = STAKE_AMOUNT.saturating_div(4);
    let expected_initial_stake = STAKE_AMOUNT.saturating_add(bob_initial_stake);

    stake_in_same_block(&env, alice, bob, bob_initial_stake).await?;

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);
    assert_eq!(env.lst.ft_balance_of(bob.id()).await?, bob_initial_stake);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(expected_initial_stake)
    );
    assert_eq!(
        env.lst.get_total_staked_balance().await?,
        INIT_LOCK.saturating_add(expected_initial_stake)
    );

    let alice_unstake_message = unstake_message(alice.id(), &WithdrawTokens::Native);
    unstake_and_stake_in_same_block(&env, alice, bob, &alice_unstake_message, bob_second_stake)
        .await?;

    let expected_bob_balance_without_reward = bob_initial_stake.saturating_add(bob_second_stake);
    // `ft_transfer_call` attaches 1 yoctoNEAR to the contract; the concurrent
    // stake observes it during reward sync before restaking the corrected total.
    let expected_total_staked_after_same_block = INIT_LOCK
        .saturating_add(expected_bob_balance_without_reward)
        .saturating_add(ONE_YOCTO);

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
    let actual_bob_balance = env.lst.ft_balance_of(bob.id()).await?;
    assert!(
        actual_bob_balance == expected_bob_balance_without_reward
            || actual_bob_balance == expected_bob_balance_without_reward.saturating_sub(ONE_YOCTO)
    );
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(actual_bob_balance)
    );
    assert_eq!(
        env.lst.get_total_staked_balance().await?,
        expected_total_staked_after_same_block
    );
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, STAKE_AMOUNT);

    env.wait_unstake_cooldown().await?;
    env.lst.withdraw(alice, &alice_unstake_message).await?;

    assert_eq!(env.lst.get_total_pending_withdrawals().await?, ZERO_AMOUNT);

    Ok(())
}
