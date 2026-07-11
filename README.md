# reapp-protocol-contracts

Source code for REAPP's on-chain contracts, published so anyone can verify that
the bytecode deployed on Stellar matches this source.

## Contract folders

| Folder | Current source | Historical verified testnet contract |
|---|---|---|
| [`contracts/simple`](contracts/simple) | `0.2.0`: SDK reference contract with admin pause and same-address upgrades. | [`CB4KOTLGMM5JEPFPU6QBJLADIBP3RSGUX44FOYTFRICNXKKFPYIW7ZOA`](https://stellar.expert/explorer/testnet/contract/CB4KOTLGMM5JEPFPU6QBJLADIBP3RSGUX44FOYTFRICNXKKFPYIW7ZOA), immutable `v0.1.0` |
| [`contracts/composites`](contracts/composites) | `0.3.0`: clearing pools with the same admin and upgrade foundation. | [`CBALARHTO5D7JLWHZ5KST4QNIRC64JI5H3DQDHMIUBSRLLOVS6FCWOQX`](https://stellar.expert/explorer/testnet/contract/CBALARHTO5D7JLWHZ5KST4QNIRC64JI5H3DQDHMIUBSRLLOVS6FCWOQX), immutable `v0.2.0` |

Both contracts keep the crate name `mandate-registry`, but their package
versions and release tags are distinct. The historical tags remain the source
anchors for the existing deployments; current `main` contains additive release
candidates and does not claim their historical WASM hashes.

## Shared Upgrade Controls

Both current contracts add the same operational surface without changing their
existing mandate or pool encodings:

| Addition | Type or signature | Behavior |
|---|---|---|
| `Admin` | instance `Address` | Set by the constructor; authorizes pause, unpause, rotation, and upgrades. |
| `Paused` | instance `bool` | Starts `false`; when `true`, money-moving entry points return `Paused = 10` before changing state. |
| `__constructor` | `(admin: Address)` | Establishes the initial admin and active state atomically at deployment. |
| `get_admin` | `() -> Address` | Returns the current operational authority. |
| `set_admin` | `(new_admin: Address)` | Requires the current admin and transfers future control. |
| `pause` / `unpause` | `() -> ()` | Require the current admin and are idempotent. |
| `is_paused` | `() -> bool` | Exposes the emergency-stop state without authorization. |
| `upgrade` | `(new_wasm_hash: BytesN<32>)` | Requires the current admin and replaces WASM while preserving the contract ID and storage. |

The simple contract pauses `execute_payment`. The composite contract pauses
solo payment and firing pool capture, while non-firing abort, revocation,
registration, validation, reads, commitment, eviction, and simulation remain
available.

## Build and test locally

Run the same gate used by CI and tagged releases:

```
./scripts/gatecheck-contracts.sh
```

Or run one contract directly:

Simple mandate contract:

```
cd contracts/simple/mandate-registry
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --target wasm32v1-none --release
```

Composite mandate contract:

```
cd contracts/composites/mandate-registry
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --target wasm32v1-none --release
```

## Source verification

The deployed contracts were built by the
[StellarExpert soroban-build-workflow](https://github.com/stellar-expert/soroban-build-workflow)
from tagged release artifacts.

- Simple mandate: tag `v0.1.0`, release artifact
  `release-artifact/mandate-registry_v0.0.0.wasm`, hash
  `4eb1b9430bd4a978348e7efc283a0bf599df048216a43b582921c17daed8c69e`.
- Composites: tag `v0.2.0`, release artifact
  `release-artifact/mandate-registry_v0.2.0.wasm`, hash
  `6333c20b490a570ed7b1c8cbfbf382da00ee8a0d1e4ef1ba013d02fa1cf16f44`.

The historical tags remain the source-verification anchors. New deployments
must use the exact WASM downloaded from the tagged GitHub release produced by
the StellarExpert workflow; locally rebuilding and deploying a lookalike can
produce a different hash and break automatic source validation. Record the
artifact hash, build attestation, commit, tag, deployment transaction, on-chain
executable hash, and StellarExpert verification URL for every release.

## Protocol, SDK, and proof

This repo is just the enforcement contract. The full protocol, SDK, x402
round-trip, reference apps, security gatechecks, and clause-by-clause on-chain
proof live in
[`reapp-protocol/reapp-protocol`](https://github.com/reapp-protocol/reapp-protocol).
