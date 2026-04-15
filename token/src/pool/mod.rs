use near_sdk::{AccountId, Gas, NearToken, PublicKey, env, near};

use crate::{LiquidStakingToken, LiquidStakingTokenExt};

pub use stake::StakeMessage;
pub use unstake::{UnstakeMessage, WithdrawTokens};

mod stake;
mod unstake;
mod withdraw;

const MODIFY_STAKED_AMOUNT_GAS: Gas = Gas::from_tgas(1);
const FT_TRANSFER_GAS: Gas = Gas::from_tgas(2);
const FT_TRANSFER_CALL_GAS_MIN: Gas = Gas::from_tgas(35);
const FT_TRANSFER_CALL_GAS_DEFAULT: Gas = Gas::from_tgas(35);

#[near]
impl LiquidStakingToken {
    #[allow(clippy::missing_const_for_fn)]
    pub fn get_number_of_accounts(&self) -> u64 {
        1
    }

    pub fn get_reward_fee_fraction(&self) -> near_sdk::serde_json::Value {
        near_sdk::serde_json::json!({ "numerator": 1, "denominator": 10 })
    }

    pub fn get_staking_key(&self) -> PublicKey {
        self.validator_public_key.clone()
    }

    pub fn get_owner_id(&self) -> AccountId {
        env::current_account_id()
    }

    #[private]
    #[allow(clippy::missing_const_for_fn)]
    pub fn modify_total_staked_amount(&mut self, amount: NearToken) {
        self.total_staked_amount = amount;
    }
}

#[inline]
fn calculate_min_gas(min_gas: Option<Gas>, is_call: bool) -> Gas {
    let (min, default) = if is_call {
        (FT_TRANSFER_CALL_GAS_MIN, FT_TRANSFER_CALL_GAS_DEFAULT)
    } else {
        (FT_TRANSFER_GAS, FT_TRANSFER_GAS)
    };

    min_gas.unwrap_or(default).max(min)
}
