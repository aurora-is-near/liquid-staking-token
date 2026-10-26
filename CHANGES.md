# Changes

## 0.1.1 — 2026-10-16

### Added

- Dedicated `CodeStager` and `CodeDeployer` roles for staging and deploying contract code. `Admin` retains both
  permissions and remains responsible for initializing and updating the staging duration.
- Unit and sandbox regression coverage for concurrent stakes, withdrawal settlement, gas limits, holder counts,
  and reward accounting, including batched storage-deposit refunds.

### Fixed

- Book accepted wNEAR deposits before unwrapping completes, and reverse the booking on failed unwraps or refunds,
  so concurrent deposits cannot be recognized as validator rewards.
- Postpone implicit reward synchronization while in-flight transfers leave the account balance below the tracked
  total. Public `ping` reports this condition as a failure so the pinger can retry; it returns zero when no rewards
  are available or the current epoch has already been synced.
- Settle withdrawals and release their locks in the same callback. Failed or malformed settlement keeps the claim
  locked to prevent duplicate payouts; a known failed plain wNEAR transfer remains retryable.
- Include the one-yoctoNEAR deposit for external FT transfers in withdrawal balance accounting, including retries
  that send only residual wNEAR.
- Count LST holders only when their balances cross between zero and nonzero, including transfers, refunds, and
  forced unregistration. Prevent an already undercounted statistic from blocking settlement.
- Prevent the pool and treasury LST accounts from unregistering, including when their token balance is zero.
- Reserve gas for complete staking, withdrawal, and FT settlement chains, and reject insufficient gas before
  accepting wNEAR for unwrapping or locking withdrawal claims. Limit serialized `StakeMessage` values to 4096 bytes.
