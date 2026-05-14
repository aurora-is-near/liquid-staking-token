# liquid-staking-token-pinger

A small daemon that watches NEAR epoch transitions and sends a `ping`
transaction to a configured liquid-staking-token contract once per epoch, so
the contract can recalculate rewards and staked amounts.

## How it works

- An **epoch watcher** subscribes to a NEAR block stream via
  [`block-client-rs`](https://github.com/aurora-is-near/block-client-rs) and
  detects when the `epoch_id` in incoming block headers changes.
- A **tx sender** signs and submits the configured contract call on every
  epoch change, with exponential-backoff retries on transient failures.
- The last seen `epoch_id` is persisted to disk so the daemon does not re-ping
  on restart within the same epoch.

The two components communicate over an in-process channel; ctrl-c triggers a
graceful shutdown.

## Build

The crate is excluded from the workspace, so build it from this directory:

```sh
cd pinger
cargo build --release
```

The toolchain is pinned via `rust-toolchain.toml` (Rust 1.88).
`.cargo/config.toml` sets `git-fetch-with-cli = true` so the system Git
handles SSH-authenticated dependencies.

## Configuration

The daemon reads a YAML config file. See [`pinger.yml`](./pinger.yml) for a
template. Fields:

| Field                | Description                                                            |
|----------------------|------------------------------------------------------------------------|
| `log_level`          | Tracing level for crate logs (`trace`, `debug`, `info`, …).            |
| `epoch_id_path`      | Path to the file used to persist the last seen epoch id.               |
| `client`             | `block-client-rs` config: NATS `url`, `token`, `stream_name`, buffers. |
| `sender.rpc_url`     | Optional NEAR RPC URL. Defaults to mainnet.                            |
| `sender.account_id`  | Signer account id.                                                     |
| `sender.private_key` | Signer private key (`ed25519:…`). May be overridden by env var.        |
| `sender.contract_id` | Target liquid-staking-token contract.                                  |
| `sender.method_name` | Method to invoke on each epoch (typically `ping`).                     |
| `sender.args`        | Optional JSON args passed to the method.                               |

### Environment overrides

- `PRIVATE_KEY` — overrides `sender.private_key` when set. Recommended for
  production deployments so the secret never lives in the config file.
- `RUST_LOG` — overrides `log_level` using the standard `EnvFilter` syntax.

## Run

```sh
./target/release/liquid-staking-token-pinger -c ./pinger.yml
```

The state file at `epoch_id_path` is created on the first epoch change and
overwritten thereafter.

## Docker

Build and run the image from this directory:

```sh
docker build -t lst-pinger .
docker run -d \
    --restart unless-stopped \
    -e PRIVATE_KEY=ed25519:... \
    -v "$(pwd)/pinger.yml:/app/pinger.yml" \
    -v "$(pwd)/data:/app/data" \
    lst-pinger
```

- The default config is baked in at `/app/pinger.yml`. Override it by
  bind-mounting your own file at the same path.
- `/app/data` is declared as a volume; the persisted epoch id lives there
  (`epoch_id_path: data/epoch_id` in the default config, resolved against
  the container's `/app` working directory).
