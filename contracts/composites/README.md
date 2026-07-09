# Composite MandateRegistry

`contracts/composites/mandate-registry` is REAPP's composite mandate contract
with clearing pools. This is the main forward-looking contract folder.

It extends the simple mandate path with deterministic group clearing: money
moves only through `execute_payment` for standalone mandates or `clear_pool` for
composite capture. Each path validates and consumes atomically before
transferring. The SDK is untrusted; this contract is the source of truth.

Public methods:

- `register_mandate` - store a user-signed mandate; optionally bind it to a
  clearing pool with a price schedule.
- `validate_mandate` - read-only preflight; would a spend be permitted right now?
- `execute_payment` - the single-mandate money path; atomic validate, consume,
  and transfer.
- `revoke_mandate` - user withdraws consent; frees a committed pool slot.
- `get_mandate` - read the stored mandate.

Composite mandates:

- `register_pool` - put a vendor minimum and hard close time on-chain; the pool
  id is the hash of those exact terms.
- `commit_child` - link a pooled mandate as a committed member.
- `evict_child` - remove an objectively ineligible member.
- `clear_pool` - close the auction: settle every leg in one transaction, or
  abort so nobody pays.
- `simulate_clear` - read-only view of the exact outcome capture would execute.
- `get_pool`, `get_pool_members` - read pool state.

Built with `soroban-sdk` v22 for the `wasm32v1-none` target.

## Deployed contract

The composite MandateRegistry is live on **Stellar testnet**:

| | |
|---|---|
| Contract id | [`CBALARHTO5D7JLWHZ5KST4QNIRC64JI5H3DQDHMIUBSRLLOVS6FCWOQX`](https://stellar.expert/explorer/testnet/contract/CBALARHTO5D7JLWHZ5KST4QNIRC64JI5H3DQDHMIUBSRLLOVS6FCWOQX) |
| Network | Stellar testnet |
| WASM hash | `6333c20b490a570ed7b1c8cbfbf382da00ee8a0d1e4ef1ba013d02fa1cf16f44` |
| Deployed | 2026-07-05 from the v0.2.0 release artifact, source-verified on StellarExpert |
| Source anchor | Tag `v0.2.0` |
| Release artifact | `release-artifact/mandate-registry_v0.2.0.wasm` |

Confirm the deployed bytecode matches this source:

```
stellar contract fetch --id CBALARHTO5D7JLWHZ5KST4QNIRC64JI5H3DQDHMIUBSRLLOVS6FCWOQX --network testnet --out-file onchain.wasm
shasum -a 256 onchain.wasm
# 6333c20b490a570ed7b1c8cbfbf382da00ee8a0d1e4ef1ba013d02fa1cf16f44
```

## Source verification

The source-verification anchor remains the historical `v0.2.0` tag and matching
release artifact. Moving the current source into this folder on `main` keeps the
verified code readable beside the simple contract without changing the deployed
artifact.

Build and test from this folder:

```
cd contracts/composites/mandate-registry
cargo test
stellar contract build
```

Use `v*` tags, except `v0.1.*`, or `composites-v*` tags for future
composite-contract release builds.
