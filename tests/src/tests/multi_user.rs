use liquid_staking_token::pool::{Tranche, UnstakeMessage, WithdrawTokens};
use near_api::NearToken;
use testresult::TestResult;

use crate::env::ft::FungibleToken;
use crate::env::native::Native;
use crate::env::pool::StakingPool;
use crate::env::types::Account;
use crate::env::{Env, INIT_BALANCE, INIT_LOCK};
use crate::tests::{
    ONE_YOCTO, STAKE_AMOUNT, ZERO_AMOUNT, advance_to_epoch, advance_until_available, stake_message,
    unstake_message,
};

async fn stake_in_same_block(
    env: &Env,
    alice: &Account,
    bob: &Account,
    bob_initial_stake: NearToken,
) -> TestResult {
    let alice_stake_tx = env.lst.stake(
        alice,
        STAKE_AMOUNT,
        stake_message(alice.id(), false, None::<&str>),
    );
    let bob_stake_tx = env.lst.stake(
        bob,
        bob_initial_stake,
        stake_message(bob.id(), false, None::<&str>),
    );

    let (alice_stake_result, bob_stake_result) = tokio::try_join!(alice_stake_tx, bob_stake_tx,)?;

    assert_eq!(
        &alice_stake_result.outcome().block_hash,
        &bob_stake_result.outcome().block_hash
    );

    Ok(())
}

async fn unstake_and_stake_in_same_block(
    env: &Env,
    alice: &Account,
    bob: &Account,
    alice_unstake_message: &UnstakeMessage,
    bob_second_stake: NearToken,
) -> TestResult {
    let alice_unstake_tx =
        env.lst
            .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, alice_unstake_message);
    let bob_stake_tx = env.lst.stake(
        bob,
        bob_second_stake,
        stake_message(bob.id(), false, None::<&str>),
    );
    let (alice_unstake_result, bob_stake_result) =
        tokio::try_join!(alice_unstake_tx, bob_stake_tx)?;

    assert_eq!(
        alice_unstake_result.outcome().block_hash,
        bob_stake_result.outcome().block_hash
    );

    Ok(())
}

async fn partial_unstake_in_same_block(
    env: &Env,
    alice: &Account,
    bob: &Account,
    alice_unstake_message: &UnstakeMessage,
    bob_unstake_message: &UnstakeMessage,
    alice_unstake_amount: NearToken,
    bob_unstake_amount: NearToken,
) -> TestResult {
    let alice_unstake_tx = env.lst.ft_transfer_call(
        alice,
        env.lst.id(),
        alice_unstake_amount,
        alice_unstake_message,
    );
    let bob_unstake_tx =
        env.lst
            .ft_transfer_call(bob, env.lst.id(), bob_unstake_amount, bob_unstake_message);
    let (alice_unstake_result, bob_unstake_result) =
        tokio::try_join!(alice_unstake_tx, bob_unstake_tx)?;

    assert_eq!(
        alice_unstake_result.outcome().block_hash,
        bob_unstake_result.outcome().block_hash
    );

    Ok(())
}

/// Alice and Bob stake independently, unstake with different messages (different
/// `receiver_ids` produce different queue keys), and each withdraws their own funds
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
            stake_message(alice.id(), false, None::<&str>),
        )
        .await?;
    env.lst
        .stake(bob, bob_stake, stake_message(bob.id(), false, None::<&str>))
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
/// again in the same block. The pool must account for both concurrent deltas.
#[tokio::test]
async fn test_concurrent_stakes_then_same_block_unstake_and_stake() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();

    let bob_first_stake = STAKE_AMOUNT.saturating_div(2);
    let bob_second_stake = STAKE_AMOUNT.saturating_div(4);
    let expected_initial_stake = STAKE_AMOUNT.saturating_add(bob_first_stake);

    stake_in_same_block(&env, alice, bob, bob_first_stake).await?;

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);
    assert_eq!(env.lst.ft_balance_of(bob.id()).await?, bob_first_stake);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(expected_initial_stake)
    );
    assert_eq!(
        env.lst.get_total_staked_balance().await?,
        INIT_LOCK.saturating_add(expected_initial_stake)
    );
    assert_eq!(
        env.lst.near_balance().await?.locked,
        INIT_LOCK.saturating_add(expected_initial_stake)
    );

    env.wait_for_epochs(1).await?;

    let alice_unstake_message = unstake_message(alice.id(), &WithdrawTokens::Native);
    unstake_and_stake_in_same_block(&env, alice, bob, &alice_unstake_message, bob_second_stake)
        .await?;

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);

    let expected_bob_balance = bob_first_stake.saturating_add(bob_second_stake);
    let actual_bob_balance = env.lst.ft_balance_of(bob.id()).await?;
    // The result depends on the order of the receipts in the block. If Alice's receipt is first,
    // Bob's receipt will see a reward of ONE_YOCTO from Alice's `ft_transfer_call`, meaning the
    // actual balance in LST will be reduced by ONE_YOCTO because the total staked balance will
    // be more than the total supply by ONE_YOCTO.
    let bob_saw_reward = expected_bob_balance.saturating_sub(actual_bob_balance) == ONE_YOCTO;

    if bob_saw_reward {
        assert_eq!(
            actual_bob_balance,
            expected_bob_balance.saturating_sub(ONE_YOCTO)
        );
    } else {
        assert_eq!(actual_bob_balance, expected_bob_balance);
    }

    let expected_total_supply = if bob_saw_reward {
        INIT_LOCK
            .saturating_add(expected_bob_balance)
            .saturating_sub(ONE_YOCTO)
    } else {
        INIT_LOCK.saturating_add(expected_bob_balance)
    };
    assert_eq!(env.lst.ft_total_supply().await?, expected_total_supply);

    let expected_total_staked = INIT_LOCK
        .saturating_add(expected_bob_balance)
        .saturating_add(ONE_YOCTO);

    assert_eq!(
        env.lst.get_total_staked_balance().await?,
        expected_total_staked
    );
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, STAKE_AMOUNT);

    env.wait_unstake_cooldown().await?;

    // We should check the locked balance after cooldown in case of unstaking, because it requires
    // about 1 epoch to reduse locked balance after staking with a less amount than before,
    assert_eq!(env.lst.near_balance().await?.locked, expected_total_staked);

    env.lst.withdraw(alice, &alice_unstake_message).await?;

    assert_eq!(env.lst.get_total_pending_withdrawals().await?, ZERO_AMOUNT);
    assert_eq!(
        alice.near_balance().await?.total,
        INIT_BALANCE.saturating_sub(NearToken::from_yoctonear(2))
    );
    assert_eq!(
        bob.near_balance().await?.total,
        INIT_BALANCE
            .saturating_sub(bob_first_stake)
            .saturating_sub(bob_second_stake)
            .saturating_sub(ONE_YOCTO)
    );

    Ok(())
}

/// Alice and Bob stake in the same block, then both partially unstake in the
/// same block. Each keeps the remaining LST and withdraws only their own queue
/// entry.
#[tokio::test]
async fn test_concurrent_stakes_then_same_block_partial_unstakes() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();

    let bob_stake = STAKE_AMOUNT.saturating_div(2);
    let alice_unstake_amount = STAKE_AMOUNT.saturating_div(2);
    let bob_unstake_amount = bob_stake.saturating_div(2);
    let expected_alice_lst_balance = STAKE_AMOUNT.saturating_sub(alice_unstake_amount);
    let expected_bob_lst_balance = bob_stake.saturating_sub(bob_unstake_amount);
    let expected_remaining_supply =
        expected_alice_lst_balance.saturating_add(expected_bob_lst_balance);
    let expected_pending_withdrawals = alice_unstake_amount.saturating_add(bob_unstake_amount);

    stake_in_same_block(&env, alice, bob, bob_stake).await?;

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);
    assert_eq!(env.lst.ft_balance_of(bob.id()).await?, bob_stake);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK
            .saturating_add(STAKE_AMOUNT)
            .saturating_add(bob_stake)
    );
    assert_eq!(
        env.lst.near_balance().await?.locked,
        INIT_LOCK
            .saturating_add(STAKE_AMOUNT)
            .saturating_add(bob_stake)
    );

    let alice_unstake_message = unstake_message(alice.id(), &WithdrawTokens::Native);
    let bob_unstake_message = unstake_message(bob.id(), &WithdrawTokens::Native);
    partial_unstake_in_same_block(
        &env,
        alice,
        bob,
        &alice_unstake_message,
        &bob_unstake_message,
        alice_unstake_amount,
        bob_unstake_amount,
    )
    .await?;

    assert_eq!(
        env.lst.ft_balance_of(alice.id()).await?,
        expected_alice_lst_balance
    );
    assert_eq!(
        env.lst.ft_balance_of(bob.id()).await?,
        expected_bob_lst_balance
    );
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(expected_remaining_supply)
    );
    assert_eq!(
        env.lst.get_total_pending_withdrawals().await?,
        expected_pending_withdrawals
    );

    env.wait_unstake_cooldown().await?;
    env.lst.withdraw(alice, &alice_unstake_message).await?;
    env.lst.withdraw(bob, &bob_unstake_message).await?;

    assert_eq!(env.lst.get_total_pending_withdrawals().await?, ZERO_AMOUNT);
    assert_eq!(
        env.lst.ft_balance_of(alice.id()).await?,
        expected_alice_lst_balance
    );
    assert_eq!(
        env.lst.ft_balance_of(bob.id()).await?,
        expected_bob_lst_balance
    );
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(expected_remaining_supply)
    );
    assert_eq!(
        env.lst.near_balance().await?.locked,
        INIT_LOCK
            .saturating_add(expected_remaining_supply)
            .saturating_add(NearToken::from_yoctonear(2))
    );

    assert_eq!(
        alice.near_balance().await?.total,
        INIT_BALANCE
            .saturating_sub(expected_alice_lst_balance)
            .saturating_sub(NearToken::from_yoctonear(2))
    );
    assert_eq!(
        bob.near_balance().await?.total,
        INIT_BALANCE
            .saturating_sub(expected_bob_lst_balance)
            .saturating_sub(NearToken::from_yoctonear(2))
    );

    Ok(())
}

/// Alice fully unstakes while Bob keeps his position. Alice's balance and
/// pending withdrawal are cleared, but Bob's LST and total supply remain.
#[tokio::test]
async fn test_user_fully_unstakes_while_other_user_remains_staked() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();

    let bob_stake = STAKE_AMOUNT.saturating_div(2);

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), false, None::<&str>),
        )
        .await?;
    env.lst
        .stake(bob, bob_stake, stake_message(bob.id(), false, None::<&str>))
        .await?;

    let alice_unstake_message = unstake_message(alice.id(), &WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &alice_unstake_message)
        .await?;

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
    assert_eq!(env.lst.ft_balance_of(bob.id()).await?, bob_stake);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(bob_stake)
    );
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, STAKE_AMOUNT);

    env.wait_unstake_cooldown().await?;
    env.lst.withdraw(alice, &alice_unstake_message).await?;

    assert_eq!(env.lst.get_total_pending_withdrawals().await?, ZERO_AMOUNT);
    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
    assert_eq!(env.lst.ft_balance_of(bob.id()).await?, bob_stake);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(bob_stake)
    );
    assert_eq!(
        alice.near_balance().await?.total,
        INIT_BALANCE.saturating_sub(NearToken::from_yoctonear(2))
    );
    assert_eq!(
        bob.near_balance().await?.total,
        INIT_BALANCE
            .saturating_sub(bob_stake)
            .saturating_sub(ONE_YOCTO)
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_native_near_by_alice_and_bob_in_parallel_and_unstake_using_withdraw()
-> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();
    let alice_init_balance = alice.near_balance().await?;
    let bob_init_balance = bob.near_balance().await?;

    let alice_stake = env.lst.stake(
        alice,
        STAKE_AMOUNT,
        stake_message(alice.id(), false, None::<&String>),
    );
    let bob_stake = env.lst.stake(
        bob,
        STAKE_AMOUNT,
        stake_message(bob.id(), false, None::<&String>),
    );

    tokio::try_join!(alice_stake, bob_stake)?;

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

    let hashes = env.lst.get_hashes_available_for_withdrawal(0, 10).await?;
    assert!(hashes.is_empty());

    let alice_unstake_message = unstake_message(alice.id(), &WithdrawTokens::Native);
    let bob_unstake_message = unstake_message(bob.id(), &WithdrawTokens::Native);

    let alice_unstake =
        env.lst
            .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &alice_unstake_message);
    let bob_unstake =
        env.lst
            .ft_transfer_call(bob, env.lst.id(), STAKE_AMOUNT, &bob_unstake_message);

    tokio::try_join!(alice_unstake, bob_unstake)?;

    let unstake_epoch = env.epoch_height(None).await?;

    assert_eq!(env.lst.get_withdrawal_requests_count().await?, 2);
    assert_eq!(env.lst.get_withdrawal_requests(0, 10).await?.len(), 2);

    env.wait_unstake_cooldown().await?;

    let hashes = env.lst.get_hashes_available_for_withdrawal(0, 10).await?;
    let expected_hashes = [alice_unstake_message.hash()?, bob_unstake_message.hash()?];
    assert!(hashes.len() == 2 && hashes.iter().all(|hash| expected_hashes.contains(hash)));

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
        env.lst.near_balance().await?.locked,
        INIT_LOCK
            .saturating_add(ONE_YOCTO)
            .saturating_add(ONE_YOCTO)
    );
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

#[tokio::test]
async fn test_stake_native_near_by_alice_and_withdraw_when_bob_stakes() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();
    let alice_init_balance = alice.near_balance().await?;
    let bob_init_balance = bob.near_balance().await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), false, None::<&String>),
        )
        .await?;

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);
    assert_eq!(
        env.lst.get_total_staked_balance().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(
        env.lst.get_total_balance().await?,
        INIT_BALANCE.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(
        env.lst.near_balance().await?.locked,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );

    let hashes = env.lst.get_hashes_available_for_withdrawal(0, 10).await?;
    assert!(hashes.is_empty());

    let alice_unstake_message = unstake_message(alice.id(), &WithdrawTokens::Native);
    let bob_unstake_message = unstake_message(bob.id(), &WithdrawTokens::Native);

    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &alice_unstake_message)
        .await?;

    env.wait_unstake_cooldown().await?;

    // Alice withdraws while bob stakes
    let alice_withdrawal_request = env.lst.withdraw(alice, alice_unstake_message);
    let bob_stake = env.lst.stake(
        bob,
        STAKE_AMOUNT,
        stake_message(bob.id(), false, None::<&String>),
    );

    tokio::try_join!(alice_withdrawal_request, bob_stake)?;

    let bob_lst_balance = STAKE_AMOUNT.saturating_sub(ONE_YOCTO);
    assert_eq!(env.lst.ft_balance_of(bob.id()).await?, bob_lst_balance);
    assert_eq!(
        env.lst.get_total_staked_balance().await?,
        INIT_LOCK
            .saturating_add(STAKE_AMOUNT)
            .saturating_add(ONE_YOCTO)
    );
    assert_eq!(
        env.lst.get_total_balance().await?,
        INIT_BALANCE
            .saturating_add(STAKE_AMOUNT)
            .saturating_add(ONE_YOCTO)
    );
    assert_eq!(
        env.lst.near_balance().await?.locked,
        INIT_LOCK
            .saturating_add(STAKE_AMOUNT)
            .saturating_add(ONE_YOCTO)
    );

    assert_eq!(
        alice.near_balance().await?.total,
        alice_init_balance.total.saturating_sub(ONE_YOCTO)
    );

    env.lst
        .ft_transfer_call(bob, env.lst.id(), bob_lst_balance, &bob_unstake_message)
        .await?;

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(bob, bob_unstake_message).await?;

    assert_eq!(
        env.lst.near_balance().await?.locked,
        INIT_LOCK.saturating_add(ONE_YOCTO.saturating_mul(3))
    );
    assert_eq!(
        bob.near_balance().await?.total,
        bob_init_balance
            .total
            .saturating_sub(ONE_YOCTO)
            .saturating_sub(ONE_YOCTO)
    );

    Ok(())
}

#[tokio::test]
async fn test_net_stake_up_during_unbonding_window_does_not_strand_withdrawal() -> TestResult {
    let env = Env::builder()
        .with_initial_balance(INIT_LOCK.saturating_add(NearToken::from_near(1)))
        .build()
        .await?;
    let alice = env.alice();
    let bob = env.bob();

    // Net-positive: Bob will stake 3x what Alice unstakes.
    let bob_stake = STAKE_AMOUNT.saturating_mul(3);

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), false, None::<&str>),
        )
        .await?;

    let staked_epoch = env.epoch_height(None).await?;
    advance_to_epoch(&env, staked_epoch + 1).await?;
    assert_eq!(
        env.lst.near_balance().await?.locked,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );

    // Epoch E: Alice unstakes everything.
    let alice_unstake_msg = unstake_message(alice.id(), &WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &alice_unstake_msg)
        .await?;
    let unstake_epoch = env.epoch_height(None).await?;
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, STAKE_AMOUNT);

    // Epoch E+1: still inside the unbonding window — Bob stakes MORE than Alice
    // unstaked, lifting total stake above the pre-unstake level.
    advance_to_epoch(&env, unstake_epoch + 1).await?;
    env.lst
        .stake(bob, bob_stake, stake_message(bob.id(), false, None::<&str>))
        .await?;
    assert_eq!(
        env.lst.get_total_staked_balance().await?,
        INIT_LOCK.saturating_add(bob_stake)
    );

    // Withdraw at the contract's first declared maturity for Alice's tranche.
    advance_until_available(&env, &alice_unstake_msg).await?;
    env.lst.withdraw(alice, &alice_unstake_msg).await?;

    // Decisive: the interleaved stake-up must NOT strand Alice. If the funds were
    // re-locked with no offsetting liquid, the `transfer` in `on_withdraw_native`
    // would fail silently (the outer tx still "succeeds"), `pending_withdrawals`
    // would stay at STAKE_AMOUNT, and Alice would be short ~1000 N.
    assert_eq!(
        env.lst.get_total_pending_withdrawals().await?,
        ZERO_AMOUNT,
        "Claim not settled — interleaved stake-up appears to have stranded the withdrawal"
    );
    assert_eq!(
        alice.near_balance().await?.total,
        INIT_BALANCE.saturating_sub(ONE_YOCTO.saturating_mul(2)),
        "Alice not paid at declared maturity after a net-positive stake during her unbonding window"
    );

    // The pool must still be staked exactly as much as it claims — i.e., Alice's
    // payout did not cannibalize the active stake.
    assert_eq!(
        env.lst.near_balance().await?.locked,
        env.lst.get_total_staked_balance().await?,
        "Locked balance diverged from total_staked — withdrawal drew from staked funds"
    );

    Ok(())
}
