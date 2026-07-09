# reapp-protocol-contracts

Source code for REAPP's on-chain contracts, published so anyone can verify that
the bytecode deployed on Stellar matches this source.

## Contract folders

| Folder | Purpose | Verified testnet contract |
|---|---|---|
| [`contracts/simple`](contracts/simple) | Simple mandate contract used for the first successful source-verified deployment. | [`CB4KOTLGMM5JEPFPU6QBJLADIBP3RSGUX44FOYTFRICNXKKFPYIW7ZOA`](https://stellar.expert/explorer/testnet/contract/CB4KOTLGMM5JEPFPU6QBJLADIBP3RSGUX44FOYTFRICNXKKFPYIW7ZOA) |
| [`contracts/composites`](contracts/composites) | Composite mandate contract with clearing pools. This is the main forward-looking contract folder. | [`CBALARHTO5D7JLWHZ5KST4QNIRC64JI5H3DQDHMIUBSRLLOVS6FCWOQX`](https://stellar.expert/explorer/testnet/contract/CBALARHTO5D7JLWHZ5KST4QNIRC64JI5H3DQDHMIUBSRLLOVS6FCWOQX) |

Both contracts keep the same crate name, `mandate-registry`, because they are two
verified versions of the same enforcement layer. Switch by entering the folder
for the contract you want to build or test.

## Build and test locally

Simple mandate contract:

```
cd contracts/simple/mandate-registry
cargo test
stellar contract build
```

Composite mandate contract:

```
cd contracts/composites/mandate-registry
cargo test
stellar contract build
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

The historical tags remain the source-verification anchors. The folder split on
`main` is for clarity and future work; it does not change the already verified
source artifacts.

## Protocol, SDK, and proof

This repo is just the enforcement contract. The full protocol, SDK, x402
round-trip, reference apps, security gatechecks, and clause-by-clause on-chain
proof live in
[`reapp-protocol/reapp-protocol`](https://github.com/reapp-protocol/reapp-protocol).
