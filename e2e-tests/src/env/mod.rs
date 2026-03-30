use near_api::{NetworkConfig, Signer};
use near_sandbox::config::ValidatorAccount;
use near_sandbox::{GenesisAccount, Sandbox, SandboxConfig};
use near_sdk::serde::Serialize;
use near_sdk::{AccountId, NearToken, serde_json};
use std::convert::Into;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::sync::OnceCell;

use crate::env::defuse::Defuse;
use crate::env::ft::FungibleToken;
use crate::env::types::{Account, Contract};

pub mod defuse;
pub mod ft;
pub mod mt;
pub mod native;
pub mod pool;
pub mod types;
pub mod wnear;

const WNEAR: &str = "wnear.sandbox";
const INTENTS: &str = "intents.sandbox";
const LST: &str = "lst.sandbox";
const COOL_DOWN_PERIOD: u64 = 4; // in epochs
pub const INIT_LOCK: NearToken = NearToken::from_near(10_000);

pub const BLOCKS_PER_EPOCH: u64 = 50;
pub const INITIAL_BALANCE: NearToken = NearToken::from_near(1_000_000);
pub static LST_ARTIFACT: OnceCell<Vec<u8>> = OnceCell::const_new();
pub static SIGNER: LazyLock<Arc<Signer>> = LazyLock::new(|| {
    Signer::from_secret_key(
        near_sandbox::config::DEFAULT_GENESIS_ACCOUNT_PRIVATE_KEY
            .parse()
            .unwrap(),
    )
    .unwrap()
});
pub static VALIDATOR_SIGNER: LazyLock<Arc<Signer>> = LazyLock::new(|| {
    let value: serde_json::Value =
        serde_json::from_reader(std::fs::File::open("../res/validator_key.json").unwrap()).unwrap();
    let secret_key = value["secret_key"].as_str().unwrap().parse().unwrap();
    Signer::from_secret_key(secret_key).unwrap()
});

pub struct Env {
    sandbox: Sandbox,
    config: NetworkConfig,
    pub wnear: Contract,
    pub defuse: Contract,
    pub lst: Contract,
    users: (Account, Account),
}

impl Env {
    async fn new(builder: EnvBuilder) -> anyhow::Result<Self> {
        let config = sandbox_config(builder.with_stake_rewards).await;
        let sandbox = Sandbox::start_sandbox_with_config(config).await?;
        let config = NetworkConfig::from_rpc_url("sandbox", sandbox.rpc_addr.parse()?);

        let (wnear, defuse, lst) = Box::pin(create_contracts(&config)).await?;
        let users = create_users(&config)?;
        let public_key = signer().get_public_key().await?;

        tokio::try_join!(
            defuse.add_public_key(users.0.id(), public_key),
            defuse.add_public_key(users.1.id(), public_key),
        )?;

        tokio::try_join!(
            wnear.ft_storage_deposit(defuse.id()),
            wnear.ft_storage_deposit(lst.id()),
        )?;

        lst.ft_storage_deposit(defuse.id()).await?;

        if !builder.without_storage_deposit {
            tokio::try_join!(
                wnear.ft_storage_deposit(users.0.id()),
                wnear.ft_storage_deposit(users.1.id()),
                lst.ft_storage_deposit(users.0.id()),
                lst.ft_storage_deposit(users.1.id()),
            )?;
        }

        Ok(Self {
            sandbox,
            config,
            wnear,
            defuse,
            lst,
            users,
        })
    }

    pub fn builder() -> EnvBuilder {
        EnvBuilder::default()
    }

    pub fn alice(&self) -> &Account {
        &self.users.0
    }

    pub fn bob(&self) -> &Account {
        &self.users.1
    }

    pub async fn fast_forward(&self, blocks: u64) -> anyhow::Result<()> {
        self.sandbox.fast_forward(blocks).await.map_err(Into::into)
    }

    pub async fn wait_unstake_cooldown(&self) -> anyhow::Result<()> {
        let current_block = self.block_height().await?;
        let blocks_until_next_epoch = BLOCKS_PER_EPOCH - current_block % BLOCKS_PER_EPOCH;

        self.fast_forward(BLOCKS_PER_EPOCH * COOL_DOWN_PERIOD + blocks_until_next_epoch)
            .await
    }

    #[allow(dead_code)]
    pub async fn epoch_height(&self, block_height: Option<u64>) -> anyhow::Result<u64> {
        tokio_retry::Retry::spawn(retry_strategy(), || async {
            near_api::Staking::epoch_validators_info()
                .at(block_height.map_or(
                    near_api::EpochReference::Latest,
                    near_api::EpochReference::AtBlock,
                ))
                .fetch_from(&self.config)
                .await
                .map(|block| block.epoch_height)
                .map_err(Into::into)
        })
        .await
    }

    pub async fn block_height(&self) -> anyhow::Result<u64> {
        tokio_retry::Retry::spawn(retry_strategy(), || async {
            near_api::Chain::block_number()
                .fetch_from(&self.config)
                .await
                .map_err(Into::into)
        })
        .await
    }
}

#[derive(Default)]
pub struct EnvBuilder {
    without_storage_deposit: bool,
    with_stake_rewards: Option<Vec<u32>>,
}

impl EnvBuilder {
    pub fn without_storage_deposit(mut self) -> Self {
        self.without_storage_deposit = true;
        self
    }

    #[allow(dead_code)]
    pub fn with_stake_rewards(mut self, reward_ratio: Vec<u32>) -> Self {
        self.with_stake_rewards = Some(reward_ratio);
        self
    }

    pub async fn build(self) -> anyhow::Result<Env> {
        Box::pin(Env::new(self)).await
    }
}

pub fn signer() -> Arc<Signer> {
    Arc::clone(&SIGNER)
}

pub fn validator_signer() -> Arc<Signer> {
    Arc::clone(&VALIDATOR_SIGNER)
}

fn create_users(config: &NetworkConfig) -> anyhow::Result<(Account, Account)> {
    Ok((
        Account::new(
            near_api::Account("alice.near".parse()?),
            config.clone(),
            signer(),
        ),
        Account::new(
            near_api::Account("bob.near".parse()?),
            config.clone(),
            signer(),
        ),
    ))
}

async fn create_contracts(
    config: &NetworkConfig,
) -> anyhow::Result<(Contract, Contract, Contract)> {
    tokio::try_join!(
        create_wnear(config),
        create_intents(config),
        create_lst(config),
    )
}

async fn create_lst(config: &NetworkConfig) -> anyhow::Result<Contract> {
    let validator_public_key = validator_signer().get_public_key().await?.to_string();
    create_contract(
        config,
        LST,
        lst_wasm().await?,
        serde_json::json!({
            "owner_id": LST,
            "wnear_id": WNEAR,
            "intents_id": INTENTS,
            "validator_public_key": validator_public_key,
            "total_supply": NearToken::from_near(0),
            "metadata": {
                "spec": "ft-1.0.0",
                "name": "stNEAR",
                "symbol": "stNEAR",
                "decimals": 24,
            },
            "init_lock": INIT_LOCK,
        }),
    )
    .await
}

async fn create_wnear(config: &NetworkConfig) -> anyhow::Result<Contract> {
    create_contract(
        config,
        WNEAR,
        wnear_wasm().await?,
        serde_json::json!({
            "owner_id": WNEAR,
            "total_supply": NearToken::from_near(1_000_000),
            "metadata": {
                "spec": "ft-1.0.0",
                "name": "WNEAR",
                "symbol": "WNEAR",
                "decimals": 24,
            }
        }),
    )
    .await
}

async fn create_intents(config: &NetworkConfig) -> anyhow::Result<Contract> {
    create_contract(
        config,
        INTENTS,
        defuse_wasm().await?,
        serde_json::json!({
            "config": {
                "wnear_id": WNEAR,
                "fees": {
                    "fee": 0,
                    "fee_collector": INTENTS,
                },
                "roles": {
                    "super_admins": [INTENTS],
                    "admins": {},
                    "grantees": {}
                },
            }
        }),
    )
    .await
}

async fn create_contract(
    config: &NetworkConfig,
    contract_id: &str,
    wasm: Vec<u8>,
    args: impl Serialize,
) -> anyhow::Result<Contract> {
    let account_id: AccountId = contract_id.parse()?;
    let signer = if contract_id == LST {
        validator_signer()
    } else {
        signer()
    };

    near_api::Contract::deploy(account_id.clone())
        .use_code(wasm)
        .with_init_call("new", args)?
        .with_signer(signer.clone())
        .send_to(config)
        .await?
        .assert_success();

    Ok(Contract::new(
        near_api::Contract(account_id),
        config.clone(),
        signer,
    ))
}

async fn wnear_wasm() -> anyhow::Result<Vec<u8>> {
    read_wasm("../res/wnear.wasm").await
}

async fn defuse_wasm() -> anyhow::Result<Vec<u8>> {
    read_wasm("../res/defuse.wasm").await
}

async fn lst_wasm() -> anyhow::Result<Vec<u8>> {
    LST_ARTIFACT
        .get_or_try_init(async || {
            let artifact = cargo_near_build::build(
                cargo_near_build::BuildOpts::builder()
                    .manifest_path("../token/Cargo.toml")
                    .no_abi(true)
                    .no_embed_abi(true)
                    .no_doc(true)
                    .build(),
            )
            .map_err(|e| anyhow::anyhow!("Failed to build LST: {e}"))?;
            read_wasm(artifact.path).await
        })
        .await
        .cloned()
}

async fn read_wasm<P: AsRef<std::path::Path>>(path: P) -> anyhow::Result<Vec<u8>> {
    tokio::fs::read(path).await.map_err(Into::into)
}

async fn sandbox_config(reward_rate: Option<Vec<u32>>) -> SandboxConfig {
    let validator_key_file = std::fs::canonicalize("../res/validator_key.json").unwrap();
    let validator_public_key = validator_signer()
        .get_public_key()
        .await
        .unwrap()
        .to_string();
    let value: serde_json::Value =
        serde_json::from_reader(std::fs::File::open(&validator_key_file).unwrap()).unwrap();
    let validator_private_key = value["secret_key"].as_str().unwrap();

    SandboxConfig {
        additional_config: Some(serde_json::json!({
            "validator_key_file": validator_key_file,
        })),
        additional_genesis: Some(serde_json::json!({
            "epoch_length": BLOCKS_PER_EPOCH,
            "min_gas_price": "0",
            "max_gas_price": "0",
            "protocol_treasury_account": LST,
            "transaction_validity_period": BLOCKS_PER_EPOCH * 2,
            "total_supply": NearToken::from_near(1_006_020_000),
            "protocol_reward_rate": reward_rate.unwrap_or_else(|| vec![1, 1]), // vec![1, 10], // do not increase balance with rewards to simplify tests
            "max_inflation_rate":vec![1, 1], // vec![1, 40],
        })),
        validators: Some(vec![ValidatorAccount {
            account_id: LST.parse().unwrap(),
            public_key: validator_signer()
                .get_public_key()
                .await
                .unwrap()
                .clone()
                .to_string(),
            amount: INIT_LOCK,
        }]),
        additional_accounts: vec![
            GenesisAccount {
                account_id: "test.near".parse().unwrap(),
                balance: INITIAL_BALANCE,
                ..Default::default()
            },
            GenesisAccount {
                account_id: "alice.near".parse().unwrap(),
                balance: INITIAL_BALANCE,
                ..Default::default()
            },
            GenesisAccount {
                account_id: "bob.near".parse().unwrap(),
                balance: INITIAL_BALANCE,
                ..Default::default()
            },
            GenesisAccount {
                account_id: WNEAR.parse().unwrap(),
                balance: INITIAL_BALANCE,
                ..Default::default()
            },
            GenesisAccount {
                account_id: INTENTS.parse().unwrap(),
                balance: INITIAL_BALANCE,
                ..Default::default()
            },
            GenesisAccount {
                account_id: LST.parse().unwrap(),
                public_key: validator_public_key,
                private_key: validator_private_key.to_string(),
                balance: INITIAL_BALANCE.saturating_sub(INIT_LOCK),
                locked: INIT_LOCK,
            },
        ],
        ..Default::default()
    }
}

fn retry_strategy() -> impl Iterator<Item = Duration> {
    tokio_retry::strategy::ExponentialBackoff::from_millis(100)
        .factor(2)
        .max_delay(Duration::from_secs(10))
        .map(tokio_retry::strategy::jitter)
        .take(10)
}
