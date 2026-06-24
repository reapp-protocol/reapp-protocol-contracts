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

## Deployed contract

The canonical MandateRegistry is live on **Stellar testnet**:

| | |
|---|---|
| Contract id | [`CB4KOTLGMM5JEPFPU6QBJLADIBP3RSGUX44FOYTFRICNXKKFPYIW7ZOA`](https://stellar.expert/explorer/testnet/contract/CB4KOTLGMM5JEPFPU6QBJLADIBP3RSGUX44FOYTFRICNXKKFPYIW7ZOA) |
| Network | Stellar testnet |
| WASM hash | `4eb1b943…d8c69e` |
| Deployed | 2026-06-19, source-verified on StellarExpert against this repo |

Confirm the deployed bytecode matches this source:

```
stellar contract fetch --id CB4KOTLGMM5JEPFPU6QBJLADIBP3RSGUX44FOYTFRICNXKKFPYIW7ZOA --network testnet --out-file onchain.wasm
shasum -a 256 onchain.wasm   # 4eb1b943…d8c69e
```

Mainnet is future work.

## Protocol, SDK, and proof

This repo is just the enforcement contract. The full protocol — the
`@reapp-sdk/core` and `@reapp-sdk/stellar` packages, the x402 round-trip, the
reference apps, the security audits, and the clause-by-clause on-chain proof — lives
in [`reapp-protocol/reapp-protocol`](https://github.com/reapp-protocol/reapp-protocol):

- [The contract, end to end](https://github.com/reapp-protocol/reapp-protocol/blob/main/docs/mandate-registry-contract.md) — every method, on-chain activity, and deployment history.
- [The SDK on npm](https://github.com/reapp-protocol/reapp-protocol/blob/main/docs/reapp-sdk-npm.md) — the under-10-line payment flow.
- [The x402 round-trip](https://github.com/reapp-protocol/reapp-protocol/blob/main/docs/x402-roundtrip.md) — pay-per-resource over HTTP 402.
- [Security audits](https://github.com/reapp-protocol/reapp-protocol/tree/main/security) — contract, SDK, and x402 adversarial reviews.

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
