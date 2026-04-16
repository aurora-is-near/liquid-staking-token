use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;
use near_sdk::json_types::U128;
use near_sdk::{AccountId, PromiseOrValue, env, near};

use crate::{LiquidStakingToken, LiquidStakingTokenExt};

#[near]
impl FungibleTokenReceiver for LiquidStakingToken {
    fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        let token_id = env::predecessor_account_id();

        if token_id == self.wnear_id {
            self.handle_staking(sender_id, amount, msg)
        } else if token_id == env::current_account_id() {
            self.handle_unstaking(sender_id, amount, msg)
        } else {
            env::panic_str("Invalid token account ID");
        }
    }
}
