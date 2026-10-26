# Liquid Staking Token

[Release notes](CHANGES.md)

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
- **Role-based access control**: `Admin`, `PauseManager`, `UnpauseManager`, `CodeStager`, and `CodeDeployer` roles
  for contract management
- **Pausable user surface**: `stake`, `withdraw`, `ping`, `ft_transfer`, `ft_transfer_call`,
  and `ft_on_transfer` can be paused by accounts holding the pause role

## How It Works

1. **Staking**: Users stake NEAR (native or wNEAR) and receive LST tokens at the current exchange rate
   (1:1 only when no rewards have been synced yet; afterward each LST is worth strictly more than 1 NEAR)
2. **Validator delegation**: The contract stakes all NEAR with a pre-configured validator
3. **Reward syncing**: Anyone may call `ping` (or it runs implicitly on each stake/unstake) to fold the
   validator's accrued rewards into the contract's tracked balance, lifting the LST/NEAR exchange rate
   and minting the configured protocol fee as LST to the treasury
4. **Unstaking**: Users burn LST tokens to initiate unstaking. Each unstake is recorded as an independent
   tranche keyed by the hash of the `UnstakeMessage`, with its own cooldown — re-unstaking with the same
   message never shifts the cooldown of earlier tranches.
5. **Cooldown period**: Each tranche matures 4 epochs (~2 days) after the unstake that created it
6. **Withdrawal**: A single `withdraw` call claims **all matured tranches** under that message hash in
   one delivery; any tranches still in cooldown stay queued for a future call

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
      "is_storage_deposit": true,
      "msg":  null,
      "memo": null,
      "min_gas": null,
      "refund_message": null
    }
  }' \
  prepaid-gas '200 Tgas' \
  attached-deposit '10 NEAR' \
  sign-as alice.near \
  network-config mainnet
```

The attached deposit is used for staking. If `is_storage_deposit` is `true`, a fixed `0.00125 NEAR` is first reserved
for registration, and only the remainder is staked. The minted LST tokens are transferred to `receiver_id`.

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
    "msg": "{\"receiver_id\":\"alice.near\",\"is_storage_deposit\":false,\"msg\":null,\"memo\":null,\"min_gas\":null}"
  }' \
  prepaid-gas '200 Tgas' \
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
  "receiver_id":         "alice.near",        // required
  "is_storage_deposit":  true,                // optional, default false
  "msg":                 "...",               // optional
  "memo":                "my stake",          // optional
  "min_gas":             35000000000000,      // optional
  "refund_message":      { ... }              // optional — see below
}
```

| Field                | Type             | Required | Description                                                                                                                                                                                                                                                                                                                                                                                                                                             |
|----------------------|------------------|----------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `receiver_id`        | `AccountId`      | Yes      | Account that will receive the minted LST tokens.                                                                                                                                                                                                                                                                                                                                                                                                        |
| `is_storage_deposit` | `bool`           | No       | When `true`, the contract deducts a fixed `0.00125 NEAR` (1250 µNEAR) from the staked amount and calls `storage_deposit` on the LST contract for `receiver_id`. Required when `receiver_id` is not yet registered. **Set only when registration is needed** — if `receiver_id` is already registered, the 1250 µNEAR is not refunded to the sender; it stays on the contract and accrues to the pool as a reward on the next sync. Defaults to `false`. |
| `msg`                | `String`         | No       | If present, `ft_on_transfer` is called on `receiver_id` after the LST tokens are minted (passing this string as `msg`). If absent, no callback is made. Useful when the receiver is a contract that needs to be notified (e.g. an intents/DEX contract).                                                                                                                                                                                                |
| `memo`               | `String`         | No       | Memo forwarded to the `ft_on_transfer` call. Ignored when `msg` is absent.                                                                                                                                                                                                                                                                                                                                                                              |
| `min_gas`            | `Gas` (u64)      | No       | Minimum gas (in gas units) attached to the `ft_on_transfer` step. Defaults to 30 TGas for calls with a message; lower values are raised to 30 TGas. Increase if the downstream `ft_on_transfer` handler requires more gas. Every TGas above the default raises the required gas reservation by the same amount.                                                                                                     |
| `refund_message`     | `UnstakeMessage` | No       | If `msg` is set and `receiver_id` returns a partial or full refund from `ft_on_transfer`, the contract automatically initiates an unstake using this message. The refunded LST tokens are burned and the corresponding NEAR enters the withdrawal queue. If omitted, refunded tokens remain on `receiver_id` with no automatic recovery.                                                                                                                |

**Token amount minted.** The amount of LST minted is `stake_amount * total_lst_supply / total_staked_amount`, floored
(1:1 only while no LST has been minted or no rewards have been synced). Once `ping` has folded validator rewards into
the tracked staked balance, each LST is worth strictly more than 1 NEAR — staking the same NEAR amount mints fewer LST,
and unstaking the same LST amount returns more NEAR. The current ratio can be inspected via `get_exchange_rate`.

**Concurrent stakes are safe.** State mutations (`total_staked_amount`, LST mint, `latest_total_balance`) happen
synchronously in the originating receipt; the validator `stake` action is scheduled as a separate follow-up
promise that targets the *post-mutation* total. Two users staking in the same block each mutate state in their
own receipt and then schedule their own `stake` action — the second action sees the first user's contribution
already reflected, so neither stake amount is lost.

**Failure handling.** If the validator's `stake` action fails (e.g. the validator key was retired), the contract
rolls back the state changes (`total_staked_amount`, the minted LST, and the tracked total balance) and refunds
the deposit — native NEAR is transferred back to the sender; wNEAR is re-wrapped via `near_deposit` and returned.
The user's LST balance is left untouched.

**Gas safety.** Attach 200 TGas for the documented staking flows. The contract checks the remaining gas before
accepting native NEAR or unwrapping wNEAR, and reserves static gas for the entire callback chain. A rejected wNEAR
stake returns its tokens through the wNEAR resolver, without borrowing any existing withdrawal residual.
The reservation grows with `min_gas` and optional storage registration. With the default receiver gas and no storage
registration, it is 90 TGas for native staking, 125 TGas with `msg`, 115 TGas inside wNEAR `ft_on_transfer`, or
150 TGas inside wNEAR `ft_on_transfer` with `msg`. Storage registration adds 5 TGas. Allow additional gas for the
entry receipt and wNEAR's outer transfer/resolver; these numbers are reservations, not transaction gas
recommendations:

| Flow                                      | Reservation (default `min_gas`) | Recommended transaction gas  |
|-------------------------------------------|---------------------------------|------------------------------|
| `stake` with native NEAR                  | 90 TGas (125 TGas with `msg`)   | 200 TGas                     |
| `ft_transfer_call` on wNEAR to the LST    | 115 TGas (150 TGas with `msg`)  | 200 TGas (wNEAR keeps 30)    |
| `msg` with `min_gas` above 30 TGas        | + (`min_gas` − 30 TGas)         | + (`min_gas` − 30 TGas)      |

For a custom `min_gas`, adding its increase over 30 TGas to the 200 TGas recommendation preserves the same margin.
This is a conservative recommendation, not the exact minimum. The complete budget, including execution overhead
and any outer calls, must fit within NEAR's [1 PGas (1000 TGas) transaction gas limit](https://github.com/near/nearcore/blob/master/CHANGELOG.md#2110).
Near that limit, calculate the reservation and overhead explicitly instead of simply capping the recommendation.

If the remaining-gas check rejects a stake, it does so before changing the staking state: native NEAR is refunded
with the failed receipt, and wNEAR is returned to the sender by the wNEAR resolver.

**Staking through NEAR Intents.** An Intents `ft_withdraw` of wNEAR with a `StakeMessage` as `msg` attaches only its
`min_gas` (50 TGas by default) plus a share of the relayer's leftover gas to the wNEAR `ft_transfer_call`. Set the
intent's `min_gas` to at least 200 TGas (more with a custom `StakeMessage.min_gas`). Otherwise the stake may be
rejected and the wNEAR returned to the user's Intents balance.

The serialized `StakeMessage`, including nested messages and memo, is limited to 4096 bytes so callback work stays bounded.

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

Attach at least 100 TGas. The unstake runs in the LST contract's own `ft_on_transfer`, which receives only the gas
left after the transfer; the transfer resolver and the unstake settlement callback each reserve 15 TGas. With too
little gas the unstake is rejected and the LST is refunded by the NEP-141 resolver.

On success the LST tokens are burned and an **independent withdrawal tranche** is recorded under the hash of the
`UnstakeMessage`. The NEAR for that tranche is released only after the **4-epoch cooldown** measured from this
specific unstake.

**Concurrent unstakes are safe.** As with staking, the LST burn and `total_staked_amount` decrement happen
synchronously in the originating receipt, and the validator's restake action is scheduled as a separate follow-up
that targets the post-mutation total. Multiple users (or multiple unstakes by the same user) processed in the
same block each commit their own state delta before their own restake fires, so no unstake amount is lost.

**Failure handling.** If the validator's restake action fails after an unstake has been recorded, the contract
rolls back the state mutations (`total_staked_amount` is restored and the LST is re-credited to the contract
account so it can be returned to the user) and the original `ft_transfer_call` resolves with the user's LST
refunded by the standard NEP-141 mechanism. No tranche is appended to the withdrawal queue.

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
    "is_storage_deposit": true,           // optional, default false
    "msg":                "...",          // optional
    "memo":               "my unstake",   // optional
    "min_gas":            60000000000000  // optional; includes ~30 TGas kept by wNEAR
  }
}
```

The unstaked NEAR is wrapped back to wNEAR and delivered to `receiver_id`.

| Sub-field            | Type        | Required | Description                                                                                                                                                                                                                                                                                                                                                                                                     |
|----------------------|-------------|----------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `is_storage_deposit` | `bool`      | No       | When `true`, the contract deducts a fixed `0.00125 NEAR` (1250 µNEAR) from the withdrawn amount and calls `storage_deposit` on the wNEAR contract for `receiver_id`. **Set only when registration is needed** — if `receiver_id` is already registered on wNEAR, the 1250 µNEAR is not refunded to the user; it ends up on the LST contract's balance and accrues to the pool as a reward. Defaults to `false`. |
| `msg`                | `String`    | No       | If present, delivers wNEAR via `ft_transfer_call` on the wNEAR contract (passing this string as `msg`). If absent, a plain `ft_transfer` is used.                                                                                                                                                                                                                                                               |
| `memo`               | `String`    | No       | Memo forwarded to the wNEAR `ft_transfer_call`.                                                                                                                                                                                                                                                                                                                                                                 |
| `min_gas`            | `Gas` (u64) | No       | Static gas attached to the wNEAR transfer step: `ft_transfer_call` when `msg` is set (default and minimum 30 TGas), plain `ft_transfer` otherwise (default and minimum 2 TGas). wNEAR keeps about 30 TGas of an `ft_transfer_call` for itself, so the receiver's `ft_on_transfer` is guaranteed only `min_gas − 30 TGas` plus a share of unused gas. Set it to 30 TGas plus what the receiver needs. The field is part of the queue hash and cannot be changed for an existing claim; choose it so the complete [`withdraw` gas budget](#withdrawing-after-cooldown) fits within the 1 PGas transaction limit. |

> **Important:** the same `UnstakeMessage` JSON you pass during unstaking must be passed again verbatim when calling
`withdraw`. The contract derives a Keccak-256 hash of the message and uses it as the queue key.
>
> **Re-unstaking with the same `UnstakeMessage` is safe and does *not* reset any prior cooldown.** Each unstake
> appends an independent tranche under the same hash with its own `unstake_epoch`; an earlier tranche's cooldown
> is never shifted by a later unstake. When you call `withdraw`, the contract collapses **all currently-matured
> tranches** under that hash into a single delivery, while any tranches still in cooldown stay queued for a
> later `withdraw`. The "all matured at once" semantics mean one `withdraw` call always claims everything that
> is currently claimable under that message hash.
>
> Bloat protection: same-epoch unstakes fold into one tranche, and matured tranches collapse on every successful
> unstake. A user spamming `unstake` (e.g. 1 yoctoNEAR each call) cannot grow the per-hash storage beyond a
> small bound.

#### Partial delivery and retries (wNEAR with `msg`)

When wNEAR is delivered via `ft_transfer_call` (i.e. `msg` is set), the receiver may consume only part of the amount.
The unconsumed wNEAR is refunded back to the LST contract by the wNEAR contract's `ft_resolve_transfer`, and the
in-flight tranche is updated so the user can retry `withdraw` for the remainder.

The contract tracks the residual precisely: it records how much of the in-flight tranche's claim is **already held
as wNEAR** at the contract (refunded back from a prior partial delivery) versus how much is **still in NEAR form**
on the contract account. The next `withdraw` only `near_deposit`s the still-in-NEAR portion before sending out the
full claim — the residual wNEAR rides along untouched.

A few consequences worth knowing:

- **`near_deposit` is skipped only for the residual portion.** A pure retry (no re-unstake in between) sends just the
  residual wNEAR; no fresh NEAR is converted.
- **The wNEAR storage deposit is paid at most once per in-flight tranche.** The first attempt that flags
  `is_storage_deposit: true` registers the receiver on the wNEAR contract; subsequent retries skip the
  registration step regardless of the flag.
- **A partial-refund residual remains immediately reclaimable.** The leftover stays in the queue as an
  already-matured tranche and is picked up by the next `withdraw` along with any other matured tranches under the
  same hash — no fresh 4-epoch wait. Re-unstaking into the same hash in the meantime simply appends a new
  independent tranche; the residual is unaffected.
- **A failed settlement stays locked.** An invalid or oversized successful result from wNEAR itself,
  or a failed settlement callback, has an uncertain payment outcome. Settlement retains the lock to prevent
  a second payout. Admin recovery must inspect the actual delivery receipts before releasing it.
- **A failed plain `ft_transfer` remains retryable.** Settlement preserves the unpaid claim and returns normally;
  the same callback releases its lock. A failed `ft_transfer_call` is still considered fully consumed,
  since failure of its returned promise chain does not prove that no tokens were delivered.
- **Settlement and unlocking are atomic.** No separate cleanup callback unlocks claims. The
  `remove_lock` endpoint has been removed, so an old callback cannot unlock a subsequent withdrawal
  under the same message hash. This requires no state migration. Pause new withdrawals and let
  existing chains finish before upgrading: already scheduled calls to the removed endpoint will fail,
  and any locked residual or unpaid claim left by an old settlement will need admin reconciliation.

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
  prepaid-gas '120 Tgas' \
  attached-deposit '0 NEAR' \
  sign-as alice.near \
  network-config mainnet
```

Before locking the claim, `withdraw` checks that the remaining gas covers the whole delivery and its settlement:

| `withdraw_tokens`                     | Required remaining gas at the check               |
|---------------------------------------|--------------------------------------------------|
| `"native"`                            | 30 TGas                                          |
| `wnear`, receiver is the LST contract | 40 TGas                                          |
| `wnear` without `msg`                 | 40 TGas + `min_gas` (42 TGas by default)         |
| `wnear` with `msg`                    | 40 TGas + `min_gas` (70 TGas by default)         |

For an external wNEAR receiver, `is_storage_deposit` adds 5 TGas. These are remaining-gas thresholds, so the attached
gas must also cover execution before the check. Attach 120 TGas for the default variants. For external wNEAR
delivery, adding any increase in `min_gas` above its default to 120 TGas is a conservative recommendation that
preserves the same margin; it is not the exact minimum.

The complete transaction must fit within 1 PGas (1000 TGas). For external wNEAR delivery, this means `min_gas`
plus 40 TGas, another 5 TGas if registration is requested, and execution before the check must fit within that
limit. Near the ceiling, calculate this budget explicitly instead of capping the recommendation at 1000 TGas.
Choose `min_gas` with this budget in mind when unstaking: it cannot be changed for an existing claim.

If the remaining-gas check fails, the claim has not been locked and no tokens have been sent, so the call can be
retried with more gas.

The contract:

1. Looks up the queued tranches by the hash of `args`.
2. Collapses every tranche whose 4-epoch cooldown has elapsed into a single in-flight delivery, leaving any
   tranches still in cooldown queued for later.
3. Transfers the summed NEAR (or wNEAR) to `receiver_id`.
4. Drops the in-flight tranche on full success; partially-delivered wNEAR leaves a residual tranche behind that
   can be reclaimed by the next `withdraw`.

If no tranche has matured, the call panics with
`"There are no available tokens for withdrawal for this message hash"`.

A given message hash can have **at most one `withdraw` call in flight at a time**. Concurrent `withdraw` calls for
the same `UnstakeMessage` hash — both native and wNEAR variants — are rejected with `"Unstake request is already in
progress"`. The in-flight tranche is locked while the delivery is pending and unlocked by its settlement callback,
in the same receipt that records the result, so callers should wait for the prior call to finish before retrying.

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

- Anyone may call `ping`. If rewards have already been synced in the current epoch or no new rewards are detected,
  it returns `0` without scheduling a restake.
- When new rewards are detected, `ping` re-stakes the new total to the validator so that the locked balance keeps
  earning on the increased principal.
- If rewards still need to be synced and the live balance is below the tracked total (for example, while wNEAR
  deposits or outgoing payments are in flight), `ping` fails with
  `"Reward sync postponed: balance is below the tracked total. Try again later."` No reward accounting is changed
  and no restake is scheduled by that call. Retry after the pending transfers settle.
- If the restake action itself fails (e.g. the validator key was retired), `ping`'s callback unstakes everything and
  emits a `Restake failed; …Admin recovery required` log line. The contract then continues serving withdrawals from
  the unbonded NEAR but stops earning rewards until an admin calls `set_validator_public_key` with a working key
  (which now restakes immediately as part of the same call — see [Admin operations](#admin-operations)).
- `sync_rewards_internal` also runs implicitly inside `stake`, the wNEAR-staking callback, and `handle_unstaking`, so
  active users do not need to call `ping` themselves to get an up-to-date exchange rate. A temporary balance deficit
  postpones this implicit reward sync without itself failing those operations.

#### Pinger daemon

An off-chain daemon that calls `ping` once per epoch lives in [`pinger/`](./pinger). It subscribes to a NEAR block
stream, detects when the `epoch_id` changes, and submits the `ping` transaction with exponential-backoff retries.
The sender checks transaction execution success, so a postponed-sync error from `ping` triggers a retry. Each epoch
notification allows up to five retries after the initial attempt (six attempts in total). If all attempts fail, the
daemon logs the failure and waits for the next epoch change; it does not keep retrying throughout the current epoch.
The crate is a workspace member and ships with its own `Dockerfile`; see [`pinger/README.md`](./pinger/README.md)
for build, configuration, and deployment.

### `set_protocol_fee_bps` (admin only)

```bash
near contract call-function as-transaction <CONTRACT_ID> set_protocol_fee_bps \
  json-args '{ "fee_bps": 1000 }' \
  prepaid-gas '20 Tgas' \
  attached-deposit '1 yoctoNEAR' \
  sign-as admin.near \
  network-config mainnet
```

- `fee_bps` is in basis points (1 bp = 0.01%). `1000` = 10%.
- Capped at `2_000` (20%); higher values panic.
- Only accounts with the `Admin` role may call this method. Requires 1 yoctoNEAR attached for full-access-key
  confirmation.
- The fee applies to *future* reward syncs only. Past syncs are not retroactively re-fee'd.

### View methods

| Method                                                 | Returns                                                | Notes                                                                                                                                                                     |
|--------------------------------------------------------|--------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `get_exchange_rate`                                    | `{ numerator (str), denominator (str) }` (yocto units) | Effective LST→NEAR ratio. Equals `1/1` before any rewards sync.                                                                                                           |
| `get_reward_fee_fraction`                              | `{ numerator (u16), denominator (u16) }` (bps / 10000) | Currently configured protocol fee.                                                                                                                                        |
| `get_total_staked_balance`                             | `NearToken`                                            | Tracked NEAR backing the LST supply.                                                                                                                                      |
| `get_total_pending_withdrawals`                        | `NearToken`                                            | Sum of NEAR amounts queued for withdrawal.                                                                                                                                |
| `get_total_balance`                                    | `NearToken`                                            | Tracked `locked + unlocked` balance in NEAR. May exceed the live balance while accepted wNEAR deposits are in flight.                                                     |
| `get_number_of_accounts`                               | `u64`                                                  | Number of LST holders.                                                                                                                                                    |
| `get_owner_id` / `get_treasury_id`                     | `AccountId`                                            | Configured roles.                                                                                                                                                         |
| `get_staking_key`                                      | `PublicKey`                                            | Validator key the contract delegates to.                                                                                                                                  |
| `get_version`                                          | `&'static str`                                         | Crate version baked in at build time.                                                                                                                                     |
| `get_withdrawal_request_tranches({ hash })`            | `Option<Vec<Tranche>>`                                 | Every tranche queued under `hash` (locked + unlocked, lock state hidden), or `None` if the hash has no entry. Use to display a single user's pending withdrawals.         |
| `get_withdrawal_requests({ skip, limit })`             | `Vec<WithdrawalRequest>`                               | Paginated dump of every queue entry: `{ hash (base58), tranches }`. `limit` is silently clamped to 100. Iteration order is **not stable across removals**.                |
| `get_hashes_available_for_withdrawal({ skip, limit })` | `Vec<CryptoHash>`                                      | Paginated list of hashes with at least one **unlocked + matured** tranche — i.e. the set on which the next `withdraw` would succeed right now. `limit` is clamped to 100. |
| `get_withdrawal_requests_count`                        | `u32`                                                  | Number of distinct queue entries. Pair with `get_withdrawal_requests` for stable totals.                                                                                  |

---

## Pausing

The contract uses `near-plugins`' `Pausable` machinery. The following user-facing methods can be paused independently
by accounts holding the `Admin` or `PauseManager` role:

- `stake`
- `withdraw`
- `ping`
- `ft_transfer`, `ft_transfer_call`
- `ft_on_transfer` (which gates wNEAR-based staking and LST-based unstaking)

Accounts holding the `Admin` or `UnpauseManager` role may unpause. The `Upgradable` plugin allows accounts holding
`Admin` or `CodeStager` to stage code, and accounts holding `Admin` or `CodeDeployer` to deploy staged code.
Initializing the staging duration, staging a duration update, and applying that update all require `Admin`.

---

## Admin operations

All methods below are gated on the `Admin` role, granted to `owner_id` at init, and require 1 yoctoNEAR attached
for full-access-key confirmation. `set_protocol_fee_bps` is documented under
[Rewards and protocol fee](#set_protocol_fee_bps-admin-only).

### `set_validator_public_key` — change the staking validator

```bash
near contract call-function as-transaction <CONTRACT_ID> set_validator_public_key \
  json-args '{ "validator_public_key": "ed25519:<NEW_BASE58_KEY>" }' \
  prepaid-gas '50 Tgas' \
  attached-deposit '1 yoctoNEAR' \
  sign-as admin.near \
  network-config mainnet
```

- Replaces the in-state validator public key and **immediately schedules a restake against the new key** in the
  same transaction — no need to wait for the next user-initiated `stake` / `unstake` / `ping`. NEAR's runtime then
  schedules unbonding from the old validator over the standard 4-epoch period before bonding to the new one.
- Returns a `Promise` (was a void method in earlier versions). Off-chain integrations that previously called this
  method may need to update their typings to handle the promise return.
- Requires 1 yoctoNEAR attached for full-access-key confirmation.
- **Bad-key recovery**: if the new key is invalid or otherwise causes the restake action to fail, the contract's
  `on_restake` callback (the same one that guards `ping`) fires `stake(0, new_key)`, which initiates full
  unbonding from any prior key. The contract then continues serving withdrawals from the unbonded NEAR but stops
  earning rewards until the admin reissues `set_validator_public_key` with a working key. A
  `Restake failed; …Admin recovery required` log line is emitted on this path.
- The contract performs no key-ownership check, so a typo is the admin's responsibility — but the recovery above
  ensures it doesn't permanently brick the contract; it just halts rewards until corrected.

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
  fresh stake. Use sparingly and revoke recovery keys when they are no longer needed.
- A valid full-access key of the contract account can revoke keys with a native `DeleteKey` transaction;
  no contract method or code upgrade is required.
- Requires 1 yoctoNEAR attached for full-access-key confirmation.

### `force_release_lock` — clear a stuck in-flight withdrawal

```bash
near contract call-function as-transaction <CONTRACT_ID> force_release_lock \
  json-args '{ "hash": [/* 32 bytes */] }' \
  prepaid-gas '20 Tgas' \
  attached-deposit '1 yoctoNEAR' \
  sign-as admin.near \
  network-config mainnet
```

- Unlocks the in-flight (locked) tranche queued under `hash`, leaving any other tranches untouched.
- Returns `true` if a locked tranche was found and released, `false` otherwise.
- Intended for recovery when settlement fails or an upgrade leaves an in-flight tranche locked. Before
  releasing it, verify that the delivery did not succeed. Releasing a successfully paid but unsettled claim
  permits a second payout; reconcile such a claim before any unlock or retry.
- Requires 1 yoctoNEAR attached for full-access-key confirmation.

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

**Holder-count upgrades.** Holder statistics change only when an account crosses between zero and nonzero LST balance. This prevents further counter corruption; it does not reconstruct an already incorrect historical count.

**Service account registration.** The pool and treasury LST accounts cannot unregister, even when their LST balance is zero. Reward fee minting and escrow settlement require these accounts to remain registered.

**Uncertain withdrawal settlement.** The claim stays locked when its settlement callback fails or returns an invalid result. Before force-releasing such a lock, inspect delivery receipts and reconcile any completed payment to prevent a second payout.

**wNEAR transfer deposits.** The NEAR balance tracker includes the one-yocto deposit spent on each external FT transfer, including attempts that send only residual wNEAR. Self-withdrawals spend no FT transfer deposit. The callback keeps its original argument list and JSON field names; new calls include the transfer deposit in the cost carried by `amount_to_near_deposit`, with storage accounted for separately.

**Withdrawal gas safety.** Before locking a claim, the contract checks the remaining gas against the delivery budget, a 15 TGas settlement callback and execution overhead. There is no separate cleanup callback. Clients and bots that call `withdraw` with less than the required gas (see [Withdrawing after cooldown](#withdrawing-after-cooldown)) must raise it; 120 TGas covers the default variants.

**FT resolver gas.** LST transfer resolvers and unstake settlement callbacks reserve 15 TGas. Regression tests exercise full refunds, long account names, a crowded storage trie and a 64 KiB receiver result.

**Concurrent wNEAR stakes.** A wNEAR deposit is booked in the NEAR balance tracker as soon as `ft_on_transfer`
accepts it, before wNEAR delivers the unwrapped NEAR. A failed unwrap or a refund removes the booking. Outgoing
payments are removed only after they settle. While transfers are in flight, the tracker can therefore run ahead of
the account balance but never behind it, so unwrapped NEAR from another in-flight stake is never treated as a
validator reward. Reward sync recognizes only `balance − tracker` when it is positive. When in-flight transfers
leave the balance below the tracker, implicit reward sync is postponed with a `Reward sync postponed` log without
itself failing the stake/unstake operation. A public `ping` that encounters this deficit fails instead, allowing the
pinger daemon to retry. A later sync recognizes the remaining reward once the pending transfers settle.

Pause new operations and drain outstanding promise chains before upgrading: a wNEAR stake accepted by the previous
code was not booked on acceptance. Callback argument lists and the Borsh state layout are unchanged.
