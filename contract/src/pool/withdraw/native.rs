use near_sdk::{AccountId, CryptoHash, NearToken, Promise, env, near};

use crate::pool::withdraw::{ON_WITHDRAW_NATIVE_GAS, REMOVE_LOCK_GAS};
use crate::{LiquidStakingToken, LiquidStakingTokenExt};

#[near]
impl LiquidStakingToken {
    #[private]
    pub fn on_withdraw_native(&mut self, hash: CryptoHash, amount: NearToken) {
        if env::promise_result_checked(0, 0).is_ok() {
            self.withdrawal_requests.remove_request(&hash);
        } else {
            near_sdk::log!("Error while withdrawing Native NEAR");
            self.statistics.increase_total_balance(amount);
            self.statistics.increase_pending_withdrawals(amount);
        }
    }
}

impl LiquidStakingToken {
    pub(super) fn withdraw_native(
        &mut self,
        receiver_id: AccountId,
        amount: NearToken,
        hash: CryptoHash,
    ) -> Promise {
        near_sdk::log!(
            "Withdraw to {receiver_id} amount: {} yoctoNEAR",
            amount.as_yoctonear(),
        );

        self.statistics.decrease_total_balance(amount);
        self.statistics.decrease_pending_withdrawals(amount);

        Promise::new(receiver_id)
            .transfer(amount)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(ON_WITHDRAW_NATIVE_GAS)
                    .on_withdraw_native(hash, amount),
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(REMOVE_LOCK_GAS)
                    .remove_lock(hash),
            )
    }
}
