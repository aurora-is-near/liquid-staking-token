use near_sdk::{AccountId, PromiseOrValue, env, json_types::U128, near};

const REFUND_ONCE_PREFIX: &str = "refund_once:";

#[derive(Default)]
#[near(contract_state)]
pub struct Contract {
    refund_once_used: bool,
}

#[near]
impl Contract {
    #[allow(clippy::use_self)]
    #[must_use]
    #[init]
    pub const fn new() -> Self {
        Self {
            refund_once_used: false,
        }
    }

    pub fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        let _ = sender_id;
        let refund_amount = if let Some(refund_amount) = msg.strip_prefix(REFUND_ONCE_PREFIX) {
            if self.refund_once_used {
                0
            } else {
                self.refund_once_used = true;
                parse_refund_amount(refund_amount)
            }
        } else {
            parse_refund_amount(&msg)
        };

        PromiseOrValue::Value(U128(amount.0.min(refund_amount)))
    }
}

fn parse_refund_amount(msg: &str) -> u128 {
    msg.parse::<u128>().unwrap_or_else(|_| {
        env::panic_str("ft_on_transfer: msg must be a valid u128 refund amount")
    })
}
