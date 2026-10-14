use crate::env::Env;
use crate::env::ft::{FtMessage, FungibleToken};
use crate::env::native::Native;
use crate::env::pool::StakingPool;
use crate::env::wnear::WNear;
use crate::tests::{ONE_YOCTO, stake_message, unstake_message};
use liquid_staking_token::pool::WithdrawTokens;
use near_api::{NearGas, NearToken};
use near_sdk::serde_json::json;

#[tokio::test]
async fn security_native_low_gas_refunds_without_minting() -> anyhow::Result<()> {
    let env = Env::builder().with_stake_rewards([0, 1]).build().await?;
    let alice = env.alice();
    let amount = NearToken::from_near(100);
    let before_balance = alice.near_balance().await?.total;
    let before_supply = env.lst.ft_total_supply().await?;
    let before_stake = env.lst.get_total_staked_balance().await?;
    for (gas, msg) in [(30, None), (58, None), (80, Some("0")), (100, Some("0"))] {
        let result = env
            .lst
            .inner
            .call_function(
                "stake",
                json!({"args": stake_message(alice.id(), false, msg)}),
            )
            .transaction()
            .deposit(amount)
            .gas(NearGas::from_tgas(gas))
            .with_signer(alice.id().clone(), alice.signer())
            .send_to(env.lst.config())
            .await?;
        assert!(result.into_result().is_err());
        assert_eq!(alice.near_balance().await?.total, before_balance);
        assert_eq!(env.lst.ft_total_supply().await?, before_supply);
        assert_eq!(env.lst.get_total_staked_balance().await?, before_stake);
    }
    Ok(())
}

#[tokio::test]
async fn security_low_gas_wnear_stake_cannot_take_another_users_residual() -> anyhow::Result<()> {
    let env = Env::builder().with_stake_rewards([0, 1]).build().await?;
    let alice = env.alice();
    let bob = env.bob();
    let receiver = env.deploy_ft_receiver().await?;
    let amount = NearToken::from_near(10);
    let residual = NearToken::from_near(5);
    env.wnear
        .ft_storage_deposit(&env.wnear.as_account(), receiver.id())
        .await?;
    env.lst
        .stake(bob, amount, stake_message(bob.id(), false, None::<&str>))
        .await?;
    let bob_lst = env.lst.ft_balance_of(bob.id()).await?;
    let args = unstake_message(
        receiver.id(),
        &WithdrawTokens::Wnear {
            is_storage_deposit: false,
            msg: Some(json!({"refund_once": true, "refund_amount": residual}).to_string()),
            memo: None,
            min_gas: None,
        },
    );
    env.lst
        .ft_transfer_call(bob, env.lst.id(), bob_lst, &args)
        .await?;
    let claim = env.lst.get_total_pending_withdrawals().await?;
    env.wait_unstake_cooldown().await?;
    env.lst.withdraw(bob, &args).await?;
    assert_eq!(env.wnear.ft_balance_of(env.lst.id()).await?, residual);
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, residual);

    env.wnear.near_deposit(alice, residual).await?;
    let before_wnear = env.wnear.ft_balance_of(alice.id()).await?;
    let before_lst = env.lst.ft_balance_of(alice.id()).await?;
    let before_supply = env.lst.ft_total_supply().await?;
    let before_pool_near = env.lst.near_balance().await?;
    for gas in [44, 56, 100] {
        let result = env
            .wnear
            .inner
            .call_function(
                "ft_transfer_call",
                json!({
                    "receiver_id": env.lst.id(),
                    "amount": residual,
                    "msg": stake_message(alice.id(), false, None::<&str>).to_msg()?,
                }),
            )
            .transaction()
            .deposit(ONE_YOCTO)
            .gas(NearGas::from_tgas(gas))
            .with_signer(alice.id().clone(), alice.signer())
            .send_to(env.wnear.config())
            .await?
            .into_result()?;
        assert_eq!(result.json::<NearToken>()?, NearToken::ZERO);
        assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, before_wnear);
        assert_eq!(env.lst.ft_balance_of(alice.id()).await?, before_lst);
        assert_eq!(env.lst.ft_total_supply().await?, before_supply);
        assert_eq!(env.wnear.ft_balance_of(env.lst.id()).await?, residual);
        assert_eq!(env.lst.get_total_pending_withdrawals().await?, residual);
        assert_eq!(env.lst.near_balance().await?.total, before_pool_near.total);
    }

    let before_retry_balance = env.lst.get_total_balance().await?;
    env.lst.withdraw(bob, &args).await?;
    assert_eq!(env.wnear.ft_balance_of(receiver.id()).await?, claim);
    assert_eq!(
        env.wnear.ft_balance_of(env.lst.id()).await?,
        NearToken::ZERO
    );
    assert_eq!(
        env.lst.get_total_pending_withdrawals().await?,
        NearToken::ZERO
    );
    assert_eq!(
        env.lst.get_total_balance().await?,
        before_retry_balance.saturating_sub(ONE_YOCTO)
    );
    Ok(())
}

#[tokio::test]
async fn security_wnear_stake_with_message_succeeds_at_recommended_gas() -> anyhow::Result<()> {
    let env = Env::builder().with_stake_rewards([0, 1]).build().await?;
    let alice = env.alice();
    let receiver = env.deploy_ft_receiver().await?;
    let amount = NearToken::from_near(10);
    env.wnear.near_deposit(alice, amount).await?;
    let before_supply = env.lst.ft_total_supply().await?;
    let result = env
        .wnear
        .inner
        .call_function(
            "ft_transfer_call",
            json!({
                "receiver_id": env.lst.id(),
                "amount": amount,
                "msg": stake_message(receiver.id(), false, Some("0")).to_msg()?,
            }),
        )
        .transaction()
        .deposit(ONE_YOCTO)
        .gas(NearGas::from_tgas(200))
        .with_signer(alice.id().clone(), alice.signer())
        .send_to(env.wnear.config())
        .await?
        .into_result()?;
    assert_eq!(result.json::<NearToken>()?, amount);
    let minted = env.lst.ft_balance_of(receiver.id()).await?;
    assert!(minted > NearToken::ZERO);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        before_supply.saturating_add(minted)
    );
    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, NearToken::ZERO);
    Ok(())
}
