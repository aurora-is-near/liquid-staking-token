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
- **Reward-bearing exchange rate**: Validator rewards are folded into the LST/NEAR exchange rate via
  `ping`; LST holders' NEAR-denominated value grows over time without any per-account claim step
- **Configurable protocol fee**: A bps fee on each reward sync is minted as LST to a treasury account
  (capped at 20%, owner-adjustable)
- **Role-based access control**: Admin, pause, and unpause roles for contract management
- **Pausable user surface**: `stake`, `withdraw`, `ping`, `ft_transfer`, `ft_transfer_call`,
  and `ft_on_transfer` can be paused by accounts holding the pause role

## How It Works

1. **Staking**: Users stake NEAR (native or wNEAR) and receive LST tokens at the current exchange rate
   (1:1 only when no rewards have been synced yet; afterward each LST is worth strictly more than 1 NEAR)
2. **Validator delegation**: The contract stakes all NEAR with a pre-configured validator
3. **Reward syncing**: Anyone may call `ping` (or it runs implicitly on each stake/unstake) to fold the
   validator's accrued rewards into the contract's tracked balance, lifting the LST/NEAR exchange rate
   and minting the configured protocol fee as LST to the treasury
4. **Unstaking**: Users burn LST tokens to initiate unstaking, creating a withdrawal queue entry
5. **Cooldown period**: After 4 epochs (~2 days), users can withdraw their NEAR
6. **Withdrawal**: Users claim their NEAR (native or wNEAR) after the cooldown completes

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
    "treasury_id":        "treasury.near",
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

| Parameter              | Type                    | Required | Description                                                                                                            |
|------------------------|-------------------------|----------|------------------------------------------------------------------------------------------------------------------------|
| `owner_id`             | `AccountId`             | Yes      | Account that receives all admin/pause/unpause roles.                                                                   |
| `wnear_id`             | `AccountId`             | Yes      | Address of the wNEAR (wrapped NEAR) contract used for wNEAR-based staking and withdrawal.                              |
| `treasury_id`          | `AccountId`             | Yes      | Account that receives the protocol fee, minted as LST on every reward sync. May equal `owner_id` or any other account. |
| `validator_public_key` | `PublicKey`             | Yes      | Ed25519 public key of the validator node. The contract stakes its locked balance to this key.                          |
| `metadata`             | `FungibleTokenMetadata` | Yes      | Standard NEP-148 metadata (`spec`, `name`, `symbol`, `decimals`, optional `icon` / `reference` / `reference_hash`).    |

> The contract panics if called a second time (`"Already initialized"`).

**Genesis stake.** At init time the contract reads `env::account_locked_balance()` and, if non-zero, mints an equal
amount of LST to `treasury_id` and seeds `total_staked_amount` with that value. To bootstrap the pool with real backing,
the deployer should pre-stake to `validator_public_key` before calling `new`. If the locked balance is zero at init,
no LST is minted and the first staker bootstraps the pool at a 1:1 NEAR-to-LST ratio.

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
      "min_gas": null,
      "refund_message": null
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
  "min_gas":          35000000000000,      // optional
  "refund_message":   { ... }              // optional — see below
}
```

| Field             | Type                           | Required | Description                                                                                                                                                                                                                                                                                                                              |
|-------------------|--------------------------------|----------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `receiver_id`     | `AccountId`                    | Yes      | Account that will receive the minted LST tokens.                                                                                                                                                                                                                                                                                         |
| `storage_deposit` | `NearToken` (yoctoNEAR string) | No       | If set, this amount is deducted from the staked NEAR and used to call `storage_deposit` on the LST contract for `receiver_id`, registering the account before the token transfer. Required when `receiver_id` is not yet registered on the LST contract.                                                                                 |
| `msg`             | `String`                       | No       | If present, `ft_on_transfer` is called on `receiver_id` after the LST tokens are minted (passing this string as `msg`). If absent, no callback is made. Useful when the receiver is a contract that needs to be notified (e.g. an intents/DEX contract).                                                                                 |
| `memo`            | `String`                       | No       | Memo forwarded to the `ft_on_transfer` call. Ignored when `msg` is absent.                                                                                                                                                                                                                                                               |
| `min_gas`         | `Gas` (u64)                    | No       | Minimum gas (in gas units) attached to the `ft_on_transfer` step. Defaults to 35 TGas. Increase if the downstream `ft_on_transfer` handler requires more gas.                                                                                                                                                                            |
| `refund_message`  | `UnstakeMessage`               | No       | If `msg` is set and `receiver_id` returns a partial or full refund from `ft_on_transfer`, the contract automatically initiates an unstake using this message. The refunded LST tokens are burned and the corresponding NEAR enters the withdrawal queue. If omitted, refunded tokens remain on `receiver_id` with no automatic recovery. |

**Token amount minted.** The amount of LST minted is `stake_amount * total_lst_supply / total_staked_amount`, floored
(1:1 only while no LST has been minted or no rewards have been synced). Once `ping` has folded validator rewards into
the tracked staked balance, each LST is worth strictly more than 1 NEAR — staking the same NEAR amount mints fewer LST,
and unstaking the same LST amount returns more NEAR. The current ratio can be inspected via `get_exchange_rate`.

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

#### Partial delivery and retries (wNEAR with `msg`)

When wNEAR is delivered via `ft_transfer_call` (i.e. `msg` is set), the receiver may consume only part of the amount.
The unconsumed wNEAR is refunded back to the LST contract by the wNEAR contract's `ft_resolve_transfer`, and the queue
entry is shrunk to the residual amount so the user can retry `withdraw` for the remainder.

A few consequences worth knowing:

- Concurrent `withdraw` calls for the same queue entry are rejected (`"The withdrawal for this hash is already in
  progress"`). Wait for the in-flight call to complete before retrying.
- On retry, neither `near_deposit` nor `storage_deposit` is performed again — the contract sends the residual wNEAR it
  already holds, and assumes the receiver is still registered.
- If the receiver's `ft_on_transfer` panics outright (no partial delivery), nothing changes in the queue and the user
  may simply call `withdraw` again.

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

If called too early, the transaction panics with `"The cooldown hasn't passed yet"`.

---

## Rewards and protocol fee

Validator rewards are not credited to a per-account balance. Instead, the contract reads its own
`account_locked_balance + account_balance` and treats any growth above the previously-recorded total as the new reward
for the epoch. That delta is added to the tracked staked amount, which raises the LST/NEAR exchange rate. A configurable
share is minted as LST to the treasury account as the protocol fee.

### `ping` — sync rewards on demand

```bash
near contract call-function as-transaction <CONTRACT_ID> ping \
  json-args '{}' \
  prepaid-gas '50 Tgas' \
  attached-deposit '0 NEAR' \
  sign-as <ANY_ACCOUNT> \
  network-config mainnet
```

- Anyone may call `ping`. It is a no-op if rewards have already been synced in the current epoch.
- When new rewards are detected, `ping` re-stakes the new total to the validator so that the locked balance keeps
  earning on the increased principal.
- If the restake action itself fails (e.g. the validator key was retired), `ping`'s callback unstakes everything and
  emits a `Restake failed; …Admin recovery required` log line. The contract then continues serving withdrawals from
  the unbonded NEAR but stops earning rewards until an admin calls `set_validator_public_key` and triggers a fresh
  stake (see [Admin operations](#admin-operations)).
- `sync_rewards_internal` also runs implicitly inside `stake`, the wNEAR-staking callback, and `handle_unstaking`, so
  active users do not need to call `ping` themselves to get an up-to-date exchange rate.

### `set_protocol_fee_bps` (admin only)

```bash
near contract call-function as-transaction <CONTRACT_ID> set_protocol_fee_bps \
  json-args '{ "fee_bps": 1000 }' \
  prepaid-gas '20 Tgas' \
  attached-deposit '0 NEAR' \
  sign-as admin.near \
  network-config mainnet
```

- `fee_bps` is in basis points (1 bp = 0.01%). `1000` = 10%.
- Capped at `2_000` (20%); higher values panic.
- Only accounts with the `Admin` role may call this method.
- The fee applies to *future* reward syncs only. Past syncs are not retroactively re-fee'd.

### View methods

| Method                             | Returns                                    | Notes                                                           |
|------------------------------------|--------------------------------------------|-----------------------------------------------------------------|
| `get_exchange_rate`                | `{ numerator, denominator }` (yocto units) | Effective LST→NEAR ratio. Equals `1/1` before any rewards sync. |
| `get_reward_fee_fraction`          | `{ numerator, denominator }` (bps / 10000) | Currently configured protocol fee.                              |
| `get_total_staked_balance`         | `NearToken`                                | Tracked NEAR backing the LST supply.                            |
| `get_total_pending_withdrawals`    | `NearToken`                                | Sum of NEAR amounts queued for withdrawal.                      |
| `get_total_balance`                | `NearToken`                                | Last-recorded `locked + unlocked` balance in NEAR.              |
| `get_number_of_accounts`           | `u64`                                      | Number of LST holders.                                          |
| `get_owner_id` / `get_treasury_id` | `AccountId`                                | Configured roles.                                               |
| `get_staking_key`                  | `PublicKey`                                | Validator key the contract delegates to.                        |
| `get_version`                      | `&'static str`                             | Crate version baked in at build time.                           |

---

## Pausing

The contract uses `near-plugins`' `Pausable` machinery. The following user-facing methods can be paused independently
by accounts holding the `Admin` or `PauseManager` role:

- `stake`
- `withdraw`
- `ping`
- `ft_transfer`, `ft_transfer_call`
- `ft_on_transfer` (which gates wNEAR-based staking and LST-based unstaking)

Accounts holding the `Admin` or `UnpauseManager` role may unpause. The `Upgradable` plugin is also enabled — code
staging, deploying, and upgrade-duration management are all gated on the `Admin` role.

---

## Admin operations

All methods below are gated on the `Admin` role, granted to `owner_id` at init.
`set_protocol_fee_bps` is documented under [Rewards and protocol fee](#set_protocol_fee_bps-admin-only).

### `set_validator_public_key` — change the staking validator

```bash
near contract call-function as-transaction <CONTRACT_ID> set_validator_public_key \
  json-args '{ "validator_public_key": "ed25519:<NEW_BASE58_KEY>" }' \
  prepaid-gas '20 Tgas' \
  attached-deposit '0 NEAR' \
  sign-as admin.near \
  network-config mainnet
```

- Replaces the in-state validator public key. The contract's currently-locked NEAR remains bonded to the *old*
  validator until a stake action fires with the new key.
- Migration is propagated by the next stake-bearing operation: `stake`, `unstake`, or `ping` (after rewards are
  detected). NEAR's runtime then schedules unbonding from the old validator over the standard 4-epoch period before
  bonding to the new one.
- If the old validator is unresponsive (no rewards arrive, so `ping` becomes a no-op), use `add_full_access_key` to
  manually fire a stake action and force the migration.
- A typo (or otherwise-invalid key) bricks subsequent stake operations until the admin reissues the call. The contract
  performs no key-ownership check.

### `add_full_access_key` — emergency recovery key

```bash
near contract call-function as-transaction <CONTRACT_ID> add_full_access_key \
  json-args '{ "public_key": "ed25519:<BASE58_KEY>" }' \
  prepaid-gas '20 Tgas' \
  attached-deposit '1 yoctoNEAR' \
  sign-as admin.near \
  network-config mainnet
```

- Adds a full access key to the contract's account so the holder can sign arbitrary actions (manual stake, transfer,
  redeploy, etc.) directly against it.
- Intended as a break-glass mechanism, primarily to recover from a dead validator that prevents `ping` from firing a
  fresh stake. Use sparingly: there is no on-chain `delete_key` method, so a granted key can only be removed via a
  contract upgrade.

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

### Native NEAR → intents contract → partial refund recovery

When staking into a DeFi protocol via `msg`, provide `refund_message` to handle the case where the protocol rejects or
partially consumes the LST tokens.

```text
1. alice calls stake({
     receiver_id: "intents.near",
     msg: "...",
     refund_message: { receiver_id: "alice.near", withdraw_tokens: "native" }
   })
   attached: 10 NEAR
   → LST tokens minted to intents.near
   → ft_on_transfer called on intents.near
   → intents.near panics or returns a refund

2. contract detects refund, burns the refunded LST tokens, and queues an unstake
   for alice using the provided refund_message

3. wait 4 epochs

4. alice calls withdraw({ receiver_id: "alice.near", withdraw_tokens: "native" })
   → 10 NEAR returned to alice
```

---

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
