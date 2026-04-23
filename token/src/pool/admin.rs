use near_plugins::{AccessControllable, access_control_any};
use near_sdk::near;
use near_sdk::{AccountId, NearToken, Promise, env, require};

use crate::{LiquidStakingToken, LiquidStakingTokenExt, Role};

const MAX_WITHDRAWAL_FEE_BPS: u128 = 2_000; // 20%

#[near]
impl LiquidStakingToken {
    /// Updates the withdrawal fee. Capped at [`MAX_WITHDRAWAL_FEE_BPS`].
    #[access_control_any(roles(Role::Admin))]
    pub fn set_withdrawal_fee_bps(&mut self, fee_bps: u16) {
        require!(
            u128::from(fee_bps) <= MAX_WITHDRAWAL_FEE_BPS,
            "withdrawal_fee_bps exceeds MAX_WITHDRAWAL_FEE_BPS"
        );
        self.statistics.withdrawal_fee_bps = fee_bps;
    }

    /// Transfers accumulated withdrawal fees to `receiver_id` as native NEAR.
    /// If `amount` is `None`, the entire collected balance is claimed.
    #[access_control_any(roles(Role::Admin))]
    pub fn claim_withdrawal_fees(
        &mut self,
        receiver_id: AccountId,
        amount: Option<NearToken>,
    ) -> Promise {
        let amount = amount.unwrap_or(self.statistics.withdrawal_collected_fees);
        require!(!amount.is_zero(), "Nothing to claim");
        require!(
            amount <= self.statistics.withdrawal_collected_fees,
            "Requested amount exceeds collected fees"
        );

        self.statistics.withdrawal_collected_fees = self
            .statistics
            .withdrawal_collected_fees
            .checked_sub(amount)
            .unwrap_or_else(|| env::panic_str("Underflow while claiming withdrawal fees"));

        Promise::new(receiver_id).transfer(amount)
    }
}
