#![no_std]
//! MandateRegistry — REAPP's on-chain enforcement layer.
//!
//! The contract is the entire protocol and is small by design: a small
//! interface is auditable. Money moves only through `execute_payment`, which
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

mod error;
mod events;
mod mandate;
mod payment;
mod registry;
mod storage;

pub use error::Error;
pub use mandate::{Mandate, Status};

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};

#[contract]
pub struct MandateRegistry;

#[contractimpl]
impl MandateRegistry {
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

    /// Read-only accessor for the stored mandate (audit / preflight).
    pub fn get_mandate(env: Env, mandate_id: BytesN<32>) -> Result<Mandate, Error> {
        storage::get_mandate(&env, mandate_id)
    }
}

#[cfg(test)]
mod test;

#[cfg(test)]
mod reentry_probe;
