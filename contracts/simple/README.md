# Simple MandateRegistry

`contracts/simple/mandate-registry` is the simple REAPP mandate contract used
for the T2 / Milestone 2 deliverables and the first successful source-verified
submission.

It is REAPP's minimal enforcement layer: a user signs a mandate, the contract
stores it, and funds can move only through `execute_payment`, which validates
and consumes the mandate atomically before transferring. The SDK is untrusted;
this contract is the source of truth.

Public methods:

- `register_mandate` - store a user-signed mandate.
- `validate_mandate` - read-only preflight; would a spend be permitted right now?
- `execute_payment` - the only money path; atomic validate, consume, and transfer.
- `revoke_mandate` - user withdraws consent.
- `get_mandate` - read the stored mandate.

Built with `soroban-sdk` v22 for the `wasm32v1-none` target.

## Deployed contract

The simple MandateRegistry is live on **Stellar testnet**:

| | |
|---|---|
| Contract id | [`CB4KOTLGMM5JEPFPU6QBJLADIBP3RSGUX44FOYTFRICNXKKFPYIW7ZOA`](https://stellar.expert/explorer/testnet/contract/CB4KOTLGMM5JEPFPU6QBJLADIBP3RSGUX44FOYTFRICNXKKFPYIW7ZOA) |
| Network | Stellar testnet |
| WASM hash | `4eb1b9430bd4a978348e7efc283a0bf599df048216a43b582921c17daed8c69e` |
| Deployed | 2026-06-19, source-verified on StellarExpert |
| Source anchor | Tag `v0.1.0` |
| Release artifact | `release-artifact/mandate-registry_v0.0.0.wasm` |

Confirm the deployed bytecode matches this source:

```
stellar contract fetch --id CB4KOTLGMM5JEPFPU6QBJLADIBP3RSGUX44FOYTFRICNXKKFPYIW7ZOA --network testnet --out-file onchain.wasm
shasum -a 256 onchain.wasm
# 4eb1b9430bd4a978348e7efc283a0bf599df048216a43b582921c17daed8c69e
```

## Source verification

The source in this folder was restored from the verified `v0.1.0` contract
source. The source-verification anchor remains the historical tag and matching
release artifact, so the verified contract stays tied to the bytecode that was
actually deployed.

Future simple-contract verification releases should build from this folder:

```
cd contracts/simple/mandate-registry
cargo test
stellar contract build
```

Use `simple-v*` tags for future simple-contract release builds.
