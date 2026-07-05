#![no_std]
// register_mandate/register_pool take 9-10 authorized parameters by design
// (each is a signed term, not a tunable); the lint also fires inside
// soroban's #[contractimpl] expansion where a per-fn allow cannot reach.
#![allow(clippy::too_many_arguments)]
//! MandateRegistry — REAPP's on-chain enforcement layer.
//!
//! The contract is the entire protocol and is small by design: a small
//! interface is easy to review. Money moves only through `execute_payment` (solo)
//! and `clear_pool` (composite capture), each of which validates-and-consumes
//! atomically before transferring. The SDK is untrusted; this contract is the
//! source of truth.
//!
//! Module responsibilities (dependencies flow ONE way, no cycles):
//!
//!   lib  →  {registry, payment, pool}  →  storage  →  {mandate, pooltypes, error}
//!                    └────────┴──────────→  events  (leaf; anyone may emit)
//!   pool →  clearing → {mandate, pooltypes}   (pure; no storage, no env I/O)
//!
//!  - `lib`       — contract entry points only: thin dispatch, no logic.
//!  - `mandate`   — the `Mandate` type + pure schedule helpers (demand curve).
//!  - `pooltypes` — `ClearingPool` + composite pool types (pure data).
//!  - `storage`   — `DataKey` + all get/set/TTL (the ONLY module touching env.storage).
//!  - `registry`  — register / revoke (allowance funding model).
//!  - `payment`   — validate_mandate + execute_payment + the token transfer.
//!  - `clearing`  — the pure clearing function (the composite trust core).
//!  - `pool`      — pool lifecycle: register / commit / evict / clear / simulate.
//!  - `error`     — typed errors.
//!  - `events`    — emitted events.

#[cfg(test)]
extern crate std;

mod clearing;
mod error;
mod events;
mod mandate;
mod payment;
mod pool;
mod pooltypes;
mod registry;
mod storage;

pub use error::Error;
pub use mandate::{Mandate, PoolState, SchedulePoint, Status};
pub use pooltypes::{Allocation, ChildView, ClearOutcome, ClearingKind, ClearingPool, PoolStatus};

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, Vec};

#[contract]
pub struct MandateRegistry;

#[contractimpl]
impl MandateRegistry {
    /// Store a user-signed mandate from its authorized parameters. The contract
    /// sets `spent=0, seq=0, status=Active` itself. Authorized by `user`.
    /// Returns the mandate id (= `vc_hash`, the storage key).
    ///
    /// `pool_id = None` + empty `price_schedule` == a standalone mandate
    /// (the pre-composite behavior, unchanged). `pool_id = Some(id)` binds the
    /// mandate to a clearing pool; the schedule is the user's authorization
    /// for the pool path (see `registry`).
    #[allow(clippy::too_many_arguments)]
    pub fn register_mandate(
        env: Env,
        user: Address,
        agent: Address,
        merchant: Address,
        asset: Address,
        max_amount: i128,
        expiry: u64,
        vc_hash: BytesN<32>,
        pool_id: Option<BytesN<32>>,
        price_schedule: Vec<SchedulePoint>,
    ) -> Result<BytesN<32>, Error> {
        registry::register_mandate(
            &env,
            user,
            agent,
            merchant,
            asset,
            max_amount,
            expiry,
            vc_hash,
            pool_id,
            price_schedule,
        )
    }

    /// Read-only preflight — would this spend be permitted right now? Mutates
    /// nothing and requires no auth; the authoritative consume happens only in
    /// `execute_payment`. (It is a dry-run; it consumes nothing.) Reflects
    /// pool state too: a Committed/Captured child preflights `MandatePooled`,
    /// exactly what `execute_payment` would do.
    pub fn validate_mandate(
        env: Env,
        mandate_id: BytesN<32>,
        amount: i128,
        merchant: Address,
    ) -> Result<(), Error> {
        payment::validate_mandate(&env, mandate_id, amount, merchant)
    }

    /// The solo money path. Atomic: require_auth(agent) → replay guard
    /// (`expected_seq` == current `seq`, else `BadSequence`) → re-validate →
    /// advance spent+seq → SEP-41 transfer_from(user → merchant). Reverts on any
    /// failure. `expected_seq` is the mandate's current sequence (read from
    /// `get_mandate`), preventing duplicate/out-of-order consumption.
    pub fn execute_payment(
        env: Env,
        mandate_id: BytesN<32>,
        amount: i128,
        expected_seq: u32,
    ) -> Result<(), Error> {
        payment::execute_payment(&env, mandate_id, amount, expected_seq)
    }

    /// User withdraws consent; marks the mandate Revoked. Authorized by the
    /// user. Also frees the pool slot of a Committed child (its one
    /// pre-deadline exit).
    pub fn revoke_mandate(env: Env, mandate_id: BytesN<32>) -> Result<(), Error> {
        registry::revoke_mandate(&env, mandate_id)
    }

    /// Read-only accessor for the stored mandate (inspection / preflight).
    pub fn get_mandate(env: Env, mandate_id: BytesN<32>) -> Result<Mandate, Error> {
        storage::get_mandate(&env, mandate_id)
    }

    // ── composite mandates (clearing pools) ─────────────────────────────────

    /// Register a clearing pool. The returned pool id is derived from the
    /// terms (sha256 of their XDR), so the id commits to the terms.
    /// Authorized by `originator` — the last special signature the pool ever
    /// requires: everything after this is permissionless and deterministic.
    #[allow(clippy::too_many_arguments)]
    pub fn register_pool(
        env: Env,
        originator: Address,
        merchant: Address,
        asset: Address,
        kind: ClearingKind,
        threshold_qty: u128,
        threshold_value: u128,
        min_child_value: u128,
        clearing_deadline: u64,
        nonce: BytesN<32>,
    ) -> Result<BytesN<32>, Error> {
        pool::register_pool(
            &env,
            originator,
            merchant,
            asset,
            kind,
            threshold_qty,
            threshold_value,
            min_child_value,
            clearing_deadline,
            nonce,
        )
    }

    /// Link a pooled mandate into its pool as a Committed member.
    /// Permissionless (objective checks only); revocable until the deadline.
    pub fn commit_child(env: Env, mandate_id: BytesN<32>) -> Result<(), Error> {
        pool::commit_child(&env, mandate_id)
    }

    /// Remove an objectively-ineligible member and free its slot.
    /// Permissionless; can never evict a still-eligible member.
    pub fn evict_child(env: Env, pool_id: BytesN<32>, mandate_id: BytesN<32>) -> Result<(), Error> {
        pool::evict_child(&env, pool_id, mandate_id)
    }

    /// Close the deadline auction: capture (all legs in this one transaction)
    /// if the threshold predicate holds within the capture window, else abort
    /// and release every committed child. Callable by anyone, never before
    /// the deadline.
    pub fn clear_pool(env: Env, pool_id: BytesN<32>) -> Result<(), Error> {
        pool::clear_pool(&env, pool_id)
    }

    /// Read-only: the exact outcome `clear_pool` would execute against current
    /// ledger state. Same builder, same clearing function — recompute this to
    /// verify the originator had no discretion over the allocation.
    pub fn simulate_clear(env: Env, pool_id: BytesN<32>) -> Result<ClearOutcome, Error> {
        pool::simulate_clear(&env, pool_id)
    }

    /// Read-only accessor for a stored pool.
    pub fn get_pool(env: Env, pool_id: BytesN<32>) -> Result<ClearingPool, Error> {
        pool::get_pool(&env, pool_id)
    }

    /// Read-only: current member mandate ids (commit order; frozen once the
    /// pool is terminal).
    pub fn get_pool_members(env: Env, pool_id: BytesN<32>) -> Result<Vec<BytesN<32>>, Error> {
        pool::get_pool_members(&env, pool_id)
    }
}

#[cfg(test)]
mod test;

#[cfg(test)]
mod pool_test;

#[cfg(test)]
mod reentry_probe;
