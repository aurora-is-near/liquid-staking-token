use near_api::NearToken;

use crate::tests::STAKE_AMOUNT;

mod intents;
mod native;
mod wnear;

const HALF_OF_STAKE: NearToken = STAKE_AMOUNT.saturating_div(2);
