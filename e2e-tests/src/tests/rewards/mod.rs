#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::as_conversions
)]
use liquid_staking_token::pool::WithdrawTokens;
use near_api::NearToken;
use ruint::aliases::U256;
use testresult::TestResult;

use crate::env::ft::FungibleToken;
use crate::env::native::Native;
use crate::env::pool::StakingPool;
use crate::env::{Env, TOTAL_SUPPLY};
use crate::tests::{STAKE_AMOUNT, ZERO_AMOUNT, stake_message, unstake_message};

#[tokio::test]
async fn test_getting_rewards_for_two_epochs() -> TestResult {
    let env = env().await?;
    let alice = env.alice();

    env.lst.set_withdrawal_fee_bps(0).await?; // 0 %

    let balance_before = alice.near_balance().await?.total;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
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

    env.lst.set_withdrawal_fee_bps(10).await?; // 0.1 %

    let balance_before = alice.near_balance().await?.total;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
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
    let fee = calculate_fee(lst_balance, exchange_rate, 10);
    let expected_reward_without_fee = expected_reward.saturating_sub(fee);
    let actual_reward = alice
        .near_balance()
        .await?
        .total
        .saturating_sub(balance_before);

    assert_eq!(
        expected_reward_without_fee.as_near(),
        actual_reward.as_near()
    );
    assert_eq!(
        env.lst.get_collected_fees().await?.as_micronear(),
        fee.as_micronear()
    );

    Ok(())
}

#[test]
fn test_rewards_per_epoch() {
    let epoch_duration = 6_000_000_000_000; // 6 seconds in nanoseconds
    let reward_per_epoch = compute_reward(epoch_duration);

    assert_eq!(reward_per_epoch.as_near(), 4306);
}

async fn env() -> anyhow::Result<Env> {
    Env::builder()
        .with_stake_rewards([1, 10])
        .with_epoch_length(50)
        .build()
        .await
}

fn calculate_reward(
    lst_balance: NearToken,
    stake_amount: NearToken,
    exchange_rate: f64,
) -> NearToken {
    let reward_float = f64::from(U256::from(lst_balance.as_yoctonear())) * exchange_rate;
    NearToken::from_yoctonear(reward_float.round() as u128).saturating_sub(stake_amount)
}

fn calculate_fee(lst_balance: NearToken, exchange_rate: f64, fee_bps: u128) -> NearToken {
    let reward_float = f64::from(U256::from(lst_balance.as_yoctonear())) * exchange_rate;
    NearToken::from_yoctonear((reward_float.round() as u128) * fee_bps / 10000)
}

#[allow(dead_code)]
fn compute_reward(epoch_duration: u128) -> NearToken {
    use num_traits::cast::ToPrimitive;
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
