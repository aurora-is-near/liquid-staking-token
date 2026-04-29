use near_sdk::{AccountId, CryptoHash, NearToken, Promise, env, near};

use crate::pool::withdraw::ON_WITHDRAW_NATIVE_GAS;
use crate::{LiquidStakingToken, LiquidStakingTokenExt};

#[near]
impl LiquidStakingToken {
    #[private]
    pub fn on_withdraw_native(&mut self, msg_hash: CryptoHash, amount: NearToken) {
        if env::promise_result_checked(0, 0).is_ok() {
            near_sdk::log!("Native NEAR withdrawn successfully");
            self.unstake_queue.remove(&msg_hash);
            self.statistics.decrease_total_balance(amount);
            self.statistics.decrease_pending_withdrawals(amount);
        } else {
            near_sdk::log!("Error while withdrawing Native NEAR");
            self.remove_lock(msg_hash);
        }
    }
}

impl LiquidStakingToken {
    pub(super) fn withdraw_native(
        receiver_id: AccountId,
        amount: NearToken,
        msg_hash: CryptoHash,
    ) -> Promise {
        near_sdk::log!(
            "Withdraw to {receiver_id} amount: {} yoctoNEAR",
            amount.as_yoctonear(),
        );

        Promise::new(receiver_id).transfer(amount).then(
            Self::ext(env::current_account_id())
                .with_unused_gas_weight(1)
                .with_static_gas(ON_WITHDRAW_NATIVE_GAS)
                .on_withdraw_native(msg_hash, amount),
        )
    }
}
