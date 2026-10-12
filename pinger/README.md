# liquid-staking-token-pinger

A small daemon that watches NEAR epoch transitions and sends a `ping`
transaction to a configured liquid-staking-token contract once per epoch, so
the contract can recalculate rewards and staked amounts.

## How it works

- An **epoch watcher** subscribes to a NEAR block stream via
  [`block-client-rs`](https://github.com/aurora-is-near/block-client-rs) and
  detects when the `epoch_id` in incoming block headers changes. It decodes
  only `block.header.epoch_id` from the compressed NEAR block payload,
  ignoring other fields so unrelated block schema changes do not affect
  epoch detection. Blocks that cannot be decoded are logged and skipped.
  The watcher automatically reconnects if the stream fails or no block
  arrives within 20 seconds; failed connection attempts are retried after
  10 seconds.
- A **tx sender** signs and submits the configured contract call on every
  epoch change, with exponential-backoff retries on transient failures.
- The last seen `epoch_id` is persisted to disk so the daemon does not re-ping
  on restart within the same epoch.

The two components communicate over an in-process channel. `SIGINT` / `SIGTERM`
triggers a graceful shutdown that is honored even while the watcher is mid-
reconnect: the last epoch is flushed to disk and the sender drains before the
process exits.

## Build

The crate is a workspace member. From the repository root, enter `pinger/`
and build it by package so its local `.cargo/config.toml` is picked up:

```sh
cd pinger
cargo build -p liquid-staking-token-pinger --release
```

The toolchain is pinned via the workspace `rust-toolchain.toml` (Rust 1.95.0).
`.cargo/config.toml` sets `git-fetch-with-cli = true` so the system Git
handles Git dependencies. The binary is written to the workspace's
`target/release/` directory.

## Configuration

The daemon reads a YAML config file. See [`pinger.yml`](./pinger.yml) for a
template. Fields:

| Field                | Description                                                                                                      |
|----------------------|------------------------------------------------------------------------------------------------------------------|
| `log_level`          | Tracing level for crate logs (`trace`, `debug`, `info`, …).                                                      |
| `epoch_id_path`      | Path to the file used to persist the last seen epoch id.                                                         |
| `client`             | `block-client-rs` config: NATS `url`, `token`, `stream_name`, buffers.                                           |
| `sender.rpc_url`     | Optional NEAR RPC URL. When omitted, the public mainnet endpoint is used.                                        |
| `sender.chain_id`    | Optional NEAR chain id (e.g. `mainnet`, `testnet`). Only consulted when `rpc_url` is set. Defaults to `mainnet`. |
| `sender.account_id`  | Signer account id.                                                                                               |
| `sender.private_key` | Signer private key (`ed25519:…`). May be overridden by env var.                                                  |
| `sender.contract_id` | Target liquid-staking-token contract.                                                                            |
| `sender.method_name` | Method to invoke on each epoch (typically `ping`).                                                               |
| `sender.args`        | Optional JSON args passed to the method.                                                                         |
| `sender.gas`         | Optional gas budget (in gas units) for the transaction. Defaults to 50 TGas.                                     |

### Environment overrides

- `PRIVATE_KEY` — overrides `sender.private_key` when set. Recommended for
  production deployments so the secret never lives in the config file.
- `RUST_LOG` — overrides `log_level` using the standard `EnvFilter` syntax.

## Run

Set `sender.account_id` in the config and provide the signer private key via
`PRIVATE_KEY` or `sender.private_key`. Run from `pinger/` after building:

```sh
mkdir -p data
../target/release/liquid-staking-token-pinger -c ./pinger.yml
```

Relative paths, including `epoch_id_path`, are resolved against the process's
working directory. The state file is created when the first block's epoch
differs from the saved epoch (or no valid saved epoch exists), and overwritten
on subsequent epoch changes. Its parent directory must already exist and be
writable. The file records the last observed epoch, without waiting for the
ping transaction to succeed.

## Docker

Build and run the image from the repository root:

```sh
docker build -t liquid-staking-token-pinger -f pinger/Dockerfile .
mkdir -p data
docker run -d \
    --restart unless-stopped \
    -e PRIVATE_KEY=ed25519:... \
    -v "$(pwd)/pinger/pinger.yml:/app/pinger.yml:ro" \
    -v "$(pwd)/data:/app/data" \
    liquid-staking-token-pinger
```

- The default config is baked in at `/app/pinger.yml`. Override it by
  bind-mounting your own file at the same path.
- `/app/data` is declared as a volume; the persisted epoch id lives there
  (`epoch_id_path: data/epoch_id` in the default config, resolved against
  the container's `/app` working directory). Ensure the host `data/` directory
  is writable by the container's non-root `pinger` user.
