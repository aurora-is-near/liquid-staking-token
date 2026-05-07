use near_contract_standards::fungible_token::metadata::{
    FungibleTokenMetadata, FungibleTokenMetadataProvider,
};
use near_sdk::near;

use crate::{LiquidStakingToken, LiquidStakingTokenExt};

#[near]
impl FungibleTokenMetadataProvider for LiquidStakingToken {
    fn ft_metadata(&self) -> FungibleTokenMetadata {
        self.metadata.clone()
    }
}
