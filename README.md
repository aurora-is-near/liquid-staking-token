# Liquid Staking Token

A NEAR Protocol smart contract that enables liquid staking functionality. Users can stake their NEAR tokens and receive
liquid staking tokens (LST) in return, maintaining liquidity while earning staking rewards.

## Overview

This liquid staking solution allows NEAR token holders to:

- **Stake NEAR tokens** and receive tradeable LST tokens representing their stake
- **Maintain liquidity** by using LST tokens in DeFi applications while tokens remain staked
- **Unstake at any time** with a standard 4-epoch (~2 days) unbonding period
- **Choose token format** for both staking (native NEAR or wNEAR) and unstaking (native NEAR or wNEAR)

## Key Features

- **Dual staking methods**: Stake with native NEAR or wrapped NEAR (wNEAR)
- **Flexible withdrawals**: Receive unstaked tokens as native NEAR or wNEAR
- **NEP-141 compliant**: LST tokens follow the fungible token standard
- **Cross-contract integration**: Seamless integration with DeFi protocols via `ft_transfer_call`
- **Single validator staking**: All staked NEAR is delegated to a single validator for simplicity
- **Role-based access control**: Admin, pause, and unpause roles for contract management

## How It Works

1. **Staking**: Users stake NEAR (native or wNEAR) and receive LST tokens at a 1:1 ratio
2. **Validator delegation**: The contract stakes all NEAR with a pre-configured validator
3. **Unstaking**: Users burn LST tokens to initiate unstaking, creating a withdrawal queue entry
4. **Cooldown period**: After 4 epochs (~2 days), users can withdraw their NEAR
5. **Withdrawal**: Users claim their NEAR (native or wNEAR) after the cooldown completes

The contract manages the staking lifecycle, handles storage deposits, and supports both simple transfers and complex
cross-contract calls through standardized message formats.

---

## How to Build Locally?

Install [`cargo-near`](https://github.com/near/cargo-near) and run:

```bash
cargo near build non-reproducible-wasm --manifest-path token/Cargo.toml 
```

## How to Test Locally?

```bash
cargo test
```

## How to Deploy?

Deployment is automated with GitHub Actions CI/CD pipeline.
To deploy manually, install [`cargo-near`](https://github.com/near/cargo-near) and run:

If you deploy for debugging purposes:

```bash
cargo near deploy build-non-reproducible-wasm --manifest-path token/Cargo.toml      
```

If you deploy production ready smart contract:

```bash
cargo near deploy build-reproducible-wasm --manifest-path token/Cargo.toml
```

## Initialize the contract

Call `new` once to deploy and initialize the contract.

```bash
near contract call-function as-transaction <CONTRACT_ID> new \
  json-args '{
    "owner_id":           "admin.near",
    "wnear_id":           "wrap.near",
    "validator_public_key": "ed25519:<BASE58_KEY>",
    "metadata": {
      "spec":     "ft-1.0.0",
      "name":     "Liquid Staking Token",
      "symbol":   "LST",
      "decimals": 24
    }
  }' \
  prepaid-gas '30 Tgas' \
  attached-deposit '0 NEAR' \
  sign-as <DEPLOYER_ACCOUNT> \
  network-config mainnet
```

### Parameters

| Parameter              | Type                    | Required | Description                                                                                                                                |
|------------------------|-------------------------|----------|--------------------------------------------------------------------------------------------------------------------------------------------|
| `owner_id`             | `AccountId`             | Yes      | Account that receives all admin/pause/unpause roles.                                                                                       |
| `wnear_id`             | `AccountId`             | Yes      | Address of the wNEAR (wrapped NEAR) contract used for wNEAR-based staking and withdrawal.                                                  |
| `validator_public_key` | `PublicKey`             | Yes      | Ed25519 public key of the validator node. The contract stakes its locked balance to this key.                                              |
| `metadata`             | `FungibleTokenMetadata` | Yes      | Standard NEP-148 metadata (`spec`, `name`, `symbol`, `decimals`, optional `icon` / `reference` / `reference_hash`).                        |
| `init_lock`            | `NearToken` (yoctoNEAR) | No       | Pre-set the initial locked balance. Useful in single-validator test sandboxes where the locked balance cannot be zero. Omit in production. |

> The contract panics if called a second time (`"Already initialized"`).

---

## Staking

There are two ways to stake: sending **native NEAR** directly, or sending **wNEAR** via `ft_transfer_call`.

In both cases a `StakeMessage` JSON object is supplied as the `msg` argument to describe where the minted LST tokens
should be sent.

---

### Option A — Stake with native NEAR

Call `stake` on the LST contract and attach the NEAR you want to stake.

```bash
near contract call-function as-transaction <CONTRACT_ID> stake \
  json-args '{
    "args": {
      "receiver_id": "alice.near",
      "storage_deposit": "1250000000000000000000",
      "msg":  null,
      "memo": null,
      "min_gas": null
    }
  }' \
  prepaid-gas '100 Tgas' \
  attached-deposit '10 NEAR' \
  sign-as alice.near \
  network-config mainnet
```

The full attached deposit is staked. The minted LST tokens are transferred to `receiver_id`.

---

### Option B — Stake with wNEAR

Call `ft_transfer_call` on the **wNEAR contract**. The LST contract is the receiver. The `msg` field must be a
JSON-serialised `StakeMessage`.

```bash
near contract call-function as-transaction wrap.near ft_transfer_call \
  json-args '{
    "receiver_id": "<CONTRACT_ID>",
    "amount":      "10000000000000000000000000",
    "memo":        null,
    "msg": "{\"receiver_id\":\"alice.near\",\"storage_deposit\":null,\"msg\":null,\"memo\":null,\"min_gas\":null}"
  }' \
  prepaid-gas '100 Tgas' \
  attached-deposit '1 yoctoNEAR' \
  sign-as alice.near \
  network-config mainnet
```

The contract unwraps the wNEAR to NEAR internally, stakes it, and transfers the minted LST tokens as specified in
`StakeMessage`.

---

### `StakeMessage` — Arguments

`StakeMessage` is passed as the `msg` field (a JSON string) when staking via wNEAR, or as the `args` object when calling
`stake` directly.

```jsonc
{
  "receiver_id":      "alice.near",         // required
  "storage_deposit":  "1250000000000000000000", // optional
  "msg":              "...",                // optional
  "memo":             "my stake",          // optional
  "min_gas":          35000000000000       // optional
}
```

| Field             | Type                           | Required | Description                                                                                                                                                                                                                                              |
|-------------------|--------------------------------|----------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `receiver_id`     | `AccountId`                    | Yes      | Account that will receive the minted LST tokens.                                                                                                                                                                                                         |
| `storage_deposit` | `NearToken` (yoctoNEAR string) | No       | If set, this amount is deducted from the staked NEAR and used to call `storage_deposit` on the LST contract for `receiver_id`, registering the account before the token transfer. Required when `receiver_id` is not yet registered on the LST contract. |
| `msg`             | `String`                       | No       | If present the LST tokens are delivered via `ft_transfer_call` (passing this string as `msg`). If absent, a plain `ft_transfer` is used. Useful when the receiver is a contract that needs to be notified (e.g. an intents/DEX contract).                |
| `memo`            | `String`                       | No       | Memo forwarded to `ft_transfer_call`. Ignored when `msg` is absent.                                                                                                                                                                                      |
| `min_gas`         | `Gas` (u64)                    | No       | Minimum gas (in gas units) attached to the LST transfer step. Defaults to 35 TGas. Increase if the downstream `ft_on_transfer` handler requires more gas.                                                                                                |

**Token amount minted.** Currently 1 yoctoNEAR staked = 1 yoctoLST minted (1:1). Future versions will adjust this ratio
based on accrued validator rewards.

---

## Unstaking

Unstaking is performed by sending LST tokens **back to the LST contract itself** via `ft_transfer_call`. The `msg` field
must be a JSON-serialised `UnstakeMessage`.

```bash
near contract call-function as-transaction <CONTRACT_ID> ft_transfer_call \
  json-args '{
    "receiver_id": "<CONTRACT_ID>",
    "amount":      "10000000000000000000000000",
    "memo":        null,
    "msg": "{\"receiver_id\":\"alice.near\",\"withdraw_tokens\":\"native\"}"
  }' \
  prepaid-gas '100 Tgas' \
  attached-deposit '1 yoctoNEAR' \
  sign-as alice.near \
  network-config mainnet
```

On success the LST tokens are burned and an unstake queue entry is recorded keyed by the hash of the `UnstakeMessage`.
The NEAR is released only after the **4-epoch cooldown**.

---

### `UnstakeMessage` — Arguments

```jsonc
{
  "receiver_id":     "alice.near",      // required
  "withdraw_tokens": "native"           // required — see variants below
}
```

| Field             | Type             | Required | Description                                                      |
|-------------------|------------------|----------|------------------------------------------------------------------|
| `receiver_id`     | `AccountId`      | Yes      | Account that will receive the unstaked NEAR (or wNEAR).          |
| `withdraw_tokens` | `WithdrawTokens` | Yes      | Specifies how the unstaked NEAR is returned. See variants below. |

#### `WithdrawTokens` variants

**`"native"` — receive plain NEAR**

```json
"native"
```

The unstaked NEAR is sent as a native NEAR transfer to `receiver_id` once `withdraw` is called.

---

**`{"wnear": {...}}` — receive wNEAR**

```jsonc
{
  "wnear": {
    "storage_deposit": "1250000000000000000000", // optional
    "msg":             "...",                    // optional
    "memo":            "my unstake",             // optional
    "min_gas":         35000000000000            // optional
  }
}
```

The unstaked NEAR is wrapped back to wNEAR and delivered to `receiver_id`.

| Sub-field         | Type                           | Required | Description                                                                                                                                                                            |
|-------------------|--------------------------------|----------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `storage_deposit` | `NearToken` (yoctoNEAR string) | No       | If set, this amount is deducted from the withdrawn NEAR and used to call `storage_deposit` on the wNEAR contract for `receiver_id`, registering the account before the wNEAR transfer. |
| `msg`             | `String`                       | No       | If present, delivers wNEAR via `ft_transfer_call` on the wNEAR contract (passing this string as `msg`). If absent, a plain `ft_transfer` is used.                                      |
| `memo`            | `String`                       | No       | Memo forwarded to the wNEAR `ft_transfer_call`.                                                                                                                                        |
| `min_gas`         | `Gas` (u64)                    | No       | Minimum gas for the wNEAR transfer step. Defaults to 35 TGas.                                                                                                                          |

> **Important:** the same `UnstakeMessage` JSON you pass during unstaking must be passed again verbatim when calling
`withdraw`. The contract derives a Keccak-256 hash of the message and uses it as the queue key.

---

## Withdrawing after cooldown

After **4 epochs** (≈ 2 days on mainnet) have passed since unstaking, call `withdraw` with the same `UnstakeMessage` to
release the NEAR.

```bash
near contract call-function as-transaction <CONTRACT_ID> withdraw \
  json-args '{
    "args": {
      "receiver_id":     "alice.near",
      "withdraw_tokens": "native"
    }
  }' \
  prepaid-gas '80 Tgas' \
  attached-deposit '0 NEAR' \
  sign-as alice.near \
  network-config mainnet
```

The contract:

1. Looks up the unstake queue entry by the hash of `args`.
2. Checks that at least 4 epochs have elapsed.
3. Transfers the NEAR (or wNEAR) to `receiver_id`.
4. Removes the entry from the queue.

If called too early, the transaction panics with `"It's too early to withdraw"`.

---

## Full flow examples

### Native NEAR → LST → intents contract

```text
1. alice calls stake({ receiver_id: "intents.near", msg: "{}", ... })
   attached: 10 NEAR
   → LST tokens transferred to intents.near via ft_transfer_call

2. alice calls ft_transfer_call on LST contract
   receiver_id: <CONTRACT_ID>, amount: <LST>, msg: UnstakeMessage (native)
   → unstake queued

3. wait 4 epochs

4. alice calls withdraw({ receiver_id: "alice.near", withdraw_tokens: "native" })
   → 10 NEAR returned to alice
```

### wNEAR → LST → wNEAR round-trip

```text
1. alice calls ft_transfer_call on wrap.near
   receiver_id: <CONTRACT_ID>, amount: 10 wNEAR
   msg: StakeMessage { receiver_id: "alice.near", ... }
   → LST tokens transferred to alice.near via ft_transfer

2. alice calls ft_transfer_call on LST contract
   receiver_id: <CONTRACT_ID>, amount: <LST>
   msg: UnstakeMessage { receiver_id: "alice.near", withdraw_tokens: { "wnear": {} } }
   → unstake queued

3. wait 4 epochs

4. alice calls withdraw({ receiver_id: "alice.near", withdraw_tokens: { "wnear": {} } })
   → 10 wNEAR returned to alice
```
