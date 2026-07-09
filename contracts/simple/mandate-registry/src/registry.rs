//! Mandate lifecycle: register / revoke. Depends on `storage`, `events`,
//! `mandate`, `error` — never on `payment`.
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

use soroban_sdk::{Address, BytesN, Env};

use crate::error::Error;
use crate::mandate::{Mandate, Status};
use crate::{events, storage};

/// Store a user-signed mandate. The caller supplies only the AUTHORIZED
/// parameters; the contract initializes `spent=0, seq=0, status=Active` so a
/// caller can never seed a tampered balance/status. Authorized by the user.
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
    };
    storage::set_mandate(env, &vc_hash, &mandate);
    events::mandate_registered(env, &vc_hash, &user);
    Ok(vc_hash)
}

/// Mark a mandate Revoked — the user withdraws consent. Authorized by the user.
pub fn revoke_mandate(env: &Env, mandate_id: BytesN<32>) -> Result<(), Error> {
    let mut mandate = storage::get_mandate(env, mandate_id.clone())?;
    mandate.user.require_auth();
    mandate.status = Status::Revoked;
    storage::set_mandate(env, &mandate_id, &mandate);
    events::mandate_revoked(env, &mandate_id);
    Ok(())
}
