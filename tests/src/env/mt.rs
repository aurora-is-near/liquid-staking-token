use near_api::{AccountId, Data, NearToken};
use near_sdk::json_types::U128;

use crate::env::types::Contract;

pub trait MultiToken {
    async fn mt_balance_of(
        &self,
        account_id: &AccountId,
        token_id: impl AsRef<str>,
    ) -> anyhow::Result<NearToken>;
}

impl MultiToken for Contract {
    async fn mt_balance_of(
        &self,
        account_id: &AccountId,
        token_id: impl AsRef<str>,
    ) -> anyhow::Result<NearToken> {
        self.inner
            .call_function(
                "mt_balance_of",
                near_sdk::serde_json::json!({
                    "account_id": account_id,
                    "token_id": format!("nep141:{}", token_id.as_ref()),
                }),
            )
            .read_only()
            .fetch_from(self.config())
            .await
            .map(|result: Data<U128>| NearToken::from_yoctonear(result.data.0))
            .map_err(Into::into)
    }
}
