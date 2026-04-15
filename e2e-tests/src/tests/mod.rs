use liquid_staking_token::pool::WithdrawTokens;
use near_api::NearToken;
use near_sdk::serde_json;

mod stake_native;
mod stake_wnear;
mod unstake_native;
mod unstake_wnear;

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
        "max_gas": null,
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
