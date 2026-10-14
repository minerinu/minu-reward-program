# Build and verification

## Local JavaScript checks

Use Node.js 22 and install locked dependencies:

```sh
npm ci --ignore-scripts
npm test
```

Rust unit tests require a compatible Rust toolchain and the locked dependencies:

```sh
cargo test --lib --locked
```

## Program build

The referenced release used Agave cargo-build-sbf 2.3.13 and Solana platform-tools 1.54 (Rust 1.89) targeting SBPF v3. Preserve Cargo.lock, the declared program identity and release compiler settings when comparing binaries. Obtain tools from the official Anza release channels.

Build the locked source with the pinned driver and platform-tools:

```sh
cargo-build-sbf --tools-version v1.54 --arch v3 --manifest-path programs/sol_fee_staking/Cargo.toml -- --locked
```

The output is `target/deploy/sol_fee_staking.so`. If the SDK's optional tool installer stops after successful compilation, finish the same stripping step explicitly:

```sh
/path/to/platform-tools/llvm/bin/llvm-objcopy --strip-all target/sbpfv3-solana-solana/release/sol_fee_staking.so target/deploy/sol_fee_staking.so
```

If CARGO_TARGET_DIR is set, use its value instead of `target` for the unstripped input path. The stripping step does not compile or change the program's logic.

A fresh compilation of this packaged source produced an exact byte match to the referenced devnet binary. See `releases/source-build-verification.json` for toolchain versions, input checksums and the binary hash. The reference is specific to this release and must be rechecked after source or deployment changes.

## Runtime tests

After building, run:

```sh
npm run test:runtime
```

These tests execute the compiled binary in LiteSVM with generated accounts and synthetic funds. They do not submit real transactions or provide deployment credentials.

## Verify the referenced deployment

```sh
npm run verify:devnet
```

The verifier reads the Solana devnet genesis hash, loader-owned program accounts, the deployed bytes and the bundled release hash. It does not sign or send transactions. A match proves that the referenced binary is deployed, not that the binary is safe.

To complete source-to-deployment verification, rebuild the source, independently compare its bytes with the referenced binary, then verify those bytes against the chain. Compilers, target architecture and build environment must match.

The current deployment is upgradeable. Re-run verification after an upgrade and inspect its authority before relying on a prior result.
