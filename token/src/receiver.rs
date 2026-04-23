use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;
use near_sdk::json_types::U128;
use near_sdk::{AccountId, NearToken, PromiseOrValue, env, near, serde_json};

use crate::{LiquidStakingToken, LiquidStakingTokenExt};

#[near]
impl FungibleTokenReceiver for LiquidStakingToken {
    fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        let _ = sender_id;
        let token_id = env::predecessor_account_id();

        if token_id == self.wnear_id {
            let stake_message = serde_json::from_str(&msg)
                .unwrap_or_else(|_| env::panic_str("Invalid format of the StakeMessage"));

            self.handle_staking(amount, stake_message)
        } else if token_id == env::current_account_id() {
            let unstake_message = serde_json::from_str(&msg)
                .unwrap_or_else(|_| env::panic_str("Invalid format of the UnstakeMessage"));
            let unstake_amount = NearToken::from_yoctonear(amount.0);

            self.handle_unstaking(unstake_amount, &unstake_message)
                .into()
        } else {
            env::panic_str("Invalid token account ID");
        }
    }
}
