use near_sdk::{AccountId, PromiseOrValue, env, json_types::U128, near};

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
        let (refund_amount, is_refund_once) = parse_refund_msg(&msg);
        let refund_amount = if is_refund_once && self.refund_once_used {
            0
        } else {
            if is_refund_once {
                self.refund_once_used = true;
            }

            refund_amount
        };

        PromiseOrValue::Value(U128(amount.0.min(refund_amount)))
    }
}

fn parse_refund_msg(msg: &str) -> (u128, bool) {
    near_sdk::serde_json::from_str::<RefundMessage>(msg)
        .or_else(|_| {
            near_sdk::serde_json::from_str::<U128>(msg).map(|refund_amount| RefundMessage {
                refund_once: false,
                refund_amount,
            })
        })
        .or_else(|_| {
            msg.parse::<u128>().map(|refund_amount| RefundMessage {
                refund_once: false,
                refund_amount: U128(refund_amount),
            })
        })
        .map_or_else(
            |_| {
                env::panic_str(&format!(
                    "ft_on_transfer: invalid refund message received: {msg}"
                ))
            },
            |refund_message| (refund_message.refund_amount.0, refund_message.refund_once),
        )
}

#[derive(Default)]
#[near(serializers = [json])]
struct RefundMessage {
    refund_once: bool,
    refund_amount: U128,
}

#[test]
fn test_parse_refund_msg() {
    let msg = r#"{"refund_once": true, "refund_amount": "1000000000"}"#;
    let (amount, is_refund_once) = parse_refund_msg(msg);
    assert_eq!((amount, is_refund_once), (1_000_000_000, true));

    let msg = "\"1000000000\"";
    let (amount, is_refund_once) = parse_refund_msg(msg);
    assert_eq!((amount, is_refund_once), (1_000_000_000, false));

    let msg = "1000000000";
    let (amount, is_refund_once) = parse_refund_msg(msg);
    assert_eq!((amount, is_refund_once), (1_000_000_000, false));
}
