# AP2 authorization extension

This folder contains the optional Soroban adapter for AP2 v0.2 open/closed
mandate evidence. It is separate from Simple and Composite so AP2-specific
verifier policy and replay state do not enlarge their base mandate encodings.

The extension accepts short-lived Ed25519-signed, typed authorizations from a
merchant verifier. Raw JSON, JWS, SD-JWT disclosures, certificate chains, and
web trust resolution remain off-chain. The extension receives no token
allowance and never calls a token.

## User routes

| Route | Registry stores as `agent` | Extension method | Registry money path |
|---|---|---|---|
| Simple | extension address | `execute_simple` | unchanged `execute_payment` |
| Released AP2 Composite child | extension address | `execute_composite_solo` | Composite `execute_payment` |
| AP2 Composite pool | extension address | Composite calls `register_pool_participation` and `consume_pool` | `clear_pool_ap2` |

The shopping agent authenticates to the extension for solo capture. Composite
pool participation is verified and fixed before the clearing deadline, so
permissionless capture does not depend on every shopping agent returning.

## What is enforced here

- authorization schema version and Stellar network id;
- enabled verifier key;
- validity window, positive amount, and a maximum authorization lifetime;
- domain-separated authorization id and Ed25519 signature;
- one-time solo capture or one-time pooled participation consumption;
- registry contract authorization for pool registration and consumption; and
- on the solo routes, that the registry's own mandate names this extension as
  its agent and carries the same merchant and asset the verifier signed.

Simple or Composite still enforces the mandate's scope, budget, expiry,
sequence, pool state, allowance, and token transfer. A failure in either
contract reverts both contracts' state.

## What is NOT enforced here

- `PoolCapture.expected_seq` and `PoolCapture.outcome_root` are recorded in the
  `ap2_pool_use` event but not checked. Composite computes both from its own
  canonical clearing outcome and is authoritative for them; the extension
  bounds a pooled leg only by `0 < amount <= max_amount` and by consuming the
  participation exactly once.
- The evidence hashes are opaque 32-byte commitments. Whether they correspond
  to a real, well-formed AP2 chain is the off-chain verifier's judgement, which
  is why the accepted verifier-key policy is a published release artifact.
- The extension has no upgrade and no pause path. Retiring one means disabling
  its verifier keys and deploying a replacement; the registries keep their own
  timelocked upgrade paths.

## Authorization lifetime

Every accepted authorization writes a replay marker whose TTL is derived from
`expires_at`. Soroban clamps a TTL extension at the network's maximum entry
TTL rather than rejecting it, so an unbounded window could leave a marker
expiring before the authorization it guards. `MAX_AUTHORIZATION_LIFETIME_SECS`
(45 days) bounds `expires_at - not_before` so that can't happen, and clears
Composite's 30-day pool horizon with room for the participation authorization
that must outlast the capture window.

## Build and test

```bash
cd contracts/ap2-authorization/mandate-extension
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The tests include byte-exact TypeScript/Rust authorization vectors, replay,
wrong route/network/window cases, downstream rollback, a real unchanged Simple
registry plus SEP-41 token, and separate Simple/CompositeSolo routing.

This extension has not been deployed. Treat addresses, accepted verifier keys,
and trust policy as release configuration that must be published with any
user-facing deployment.
