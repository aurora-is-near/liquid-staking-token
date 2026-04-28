use near_sdk::{AccountId, PromiseOrValue, env, json_types::U128, near};

#[derive(Default)]
#[near(contract_state)]
pub struct Contract;

#[near]
impl Contract {
    #[allow(clippy::use_self)]
    #[must_use]
    #[init]
    pub const fn new() -> Self {
        Self
    }

    pub fn mt_on_transfer(
        &mut self,
        sender_id: AccountId,
        previous_owner_ids: Vec<AccountId>,
        token_ids: Vec<String>,
        amounts: Vec<U128>,
        msg: String,
    ) -> PromiseOrValue<Vec<U128>> {
        let _ = (sender_id, previous_owner_ids, token_ids);
        let refund_amount = msg.parse::<u128>().unwrap_or_else(|_| {
            env::panic_str("mt_on_transfer: msg must be a valid u128 refund amount")
        });

        PromiseOrValue::Value(
            amounts
                .into_iter()
                .map(|amount| U128(amount.0.min(refund_amount)))
                .collect(),
        )
    }
}
