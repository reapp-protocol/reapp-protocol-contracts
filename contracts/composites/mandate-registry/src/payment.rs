//! The money path. The core invariant lives here: validating and consuming the
//! mandate is the SAME atomic operation as moving the funds, so there is no
//! window where validation and settlement disagree. Depends on `storage`,
//! `events`, `mandate`, `error` — never on `registry`.

use soroban_sdk::token::TokenClient;
use soroban_sdk::{Address, BytesN, Env};

use crate::error::Error;
use crate::mandate::{Mandate, PoolState, Status};
use crate::{events, storage};

/// The single source of enforcement truth. Every check the protocol makes lives
/// here, and `execute_payment` re-runs it against stored state on every spend —
/// the SDK is never trusted to have validated.
fn check(env: &Env, m: &Mandate, amount: i128, merchant: &Address) -> Result<(), Error> {
    if amount <= 0 {
        return Err(Error::InvalidAmount);
    }
    match m.status {
        Status::Revoked => return Err(Error::MandateRevoked),
        Status::Exhausted => return Err(Error::BudgetExceeded),
        Status::Active => {}
    }
    // A Committed child's allowance is spoken for by its pool; a Captured
    // child's remaining budget stays locked to the pool (simplest safe rule).
    // Unlinked/Released pooled children spend solo within their own limits.
    match m.pool_state {
        PoolState::Committed | PoolState::Captured => return Err(Error::MandatePooled),
        PoolState::Unlinked | PoolState::Released => {}
    }
    // Expired at-or-after the expiry instant. Symmetric with register_mandate,
    // which requires expiry > now — a mandate is valid strictly while now < expiry.
    if env.ledger().timestamp() >= m.expiry {
        return Err(Error::MandateExpired);
    }
    if *merchant != m.merchant {
        return Err(Error::MerchantOutOfScope);
    }
    if m.spent + amount > m.max_amount {
        return Err(Error::BudgetExceeded);
    }
    Ok(())
}

/// Read-only preflight (dry run): would a payment of `amount` to `merchant` be
/// permitted right now? Mutates nothing, requires no auth — the SDK calls this
/// for a clean typed error before paying. The authoritative consume + transfer
/// happens only in `execute_payment`.
pub fn validate_mandate(
    env: &Env,
    mandate_id: BytesN<32>,
    amount: i128,
    merchant: Address,
) -> Result<(), Error> {
    let mandate = storage::get_mandate(env, mandate_id)?;
    check(env, &mandate, amount, &merchant)
}

/// The only code path that moves the user's funds. Atomically, in one tx:
///   1. `require_auth(mandate.agent)` — caller must be the bound agent.
///   2. replay guard: `expected_seq` must equal the mandate's current `seq`
///      (mandate-layer protection on top of Soroban's transport nonce). A
///      duplicate or out-of-order spend fails with `BadSequence`.
///   3. re-validate scope / budget / expiry / status against stored state.
///   4. advance `spent` + `seq` (flip to `Exhausted` when the budget is used up).
///   5. SEP-41 `transfer_from(contract spender, user → merchant, amount)`.
///
/// Any failure reverts the whole transaction — no partial spend.
pub fn execute_payment(
    env: &Env,
    mandate_id: BytesN<32>,
    amount: i128,
    expected_seq: u32,
) -> Result<(), Error> {
    let mut mandate = storage::get_mandate(env, mandate_id.clone())?;
    mandate.agent.require_auth();

    if expected_seq != mandate.seq {
        return Err(Error::BadSequence);
    }

    let merchant = mandate.merchant.clone();
    check(env, &mandate, amount, &merchant)?;

    mandate.spent += amount;
    mandate.seq += 1;
    if mandate.spent == mandate.max_amount {
        mandate.status = Status::Exhausted;
    }
    storage::set_mandate(env, &mandate_id, &mandate);

    // The contract is the allowance holder (spender); the user approved it.
    let token = TokenClient::new(env, &mandate.asset);
    token.transfer_from(
        &env.current_contract_address(),
        &mandate.user,
        &merchant,
        &amount,
    );

    events::payment_executed(env, &mandate_id, &merchant, amount);
    Ok(())
}
