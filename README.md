# MINU reward program

Source for inspecting the on-chain MINU mining and SOL reward logic. This package contains the Rust program, a JavaScript reference client, local tests and deployment verification tooling.

This source is published for inspection, local testing and security review under the MINU Source-Available License in [LICENSE](LICENSE). Production deployment and commercial use require separate written permission. Third-party licenses and previously granted rights remain applicable.

## Mining rules

| Mine | MINU lock | Reward weight |
| --- | --- | --- |
| Bronze | 4 hours | 1× |
| Silver | 8 hours | 1.25× |
| Gold | 16 hours | 1.5× |

All mines share one SOL reserve. A position's weight is its deposited MINU multiplied by its mine's weight. Its share of each released amount is its weight divided by the total weight of active positions.

The flow engine budgets 1% of the unallocated reserve per active minute and releases it over 60 blockchain seconds. Newly observed funding joins future release calculations. New positions participate going forward; they cannot take rewards already earned by existing positions. Joining changes future shares rather than past earnings.

Earned whole-lamport SOL is immediately claimable. Only MINU has a time lock. A matured position keeps participating until its owner withdraws. Withdrawal pays outstanding whole-lamport rewards, returns MINU and removes its weight. Fractional rounding dust can remain below one lamport.

The reserve is distinct from earned but unpaid rewards. Funds already earned must remain backed until paid. When no positions participate, reserve decay pauses. Rewards are variable and depend on funding and active weight; there is no guaranteed yield or minimum reward.

The contract calculates elapsed flow on transactions. Client projections display estimated current amounts between transactions. A collector can synchronize accounting, but public reads alone do not mutate blockchain accounts.

## Inspect and verify

- `programs/sol_fee_staking/src/lib.rs`: custody, timing, accounting and payout rules.
- `client/reward-flow.mjs`: integer-only projection of the flow engine.
- `client/staking.mjs`: account decoding and transaction construction, without website design.
- `tests/`: local correctness checks using synthetic accounts and funds.
- `releases/manifest.json`: current devnet binary identity.
- `BUILD.md`: build and verification instructions.
- `SECURITY.md`: trust assumptions and review boundaries.
- `DEPENDENCY-LICENSES.json` and `third-party-licenses/`: dependency licensing inventory and notices.

This release describes a devnet deployment. It does not announce a mainnet deployment. Source visibility and a matching binary are not an independent security audit.

Website: https://minerinu.app

Documentation: https://docs.minerinu.app
