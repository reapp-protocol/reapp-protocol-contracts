# reapp-protocol-contracts

Source code for REAPP's on-chain contracts, published so anyone can verify that
the bytecode deployed on Stellar matches this source.

## MandateRegistry

`contracts/mandate-registry` is REAPP's enforcement layer. It is the entire
protocol and is small by design: a small interface is auditable. Money moves
only through `execute_payment`, which validates and consumes a mandate atomically
before transferring. The SDK is untrusted; this contract is the source of truth.

Public methods:

- `register_mandate` — store a user-signed mandate.
- `validate_mandate` — read-only preflight; would a spend be permitted right now?
- `execute_payment` — the only money path; atomic validate, consume, and transfer.
- `revoke_mandate` — user withdraws consent.
- `get_mandate` — read the stored mandate.

Built with `soroban-sdk` v22 for the `wasm32v1-none` target.

## Source verification

Releases are built by the
[StellarExpert soroban-build-workflow](https://github.com/stellar-expert/soroban-build-workflow).
On each `v*` tag the workflow compiles the contract, publishes the optimized WASM
as a GitHub release, and reports the binary hash, repository, and commit to
StellarExpert. The contract deployed on Stellar is deployed from that exact
release artifact, so its on-chain hash matches the published source and the
contract page links back here.

## Build and test locally

```
cd contracts/mandate-registry
cargo test
stellar contract build
```
