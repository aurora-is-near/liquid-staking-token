#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::as_conversions
)]
use liquid_staking_token::pool::{DepositToken, WithdrawTokens};
use near_api::types::Action;
use near_api::types::transaction::actions::{FunctionCallAction, TransferAction};
use near_api::types::transaction::result::ExecutionSuccess;
use near_api::{NearGas, NearToken, Transaction};
use near_sdk::json_types::U128;
use near_sdk::serde_json::json;
use num_traits::ToPrimitive;
use ruint::aliases::U256;
use testresult::TestResult;

use crate::env::ft::FungibleToken;
use crate::env::native::Native;
use crate::env::pool::StakingPool;
use crate::env::wnear::WNear;
use crate::env::{Env, INIT_BALANCE, INIT_LOCK, TOTAL_SUPPLY};
use crate::tests::{
    ONE_YOCTO, STAKE_AMOUNT, ZERO_AMOUNT, advance_until_available, stake_message, unstake_message,
};

#[tokio::test]
async fn test_getting_rewards_for_two_epochs() -> TestResult {
    let env = env().await?;
    let alice = env.alice();

    env.lst.set_protocol_fee_bps(0).await?; // 0 %

    let balance_before = alice.near_balance().await?.total;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), false, None::<&str>),
        )
        .await?;

    let lst_balance = env.lst.ft_balance_of(alice.id()).await?;

    env.lst.ping().await?;
    env.wait_for_epochs(2).await?;

    let unstake_msg = unstake_message(alice.id(), &WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(alice, env.lst.id(), lst_balance, &unstake_msg)
        .await?;

    let exchange_rate = env.lst.get_exchange_rate().await?;

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(alice, &unstake_msg).await?;

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);

    let expected_reward = calculate_reward(lst_balance, STAKE_AMOUNT, exchange_rate);
    let actual_reward = alice
        .near_balance()
        .await?
        .total
        .saturating_sub(balance_before);

    assert_eq!(expected_reward.as_near(), actual_reward.as_near());

    Ok(())
}

#[tokio::test]
async fn test_getting_rewards_for_two_epochs_with_fee() -> TestResult {
    let env = env().await?;
    let alice = env.alice();

    env.lst.set_protocol_fee_bps(100).await?; // 1 %

    let balance_before = alice.near_balance().await?.total;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), false, None::<&str>),
        )
        .await?;

    let lst_balance = env.lst.ft_balance_of(alice.id()).await?;
    let total_balance_before = env.lst.get_total_balance().await?;
    let mut total_reward = ZERO_AMOUNT;
    let unstake_msg = unstake_message(alice.id(), &WithdrawTokens::Native);

    // Sync each epoch explicitly to exercise appreciation of previously minted
    // fee LST. Its final NEAR value is no longer 1% of all synced rewards.
    for epoch in 0..2 {
        let total_before_sync = env.lst.get_total_balance().await?;
        let staked_before_sync = env.lst.get_total_staked_balance().await?;
        let supply_before_sync = env.lst.ft_total_supply().await?;
        let treasury_before_sync = env.lst.ft_balance_of(env.lst.id()).await?;

        env.wait_for_epochs(1).await?;

        let burned_lst = if epoch == 0 {
            env.lst.ping().await?;
            ZERO_AMOUNT
        } else {
            env.lst
                .ft_transfer_call(alice, env.lst.id(), lst_balance, &unstake_msg)
                .await?;
            lst_balance
        };

        let reward = env
            .lst
            .get_total_balance()
            .await?
            .checked_sub(total_before_sync)
            .unwrap();
        assert!(reward > ZERO_AMOUNT);
        total_reward = total_reward.checked_add(reward).unwrap();

        // The 1% fee is converted to LST after crediting the other 99% to
        // backing, before minting the fee or burning Alice's LST.
        let fee_near = reward.checked_div(100).unwrap();
        let staked_after_sync = staked_before_sync.checked_add(reward).unwrap();
        let expected_fee_lst = convert_amount(
            fee_near,
            supply_before_sync,
            staked_after_sync.checked_sub(fee_near).unwrap(),
        );
        assert_eq!(
            env.lst.ft_balance_of(env.lst.id()).await?,
            treasury_before_sync.checked_add(expected_fee_lst).unwrap()
        );

        let supply_after_sync = supply_before_sync.checked_add(expected_fee_lst).unwrap();
        assert_eq!(
            env.lst.ft_total_supply().await?,
            supply_after_sync.checked_sub(burned_lst).unwrap()
        );

        let pending = env.lst.get_total_pending_withdrawals().await?;
        assert_eq!(
            pending,
            convert_amount(burned_lst, staked_after_sync, supply_after_sync)
        );
        assert_eq!(
            env.lst.get_total_staked_balance().await?,
            staked_after_sync.checked_sub(pending).unwrap()
        );
    }

    let total_pending_withdrawals = env.lst.get_total_pending_withdrawals().await?;

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(alice, &unstake_msg).await?;

    assert_eq!(env.lst.get_total_pending_withdrawals().await?, ZERO_AMOUNT);
    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);

    // Alice pays one yoctoNEAR for ft_transfer_call; gas is free in this sandbox.
    let alice_balance = alice.near_balance().await?.total;
    assert_eq!(
        alice_balance
            .checked_sub(balance_before)
            .unwrap()
            .checked_add(STAKE_AMOUNT)
            .unwrap()
            .checked_add(ONE_YOCTO)
            .unwrap(),
        total_pending_withdrawals
    );

    assert_eq!(
        env.lst.get_total_balance().await?,
        total_balance_before
            .checked_add(total_reward)
            .unwrap()
            .checked_sub(total_pending_withdrawals)
            .unwrap()
    );

    assert_eq!(
        env.lst
            .get_total_balance()
            .await?
            .saturating_sub(INIT_BALANCE),
        env.lst
            .get_total_staked_balance()
            .await?
            .saturating_sub(INIT_LOCK)
    );

    Ok(())
}

#[tokio::test]
async fn test_ping_without_rewards_preserves_pool_balances() -> TestResult {
    let env = controlled_env(100).await?;
    let before = PoolState::read(&env).await?;

    for _ in 0..2 {
        env.wait_for_next_epoch_start().await?;
        let result = env.lst.ping().await?;

        assert_eq!(result.json::<U128>()?.0, 0);
        assert_eq!(PoolState::read(&env).await?, before);
    }

    Ok(())
}

#[tokio::test]
async fn test_repeated_ping_defers_new_rewards_until_next_epoch() -> TestResult {
    let env = controlled_env(100).await?;
    let before = PoolState::read(&env).await?;
    let first_reward = NearToken::from_near(100);
    let second_reward = NearToken::from_near(50);

    env.wait_for_next_epoch_start().await?;
    // Actions in one receipt share an epoch even on a slow CI runner. Add
    // fresh funds between the pings so the epoch guard is actually exercised.
    let result = send_actions(
        &env,
        vec![
            transfer_action(first_reward),
            ping_action(),
            transfer_action(second_reward),
            ping_action(),
        ],
    )
    .await?;

    assert_eq!(result.json::<U128>()?.0, 0);
    let after_first = before.after_reward(first_reward, 100);
    assert_eq!(PoolState::read(&env).await?, after_first);

    env.wait_for_next_epoch_start().await?;
    env.lst.ping().await?;
    assert_eq!(
        PoolState::read(&env).await?,
        after_first.after_reward(second_reward, 100)
    );

    Ok(())
}

#[tokio::test]
async fn test_ping_collects_rewards_after_skipped_epochs() -> TestResult {
    let env = controlled_env(100).await?;
    let before = PoolState::read(&env).await?;
    let mut total_reward = ZERO_AMOUNT;

    for reward in [10, 20, 30].map(NearToken::from_near) {
        add_reward(&env, reward).await?;
        env.wait_for_next_epoch_start().await?;
        assert_eq!(PoolState::read(&env).await?, before);
        total_reward = total_reward.checked_add(reward).unwrap();
    }

    env.lst.ping().await?;
    assert_eq!(
        PoolState::read(&env).await?,
        before.after_reward(total_reward, 100)
    );

    Ok(())
}

#[tokio::test]
async fn test_fee_changes_apply_only_to_future_rewards() -> TestResult {
    let env = controlled_env(100).await?;
    let reward = NearToken::from_near(100);
    let mut old_fee_bps = 100;

    // Cover both the maximum fee and disabling fees completely.
    for new_fee_bps in [2_000, 0] {
        let before = PoolState::read(&env).await?;
        add_reward(&env, reward).await?;
        env.wait_for_next_epoch_start().await?;

        // The fee update itself must sync accrued rewards at the old rate.
        env.lst.set_protocol_fee_bps(new_fee_bps).await?;
        let after_change = before.after_reward(reward, old_fee_bps);
        assert_eq!(PoolState::read(&env).await?, after_change);
        assert_eq!(
            env.lst.get_reward_fee_fraction().await?,
            json!({"numerator": new_fee_bps, "denominator": 10_000})
        );

        add_reward(&env, reward).await?;
        env.wait_for_next_epoch_start().await?;
        env.lst.ping().await?;
        assert_eq!(
            PoolState::read(&env).await?,
            after_change.after_reward(reward, new_fee_bps)
        );
        old_fee_bps = new_fee_bps;
    }

    Ok(())
}

#[tokio::test]
async fn test_native_stake_syncs_rewards_before_minting_new_shares() -> TestResult {
    check_stake_syncs_rewards(DepositToken::Native).await
}

#[tokio::test]
async fn test_wnear_stake_syncs_rewards_before_minting_new_shares() -> TestResult {
    check_stake_syncs_rewards(DepositToken::Wnear).await
}

#[tokio::test]
async fn test_pending_withdrawal_does_not_receive_later_rewards() -> TestResult {
    let env = controlled_env(100).await?;
    let alice = env.alice();
    let bob = env.bob();
    for user in [alice, bob] {
        env.lst
            .stake(
                user,
                STAKE_AMOUNT,
                stake_message(user.id(), false, None::<&str>),
            )
            .await?;
    }

    let before = PoolState::read(&env).await?;
    let alice_lst = env.lst.ft_balance_of(alice.id()).await?;
    let bob_lst = env.lst.ft_balance_of(bob.id()).await?;
    let burned_lst = alice_lst.checked_div(2).unwrap();
    let reward = NearToken::from_near(100);
    add_reward(&env, reward).await?;
    env.wait_for_next_epoch_start().await?;

    let msg = unstake_message(alice.id(), &WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(alice, env.lst.id(), burned_lst, &msg)
        .await?;

    // ft_transfer_call adds one yoctoNEAR to the unsynced reward.
    let synced = before.after_reward(reward.checked_add(ONE_YOCTO).unwrap(), 100);
    let claim = synced.value_of(burned_lst);
    let after_unstake = PoolState {
        staked: synced.staked.checked_sub(claim).unwrap(),
        supply: synced.supply.checked_sub(burned_lst).unwrap(),
        pending: claim,
        ..synced
    };
    assert_eq!(PoolState::read(&env).await?, after_unstake);

    add_reward(&env, reward).await?;
    env.wait_for_next_epoch_start().await?;
    env.lst.ping().await?;
    let after_reward = after_unstake.after_reward(reward, 100);
    assert_eq!(PoolState::read(&env).await?, after_reward);
    let remaining_lst = alice_lst.checked_sub(burned_lst).unwrap();
    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, remaining_lst);
    assert_eq!(env.lst.ft_balance_of(bob.id()).await?, bob_lst);
    assert!(after_reward.value_of(remaining_lst) > after_unstake.value_of(remaining_lst));
    assert!(after_reward.value_of(bob_lst) > after_unstake.value_of(bob_lst));

    advance_until_available(&env, &msg).await?;
    let alice_balance = alice.near_balance().await?.total;
    env.lst.withdraw(alice, &msg).await?;
    assert_eq!(
        alice.near_balance().await?.total,
        alice_balance.checked_add(claim).unwrap()
    );
    assert_eq!(
        PoolState::read(&env).await?,
        PoolState {
            balance: after_reward.balance.checked_sub(claim).unwrap(),
            pending: ZERO_AMOUNT,
            ..after_reward
        }
    );

    Ok(())
}

#[tokio::test]
async fn test_small_rewards_round_protocol_fee_down() -> TestResult {
    let env = controlled_env(100).await?;

    // At a rate above 1, even a one-yoctoNEAR fee cannot mint one yoctoLST.
    for (reward_yocto, minted_fee_yocto) in [(99, 0), (100, 0), (10_001, 99)] {
        let before = PoolState::read(&env).await?;
        let reward = NearToken::from_yoctonear(reward_yocto);
        add_reward(&env, reward).await?;
        env.wait_for_next_epoch_start().await?;
        env.lst.ping().await?;

        let after = PoolState::read(&env).await?;
        assert_eq!(after, before.after_reward(reward, 100));
        assert_eq!(
            after.treasury.checked_sub(before.treasury).unwrap(),
            NearToken::from_yoctonear(minted_fee_yocto)
        );
    }

    Ok(())
}

#[test]
fn test_rewards_per_epoch() {
    let epoch_duration = 6_000_000_000_000; // 6 seconds in nanoseconds
    let reward_per_epoch = compute_reward(epoch_duration);

    assert_eq!(reward_per_epoch.as_near(), 4315);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PoolState {
    balance: NearToken,
    staked: NearToken,
    supply: NearToken,
    treasury: NearToken,
    pending: NearToken,
}

impl PoolState {
    async fn read(env: &Env) -> anyhow::Result<Self> {
        let (balance, staked, supply, treasury, pending) = tokio::try_join!(
            env.lst.get_total_balance(),
            env.lst.get_total_staked_balance(),
            env.lst.ft_total_supply(),
            env.lst.ft_balance_of(env.lst.id()),
            env.lst.get_total_pending_withdrawals(),
        )?;
        Ok(Self {
            balance,
            staked,
            supply,
            treasury,
            pending,
        })
    }

    fn after_reward(self, reward: NearToken, fee_bps: u16) -> Self {
        let fee = reward
            .checked_mul(u128::from(fee_bps))
            .unwrap()
            .checked_div(10_000)
            .unwrap();
        let staked = self.staked.checked_add(reward).unwrap();
        let fee_lst = convert_amount(fee, self.supply, staked.checked_sub(fee).unwrap());
        Self {
            balance: self.balance.checked_add(reward).unwrap(),
            staked,
            supply: self.supply.checked_add(fee_lst).unwrap(),
            treasury: self.treasury.checked_add(fee_lst).unwrap(),
            ..self
        }
    }

    fn value_of(self, lst: NearToken) -> NearToken {
        convert_amount(lst, self.staked, self.supply)
    }
}

async fn controlled_env(fee_bps: u16) -> anyhow::Result<Env> {
    // Fixed balance top-ups replace inflation so even one-yoctoNEAR rounding
    // checks are independent of block production speed and epoch duration.
    let env = Env::builder().without_inflation().build().await?;
    env.lst.set_protocol_fee_bps(fee_bps).await?;
    env.wait_for_next_epoch_start().await?;
    env.lst.ping().await?;
    Ok(env)
}

fn transfer_action(amount: NearToken) -> Action {
    Action::Transfer(TransferAction { deposit: amount })
}

fn ping_action() -> Action {
    Action::FunctionCall(Box::new(FunctionCallAction {
        method_name: "ping".to_owned(),
        args: b"{}".to_vec(),
        gas: NearGas::from_tgas(80),
        deposit: ZERO_AMOUNT,
    }))
}

async fn send_actions(env: &Env, actions: Vec<Action>) -> anyhow::Result<ExecutionSuccess> {
    let alice = env.alice();
    let result = Transaction::construct(alice.id().clone(), env.lst.id().clone())
        .add_actions(actions)
        .with_signer(alice.signer())
        .send_to(env.lst.config())
        .await?
        .into_result()?;
    assert!(
        result.receipt_failures().is_empty(),
        "{:?}",
        result.receipt_failures()
    );
    Ok(result)
}

async fn add_reward(env: &Env, reward: NearToken) -> anyhow::Result<()> {
    send_actions(env, vec![transfer_action(reward)]).await?;
    Ok(())
}

async fn check_stake_syncs_rewards(deposit_token: DepositToken) -> TestResult {
    let env = controlled_env(1_000).await?;
    let alice = env.alice();
    let bob = env.bob();
    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), false, None::<&str>),
        )
        .await?;
    if matches!(deposit_token, DepositToken::Wnear) {
        env.wnear.near_deposit(bob, STAKE_AMOUNT).await?;
    }
    let before = PoolState::read(&env).await?;
    let alice_lst = env.lst.ft_balance_of(alice.id()).await?;
    let reward = NearToken::from_near(100);
    add_reward(&env, reward).await?;
    env.wait_for_next_epoch_start().await?;

    let message = stake_message(bob.id(), false, None::<&str>);
    match deposit_token {
        DepositToken::Native => {
            env.lst.stake(bob, STAKE_AMOUNT, message).await?;
        }
        DepositToken::Wnear => {
            // near_withdraw returns the pool's attached yoctoNEAR together
            // with the unwrapped deposit, leaving the reward unchanged.
            env.wnear
                .ft_transfer_call(bob, env.lst.id(), STAKE_AMOUNT, message)
                .await?;
            assert_eq!(env.wnear.ft_balance_of(bob.id()).await?, ZERO_AMOUNT);
        }
    }

    let synced = before.after_reward(reward, 1_000);
    let bob_lst = convert_amount(STAKE_AMOUNT, synced.supply, synced.staked);
    let after = PoolState {
        balance: synced.balance.checked_add(STAKE_AMOUNT).unwrap(),
        staked: synced.staked.checked_add(STAKE_AMOUNT).unwrap(),
        supply: synced.supply.checked_add(bob_lst).unwrap(),
        ..synced
    };
    assert_eq!(PoolState::read(&env).await?, after);
    assert_eq!(env.lst.ft_balance_of(bob.id()).await?, bob_lst);
    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, alice_lst);
    assert!(bob_lst < STAKE_AMOUNT);
    assert!(after.value_of(bob_lst) <= STAKE_AMOUNT);
    assert!(after.value_of(alice_lst) > before.value_of(alice_lst));

    Ok(())
}

async fn env() -> anyhow::Result<Env> {
    Env::builder()
        .with_stake_rewards([1, 10])
        .with_epoch_length(50)
        .build()
        .await
}

fn calculate_reward(
    lst_amount: NearToken,
    stake_amount: NearToken,
    exchange_rate: f64,
) -> NearToken {
    lst_to_near(lst_amount, exchange_rate).saturating_sub(stake_amount)
}

fn lst_to_near(lst_amount: NearToken, exchange_rate: f64) -> NearToken {
    NearToken::from_yoctonear(
        (f64::from(U256::from(lst_amount.as_yoctonear())) * exchange_rate).round() as u128,
    )
}

fn convert_amount(amount: NearToken, numerator: NearToken, denominator: NearToken) -> NearToken {
    NearToken::from_yoctonear(
        (U256::from(amount.as_yoctonear()) * U256::from(numerator.as_yoctonear())
            / U256::from(denominator.as_yoctonear()))
        .to_u128()
        .unwrap(),
    )
}

fn compute_reward(epoch_duration: u128) -> NearToken {
    let num_seconds_per_year = 60 * 60 * 24 * 365;
    let num_ns_in_second = 1_000_000_000;
    let per_epoch_total_reward = NearToken::from_yoctonear(
        (U256::from(1) * U256::from(TOTAL_SUPPLY.as_yoctonear()) * U256::from(epoch_duration)
            / (U256::from(num_seconds_per_year) * U256::from(40) * U256::from(num_ns_in_second)))
        .to_u128()
        .unwrap(),
    );
    let per_epoch_protocol_treasury = per_epoch_total_reward
        .checked_mul(1)
        .unwrap()
        .checked_div(10)
        .unwrap();

    per_epoch_total_reward
        .checked_sub(per_epoch_protocol_treasury)
        .unwrap()
}
