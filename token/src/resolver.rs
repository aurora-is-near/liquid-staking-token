use near_contract_standards::fungible_token::FungibleTokenResolver;
use near_sdk::json_types::U128;
use near_sdk::{AccountId, near};

use crate::{LiquidStakingToken, LiquidStakingTokenExt};

#[near]
impl FungibleTokenResolver for LiquidStakingToken {
    #[private]
    fn ft_resolve_transfer(
        &mut self,
        sender_id: AccountId,
        receiver_id: AccountId,
        amount: U128,
    ) -> U128 {
        let (used_amount, burned_amount) =
            self.token
                .internal_ft_resolve_transfer(&sender_id, receiver_id, amount);
        if burned_amount > 0 {
            near_sdk::log!("Account @{} burned {}", sender_id, burned_amount);
        }

        used_amount.into()
    }
}
