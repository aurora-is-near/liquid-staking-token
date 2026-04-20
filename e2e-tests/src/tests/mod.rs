use liquid_staking_token::pool::WithdrawTokens;
use near_api::NearToken;
use near_sdk::serde::Serialize;
use near_sdk::serde_json;

mod multi_user;
mod stake;
mod unstake;

const ZERO_AMOUNT: NearToken = NearToken::ZERO;
const ONE_YOCTO: NearToken = NearToken::from_yoctonear(1);
const STAKE_AMOUNT: NearToken = NearToken::from_near(1_000);

fn stake_message<T: AsRef<str>>(
    receiver_id: impl AsRef<str>,
    storage_deposit: Option<NearToken>,
    msg: Option<T>,
) -> serde_json::Value {
    serde_json::json!({
        "receiver_id": receiver_id.as_ref(),
        "storage_deposit": storage_deposit,
        "msg": msg.as_ref().map(AsRef::as_ref),
        "min_gas": null,
    })
}

fn stake_message_with_refund<T: AsRef<str>>(
    receiver_id: impl AsRef<str>,
    storage_deposit: Option<NearToken>,
    msg: Option<T>,
    refund_message: Option<impl Serialize>,
) -> serde_json::Value {
    serde_json::json!({
        "receiver_id": receiver_id.as_ref(),
        "storage_deposit": storage_deposit,
        "msg": msg.as_ref().map(AsRef::as_ref),
        "min_gas": null,
        "refund_message": refund_message,
    })
}

fn unstake_message(
    receiver_id: impl AsRef<str>,
    withdraw_tokens: WithdrawTokens,
) -> serde_json::Value {
    serde_json::json!({
        "receiver_id": receiver_id.as_ref(),
        "withdraw_tokens": withdraw_tokens,
    })
}
