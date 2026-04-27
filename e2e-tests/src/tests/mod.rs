use liquid_staking_token::pool::WithdrawTokens;
use near_api::NearToken;
use near_sdk::serde::Serialize;
use near_sdk::serde_json;

mod delegators;
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
    storage_deposit: Option<NearToken>,
    msg: Option<impl Serialize>,
) -> serde_json::Value {
    serde_json::json!({
        "receiver_id": receiver_id,
        "storage_deposit": storage_deposit,
        "msg": msg,
        "min_gas": null,
    })
}

fn stake_message_with_refund(
    receiver_id: impl Serialize,
    storage_deposit: Option<NearToken>,
    msg: Option<impl Serialize>,
    refund_message: Option<impl Serialize>,
) -> serde_json::Value {
    serde_json::json!({
        "receiver_id": receiver_id,
        "storage_deposit": storage_deposit,
        "msg": msg,
        "min_gas": null,
        "refund_message": refund_message,
    })
}

fn unstake_message(
    receiver_id: impl Serialize,
    withdraw_tokens: &WithdrawTokens,
) -> serde_json::Value {
    serde_json::json!({
        "receiver_id": receiver_id,
        "withdraw_tokens": withdraw_tokens,
    })
}
