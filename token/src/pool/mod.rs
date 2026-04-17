use near_sdk::{AccountId, Gas, NearToken, PublicKey, near};

use crate::{LiquidStakingToken, LiquidStakingTokenExt};

pub use stake::StakeMessage;
pub use unstake::{UnstakeMessage, WithdrawTokens};

mod stake;
mod unstake;
mod withdraw;

const FT_TRANSFER_GAS: Gas = Gas::from_tgas(2);
const FT_TRANSFER_CALL_GAS_MIN: Gas = Gas::from_tgas(30);
const MODIFY_STAKED_AMOUNT_GAS: Gas = Gas::from_tgas(1);
const STORAGE_DEPOSIT_GAS: Gas = Gas::from_tgas(2);

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
        self.owner_id.clone()
    }

    #[private]
    #[allow(clippy::missing_const_for_fn)]
    pub fn modify_total_staked_amount(
        &mut self,
        account_id: &AccountId,
        total_staked_amount: NearToken,
        staked_tokens: NearToken,
        is_stake: bool,
    ) {
        self.total_staked_amount = total_staked_amount;

        if is_stake {
            self.token
                .internal_deposit(account_id, staked_tokens.as_yoctonear());
        } else {
            self.token
                .internal_withdraw(account_id, staked_tokens.as_yoctonear());
        }
    }
}

#[inline]
fn calculate_min_gas(min_gas: Option<Gas>, is_call: bool) -> Gas {
    let min = if is_call {
        FT_TRANSFER_CALL_GAS_MIN
    } else {
        FT_TRANSFER_GAS
    };

    min_gas.unwrap_or(min).max(min)
}
