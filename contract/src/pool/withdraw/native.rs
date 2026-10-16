use near_sdk::{AccountId, CryptoHash, NearToken, Promise, env, near, require};

use crate::pool::withdraw::ON_WITHDRAW_NATIVE_GAS;
use crate::{LiquidStakingToken, LiquidStakingTokenExt};

#[near]
impl LiquidStakingToken {
    #[private]
    pub fn on_withdraw_native(&mut self, hash: CryptoHash, amount: NearToken) {
        if env::promise_result_checked(0, 0).is_ok() {
            require!(
                self.withdrawal_requests
                    .locked_tranche(&hash)
                    .withdrawal_amount
                    == amount,
                "Withdrawal amount does not match the locked claim"
            );
            self.withdrawal_requests.remove_request(&hash);
            self.statistics.decrease_total_balance(amount);
            self.statistics.decrease_pending_withdrawals(amount);
        } else {
            near_sdk::log!("Error while withdrawing Native NEAR");
        }
        // Settlement and unlocking must commit in the same receipt. A later
        // cleanup receipt could otherwise unlock a new withdrawal for this hash.
        self.withdrawal_requests.release_lock(&hash);
    }
}

impl LiquidStakingToken {
    pub(super) fn withdraw_native(
        receiver_id: AccountId,
        amount: NearToken,
        hash: CryptoHash,
    ) -> Promise {
        near_sdk::log!(
            "Withdraw to {receiver_id} amount: {} yoctoNEAR",
            amount.as_yoctonear(),
        );

        Promise::new(receiver_id).transfer(amount).then(
            Self::ext(env::current_account_id())
                .with_static_gas(ON_WITHDRAW_NATIVE_GAS)
                .on_withdraw_native(hash, amount),
        )
    }
}
