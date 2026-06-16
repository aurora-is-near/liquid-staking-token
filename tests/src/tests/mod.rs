use liquid_staking_token::pool::{UnstakeMessage, WithdrawTokens};
use near_api::{AccountId, NearToken};
use near_sdk::serde::Serialize;
use near_sdk::serde_json;

use crate::env::pool::StakingPool;
use crate::env::{BLOCKS_PER_EPOCH, Env};

mod access_control;
mod assertions;
mod delegators;
mod ft;
mod multi_user;
mod pool;
mod rewards;
mod stake;
mod storage;
mod unstake;

const ZERO_AMOUNT: NearToken = NearToken::ZERO;
const ONE_YOCTO: NearToken = NearToken::from_yoctonear(1);
const STAKE_AMOUNT: NearToken = NearToken::from_near(1_000);

fn stake_message(
    receiver_id: impl Serialize,
    is_storage_deposit: bool,
    msg: Option<impl Serialize>,
) -> serde_json::Value {
    serde_json::json!({
        "receiver_id": receiver_id,
        "is_storage_deposit": is_storage_deposit,
        "msg": msg,
        "min_gas": null,
    })
}

fn stake_message_with_refund(
    receiver_id: impl Serialize,
    is_storage_deposit: bool,
    msg: Option<impl Serialize>,
    refund_message: Option<impl Serialize>,
) -> serde_json::Value {
    serde_json::json!({
        "receiver_id": receiver_id,
        "is_storage_deposit": is_storage_deposit,
        "msg": msg,
        "min_gas": null,
        "refund_message": refund_message,
    })
}

fn unstake_message(receiver_id: &AccountId, withdraw_tokens: &WithdrawTokens) -> UnstakeMessage {
    UnstakeMessage {
        receiver_id: receiver_id.clone(),
        withdraw_tokens: withdraw_tokens.clone(),
    }
}

/// Fast-forwards epoch-by-epoch until `env`'s current epoch reaches `target`.
async fn advance_to_epoch(env: &Env, target: u64) -> anyhow::Result<()> {
    while env.epoch_height(None).await? < target {
        env.fast_forward(BLOCKS_PER_EPOCH).await?;
    }
    Ok(())
}

/// Fast-forwards epoch-by-epoch until the contract itself declares the tranche
/// for `msg` available for withdrawal (its own `is_matured` view), then returns.
/// Bounded so a never-maturing tranche fails the test loudly instead of hanging.
async fn advance_until_available(env: &Env, msg: &UnstakeMessage) -> anyhow::Result<()> {
    let hash = msg.hash()?;

    for _ in 0..12 {
        if env
            .lst
            .get_hashes_available_for_withdrawal(0, 10)
            .await?
            .contains(&hash)
        {
            return Ok(());
        }
        env.fast_forward(BLOCKS_PER_EPOCH).await?;
    }

    anyhow::bail!("tranche never became available for withdrawal within 12 epochs");
}
