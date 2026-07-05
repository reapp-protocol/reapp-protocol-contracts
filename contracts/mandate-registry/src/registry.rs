//! Mandate lifecycle: register / revoke. Depends on `storage`, `events`,
//! `mandate`, `pooltypes`, `error` — never on `payment` or `pool`.
//!
//! Funding model (§4.3): allowance is PRIMARY — after registering, the user
//! signs SEP-41 `approve(spender = this contract, max_amount)` separately, so
//! no funds are pulled here. `execute_payment` later calls `transfer_from`.
//!
//! Escrow (§4.3 escape hatch): the decided rule is "use the allowance path; if
//! `transfer_from` fails after two genuine attempts, switch to escrow." The
//! allowance path works on live testnet (proven end-to-end), so escrow was
//! never triggered and is intentionally NOT implemented — adding it now would
//! be untriggered dead code. It is the documented contingency, not MVP scope.

use soroban_sdk::{Address, BytesN, Env, Vec};

use crate::error::Error;
use crate::mandate::{self, Mandate, PoolState, SchedulePoint, Status};
use crate::pooltypes::{PoolStatus, CAPTURE_WINDOW_SECS};
use crate::{events, storage};

/// Store a user-signed mandate. The caller supplies only the AUTHORIZED
/// parameters; the contract initializes `spent=0, seq=0, status=Active,
/// pool_state=Unlinked` itself so a caller can never seed a tampered
/// balance/status. Authorized by the user.
///
/// `pool_id = None` (with an empty schedule) is a standalone mandate — exactly
/// the pre-composite behavior. `pool_id = Some(id)` binds the mandate to a
/// clearing pool: the user's signature over these parameters (schedule
/// included) is the ENTIRE authorization the pool path will ever get, so
/// every pool-compatibility rule is enforced here, at signing time.
#[allow(clippy::too_many_arguments)]
pub fn register_mandate(
    env: &Env,
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
    user.require_auth();

    if max_amount <= 0 {
        return Err(Error::InvalidAmount);
    }
    if expiry <= env.ledger().timestamp() {
        return Err(Error::MandateExpired);
    }
    if storage::has_mandate(env, &vc_hash) {
        return Err(Error::AlreadyExists);
    }

    match &pool_id {
        None => {
            if !price_schedule.is_empty() {
                return Err(Error::ScheduleInvalid);
            }
        }
        Some(id) => {
            let pool = storage::get_pool(env, id.clone())?;
            if pool.status != PoolStatus::Open {
                return Err(Error::PoolNotOpen);
            }
            if env.ledger().timestamp() >= pool.clearing_deadline {
                return Err(Error::DeadlinePassed);
            }
            if merchant != pool.merchant {
                return Err(Error::PoolMerchantMismatch);
            }
            if asset != pool.asset {
                return Err(Error::PoolAssetMismatch);
            }
            mandate::validate_schedule(&price_schedule)?;
            let wc = mandate::worst_case(&price_schedule);
            // The signed max_amount stays the hard ceiling (defense in depth):
            // no schedule may authorize a leg the budget cannot cover.
            if wc > max_amount {
                return Err(Error::ScheduleInvalid);
            }
            if (wc as u128) < pool.min_child_value {
                return Err(Error::BelowMinChild);
            }
            // No committed child may expire inside the capture window — this
            // kills the wait-for-expiry timing lever on the deadline auction.
            if expiry <= pool.clearing_deadline + CAPTURE_WINDOW_SECS {
                return Err(Error::ExpiryBeforeDeadline);
            }
        }
    }

    let mandate = Mandate {
        user: user.clone(),
        agent,
        merchant,
        asset,
        max_amount,
        spent: 0,
        expiry,
        seq: 0,
        status: Status::Active,
        vc_hash: vc_hash.clone(),
        pool_id,
        price_schedule,
        pool_state: PoolState::Unlinked,
    };
    storage::set_mandate(env, &vc_hash, &mandate);
    events::mandate_registered(env, &vc_hash, &user);
    Ok(vc_hash)
}

/// Mark a mandate Revoked — the user withdraws consent. Authorized by the
/// user. Never blockable. For a Committed pool member this is the one
/// pre-deadline exit: it also frees the pool slot (the vc_hash itself is
/// spent; continuing solo needs a fresh mandate). A Captured child's purchase
/// is final — revoking blocks nothing retroactively.
pub fn revoke_mandate(env: &Env, mandate_id: BytesN<32>) -> Result<(), Error> {
    let mut mandate = storage::get_mandate(env, mandate_id.clone())?;
    mandate.user.require_auth();
    mandate.status = Status::Revoked;

    if mandate.pool_state == PoolState::Committed {
        if let Some(pool_id) = mandate.pool_id.clone() {
            let mut pool = storage::get_pool(env, pool_id.clone())?;
            if pool.status == PoolStatus::Open {
                let mut members = storage::get_pool_members(env, &pool_id);
                if let Some(index) = members.first_index_of(mandate_id.clone()) {
                    members.remove(index);
                }
                storage::set_pool_members(env, &pool_id, &members);
                pool.member_count = pool.member_count.saturating_sub(1);
                storage::set_pool(env, &pool_id, &pool);
                mandate.pool_state = PoolState::Released;
                let horizon = (pool.clearing_deadline + CAPTURE_WINDOW_SECS)
                    .saturating_sub(env.ledger().timestamp());
                storage::bump_pool_horizon(env, &pool_id, horizon);
                storage::bump_mandate_horizon(env, &mandate_id, horizon);
                events::child_released(env, &pool_id, &mandate_id);
            }
        }
    }

    storage::set_mandate(env, &mandate_id, &mandate);
    events::mandate_revoked(env, &mandate_id);
    Ok(())
}
