#![no_std]
//! MandateRegistry — REAPP's on-chain enforcement layer.
//!
//! The contract is the entire protocol and is small by design: a small
//! interface is reviewable. Money moves only through `execute_payment`, which
//! validates-and-consumes the mandate atomically before transferring. The SDK
//! is untrusted; this contract is the source of truth.
//!
//! Module responsibilities (dependencies flow ONE way, no cycles):
//!
//!   lib  →  {registry, payment}  →  storage  →  mandate / error
//!                  └────────────→  events  (leaf; anyone may emit)
//!
//!  - `lib`      — contract entry points only: thin dispatch, no logic.
//!  - `mandate`  — the `Mandate` type (pure data).
//!  - `storage`  — `DataKey` + all get/set/TTL (the ONLY module touching env.storage).
//!  - `registry` — register / revoke (allowance funding model).
//!  - `payment`  — validate_mandate + execute_payment + the token transfer.
//!  - `error`    — typed errors.
//!  - `events`   — emitted events.

mod admin;
mod error;
mod events;
mod mandate;
mod payment;
mod registry;
mod storage;

pub use admin::{PendingUpgrade, UPGRADE_DELAY_SECONDS};
pub use error::Error;
pub use mandate::{Mandate, Status};

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};

#[contract]
pub struct MandateRegistry;

#[contractimpl]
impl MandateRegistry {
    /// Atomically establishes the initial administrator during deployment.
    /// Constructors run only once; WASM upgrades do not run them again.
    pub fn __constructor(env: Env, admin: Address) {
        storage::set_admin(&env, &admin);
        storage::set_paused(&env, false);
    }

    /// Current operational administrator.
    pub fn get_admin(env: Env) -> Address {
        admin::get_admin(&env)
    }

    /// Rotate operational authority. Authorized by the current administrator.
    pub fn set_admin(env: Env, new_admin: Address) {
        admin::set_admin(&env, new_admin)
    }

    /// Emergency stop for the sole money-moving path.
    pub fn pause(env: Env) {
        admin::pause(&env)
    }

    /// Restore the money-moving path after an emergency stop.
    pub fn unpause(env: Env) {
        admin::unpause(&env)
    }

    /// Read the emergency-stop state without authorization.
    pub fn is_paused(env: Env) -> bool {
        admin::is_paused(&env)
    }

    /// Schedule a same-address WASM upgrade after the fixed one-hour delay.
    pub fn schedule_upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<u64, Error> {
        admin::schedule_upgrade(&env, new_wasm_hash)
    }

    /// Cancel the currently scheduled upgrade.
    pub fn cancel_upgrade(env: Env) -> Result<(), Error> {
        admin::cancel_upgrade(&env)
    }

    /// Execute the scheduled upgrade after the delay while the contract is paused.
    pub fn execute_upgrade(env: Env) -> Result<(), Error> {
        admin::execute_upgrade(&env)
    }

    /// Read the pending upgrade, including hash and earliest execution time.
    pub fn get_pending_upgrade(env: Env) -> Option<PendingUpgrade> {
        storage::get_pending_upgrade(&env)
    }

    /// Fixed timelock duration in seconds.
    pub fn get_upgrade_delay(_env: Env) -> u64 {
        UPGRADE_DELAY_SECONDS
    }

    /// Temporary read-only marker for the same-address upgrade validation.
    /// The cleanup release removes this method after the test is complete.
    pub fn upgrade_test_version(_env: Env) -> u32 {
        1
    }

    /// Store a user-signed mandate from its authorized parameters. The contract
    /// sets `spent=0, seq=0, status=Active` itself. Authorized by `user`.
    /// Returns the mandate id (= `vc_hash`, the storage key).
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
    ) -> Result<BytesN<32>, Error> {
        registry::register_mandate(
            &env, user, agent, merchant, asset, max_amount, expiry, vc_hash,
        )
    }

    /// Read-only preflight — would this spend be permitted right now? Mutates
    /// nothing and requires no auth; the authoritative consume happens only in
    /// `execute_payment`. (It is a dry-run; it consumes nothing.)
    pub fn validate_mandate(
        env: Env,
        mandate_id: BytesN<32>,
        amount: i128,
        merchant: Address,
    ) -> Result<(), Error> {
        payment::validate_mandate(&env, mandate_id, amount, merchant)
    }

    /// The only money path. Atomic: require_auth(agent) → replay guard
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

    /// User withdraws consent; marks the mandate Revoked. Authorized by the user.
    pub fn revoke_mandate(env: Env, mandate_id: BytesN<32>) -> Result<(), Error> {
        registry::revoke_mandate(&env, mandate_id)
    }

    /// Read-only accessor for the stored mandate (inspection / preflight).
    pub fn get_mandate(env: Env, mandate_id: BytesN<32>) -> Result<Mandate, Error> {
        storage::get_mandate(&env, mandate_id)
    }
}

#[cfg(test)]
mod test;

#[cfg(test)]
mod reentry_probe;
