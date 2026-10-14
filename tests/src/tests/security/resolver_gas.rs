use crate::env::Env;
use crate::env::ft::{FT_STORAGE_DEPOSIT, FungibleToken};
use crate::env::native::Native;
use crate::env::pool::StakingPool;
use crate::env::types::Contract;
use crate::env::wnear::WNear;
use crate::tests::{ONE_YOCTO, stake_message, stake_message_with_refund, unstake_message};
use liquid_staking_token::pool::WithdrawTokens;
use near_api::types::Action;
use near_api::types::transaction::actions::FunctionCallAction;
use near_api::{AccountId, NearGas, NearToken, Transaction};
use near_sdk::serde_json::json;

async fn crowd_account_keys(env: &Env, token: &Contract, target: &AccountId) -> anyhow::Result<()> {
    let mut decoys = Vec::new();
    let allowed = b"0123456789abcdefghijklmnopqrstuvwxyz_-";
    for (index, byte) in target.as_bytes().iter().copied().enumerate() {
        for same_high_nibble in [true, false] {
            let Some(replacement) = allowed
                .iter()
                .copied()
                .find(|other| *other != byte && ((*other >> 4 == byte >> 4) == same_high_nibble))
            else {
                continue;
            };
            let mut key = target.as_bytes().to_vec();
            key[index] = replacement;
            if let Ok(account) = String::from_utf8(key)?.parse::<AccountId>() {
                decoys.push(account);
            }
        }
    }
    assert!(decoys.len() > 100);
    for group in decoys.chunks(16) {
        let actions = group
            .iter()
            .map(|account| {
                Ok(Action::FunctionCall(Box::new(FunctionCallAction {
                    method_name: "storage_deposit".to_owned(),
                    args: near_sdk::serde_json::to_vec(
                        &json!({"account_id": account, "registration_only": true}),
                    )?,
                    gas: NearGas::from_tgas(5),
                    deposit: FT_STORAGE_DEPOSIT,
                })))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Transaction::construct(env.alice().id().clone(), token.id().clone())
            .add_actions(actions)
            .with_signer(env.alice().signer())
            .send_to(token.config())
            .await?
            .into_result()?;
    }
    Ok(())
}

#[tokio::test]
async fn security_large_receiver_result_refunds_lst_without_refunding_spent_wnear()
-> anyhow::Result<()> {
    let env = Env::builder().with_stake_rewards([0, 1]).build().await?;
    let alice = env.alice();
    let receiver = env.deploy_ft_receiver().await?;
    let amount = NearToken::from_near(10);
    let before_supply = env.lst.ft_total_supply().await?;
    env.wnear.near_deposit(alice, amount).await?;
    let refund = unstake_message(alice.id(), &WithdrawTokens::Native);
    // Validator rewards accrue in this environment. Keep the mint and the
    // refund's unstake in one epoch, so both use the same exchange rate.
    env.wait_for_next_epoch_start().await?;
    let result = env
        .wnear
        .ft_transfer_call(
            alice,
            env.lst.id(),
            amount,
            stake_message_with_refund(receiver.id(), false, Some("large_result"), Some(&refund)),
        )
        .await?;
    assert!(
        result.receipt_failures().is_empty(),
        "{:?}",
        result.receipt_failures()
    );
    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, NearToken::ZERO);
    assert_eq!(
        env.wnear.ft_balance_of(env.lst.id()).await?,
        NearToken::ZERO
    );
    assert_eq!(env.lst.ft_balance_of(receiver.id()).await?, NearToken::ZERO);
    assert_eq!(env.lst.ft_total_supply().await?, before_supply);
    let claim = env.lst.get_total_pending_withdrawals().await?;
    assert!(claim <= amount && claim >= amount.saturating_sub(ONE_YOCTO));
    env.wait_unstake_cooldown().await?;
    let before_refund = alice.near_balance().await?.total;
    env.lst.withdraw(alice, &refund).await?;
    assert_eq!(
        env.lst.get_total_pending_withdrawals().await?,
        NearToken::ZERO
    );
    assert_eq!(
        alice.near_balance().await?.total,
        before_refund.saturating_add(claim)
    );
    Ok(())
}

#[tokio::test]
async fn security_resolvers_survive_crowded_trie_and_large_result() -> anyhow::Result<()> {
    let env = Env::builder().with_stake_rewards([0, 1]).build().await?;
    let alice = env.alice();
    let receiver = env.deploy_long_ft_receiver().await?;
    assert_eq!(receiver.id().len(), 64);
    crowd_account_keys(&env, &env.lst, receiver.id()).await?;
    crowd_account_keys(&env, &env.wnear, receiver.id()).await?;
    env.wnear
        .ft_storage_deposit(&env.wnear.as_account(), receiver.id())
        .await?;
    let amount = NearToken::from_near(10);
    env.lst
        .stake(
            alice,
            amount,
            stake_message(alice.id(), false, None::<&str>),
        )
        .await?;
    let balance = env.lst.ft_balance_of(alice.id()).await?;
    let holders_before_transfer = env.lst.get_number_of_accounts().await?;
    let result = env
        .lst
        .ft_transfer_call(alice, receiver.id(), balance, balance)
        .await?;
    assert!(
        result.receipt_failures().is_empty(),
        "{:?}",
        result.receipt_failures()
    );
    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, balance);
    assert_eq!(env.lst.ft_balance_of(receiver.id()).await?, NearToken::ZERO);
    assert_eq!(
        env.lst.get_number_of_accounts().await?,
        holders_before_transfer
    );

    let args = unstake_message(
        receiver.id(),
        &WithdrawTokens::Wnear {
            is_storage_deposit: false,
            msg: Some("large_result".to_owned()),
            memo: None,
            min_gas: None,
        },
    );
    env.lst
        .ft_transfer_call(alice, env.lst.id(), balance, &args)
        .await?;
    let claim = env.lst.get_total_pending_withdrawals().await?;
    env.wait_unstake_cooldown().await?;
    let before_withdraw_balance = env.lst.get_total_balance().await?;
    let result = env.lst.withdraw(alice, &args).await?;
    assert!(
        result.receipt_failures().is_empty(),
        "{:?}",
        result.receipt_failures()
    );
    assert_eq!(
        env.wnear.ft_balance_of(receiver.id()).await?,
        NearToken::ZERO
    );
    assert_eq!(env.wnear.ft_balance_of(env.lst.id()).await?, claim);
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, claim);
    assert!(
        env.lst
            .get_hashes_available_for_withdrawal(0, 10)
            .await?
            .contains(&args.hash()?)
    );
    assert_eq!(
        env.lst.get_total_balance().await?,
        before_withdraw_balance
            .saturating_sub(claim)
            .saturating_sub(ONE_YOCTO)
    );
    Ok(())
}
