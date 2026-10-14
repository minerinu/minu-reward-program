# Security and trust model

MINU is an upgradeable Solana program. The upgrade authority can change the code, including fund-handling rules. Public source does not remove this authority or guarantee its future behavior. Verify the deployed program and its authority independently.

Review the following properties:

- Account ownership, mint identity, token-program identity and signing requirements.
- Checked arithmetic and SOL backing for accrued liabilities.
- Chain-clock enforcement of MINU locks.
- Weighted future participation when miners join or withdraw.
- Payout idempotency and fractional rounding.
- Accepted mint extensions and freeze-authority restrictions.
- Pause of reserve decay when no positions participate.

The SOL vault funds both future reserve and unpaid earned liabilities. Previously earned rewards cannot be redistributed to new participants. The collector's fee routing and infrastructure are separate from this program package; inspecting this package does not independently verify an external fee source or its routing configuration.

The repository does not include private keys, deployment secrets, website assets or collector credentials. Never send anyone a seed phrase or private key. Local tests use generated accounts.

No independent audit is claimed. For a private security report, contact the project through the official website. Do not include credentials or exploit active user funds.

## Compiler diagnostics

The pinned compiler emits stack-frame diagnostics for dependency serialization functions exceeding the SBPF frame limit. Successful linking, a matching deployed binary and passing runtime checks do not independently establish that these functions are unreachable in all program paths. Dependency reachability and these diagnostics remain security-review items before production use.
