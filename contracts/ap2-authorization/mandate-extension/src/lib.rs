#![no_std]
//! AP2 authorization extension.
//!
//! This contract consumes compact merchant-verifier authorizations. It never
//! receives a token allowance and never calls a token. Simple or Composite
//! remains the only contract that can move funds.

#[cfg(test)]
extern crate std;

mod error;
mod registry;
mod storage;
mod types;

pub use error::Error;
pub use registry::{
    CompositeMandate, CompositeRegistry, CompositeRegistryClient, MandatePoolState,
    MandateSchedulePoint, MandateStatus, SimpleMandate, SimpleRegistry, SimpleRegistryClient,
};
pub use types::{
    CaptureAuthorization, CaptureKind, PoolCapture, PoolParticipationAuthorization,
    AUTHORIZATION_VERSION, MAX_AUTHORIZATION_LIFETIME_SECS,
};

use soroban_sdk::{
    contract, contractimpl, xdr::ToXdr, Address, Bytes, BytesN, Env, IntoVal, Symbol,
};

const CAPTURE_DOMAIN: &[u8] = b"REAPP\0AP2\0CAPTURE\0V1\0";
const PARTICIPATION_DOMAIN: &[u8] = b"REAPP\0AP2\0POOL-PARTICIPATION\0V1\0";

#[contract]
pub struct Ap2AuthorizationExtension;

#[contractimpl]
impl Ap2AuthorizationExtension {
    pub fn __constructor(env: Env, admin: Address) {
        storage::set_admin(&env, &admin);
    }

    pub fn get_admin(env: Env) -> Address {
        storage::get_admin(&env)
    }

    pub fn set_admin(env: Env, new_admin: Address) {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::set_admin(&env, &new_admin);
        // The admin owns the verifier allowlist, which is this contract's whole
        // trust root. Both changes to it must be observable on-chain.
        env.events()
            .publish((Symbol::new(&env, "ap2_admin"),), (admin, new_admin));
    }

    pub fn set_verifier(env: Env, verifier_key: BytesN<32>, enabled: bool) {
        storage::get_admin(&env).require_auth();
        storage::set_verifier(&env, &verifier_key, enabled);
        env.events()
            .publish((Symbol::new(&env, "ap2_verifier"), verifier_key), enabled);
    }

    pub fn verifier_enabled(env: Env, verifier_key: BytesN<32>) -> bool {
        storage::verifier_enabled(&env, &verifier_key)
    }

    pub fn capture_id(env: Env, authorization: CaptureAuthorization) -> BytesN<32> {
        authorization_id(&env, CAPTURE_DOMAIN, &authorization)
    }

    pub fn participation_id(env: Env, authorization: PoolParticipationAuthorization) -> BytesN<32> {
        authorization_id(&env, PARTICIPATION_DOMAIN, &authorization)
    }

    pub fn is_consumed(env: Env, authorization_id: BytesN<32>) -> bool {
        storage::is_consumed(&env, &authorization_id)
    }

    /// Route an exact AP2-authorized solo capture into an unchanged registry.
    pub fn execute_simple(
        env: Env,
        authorization: CaptureAuthorization,
        signature: BytesN<64>,
    ) -> Result<(), Error> {
        if authorization.kind != CaptureKind::Simple {
            return Err(Error::WrongCaptureKind);
        }
        execute_solo(&env, authorization, signature)
    }

    /// Route an exact AP2-authorized solo capture into Composite. This is the
    /// fallback for an AP2 pool child that was released without being charged.
    /// Composite stores this extension as the mandate agent, preventing the
    /// shopping agent from bypassing the AP2 evidence check.
    pub fn execute_composite_solo(
        env: Env,
        authorization: CaptureAuthorization,
        signature: BytesN<64>,
    ) -> Result<(), Error> {
        if authorization.kind != CaptureKind::CompositeSolo {
            return Err(Error::WrongCaptureKind);
        }
        execute_solo(&env, authorization, signature)
    }

    pub fn get_pool_participation(
        env: Env,
        participation_id: BytesN<32>,
    ) -> Result<PoolParticipationAuthorization, Error> {
        storage::get_participation(&env, &participation_id).ok_or(Error::ParticipationNotFound)
    }

    /// Called by Composite while committing a child to an AP2-aware pool.
    pub fn register_pool_participation(
        env: Env,
        authorization: PoolParticipationAuthorization,
        signature: BytesN<64>,
    ) -> Result<BytesN<32>, Error> {
        authorization.registry.require_auth();
        validate_common(
            &env,
            authorization.version,
            &authorization.network_id,
            authorization.max_amount,
            authorization.not_before,
            authorization.expires_at,
            &authorization.verifier_key,
        )?;
        let id = authorization_id(&env, PARTICIPATION_DOMAIN, &authorization);
        if storage::has_participation(&env, &id) {
            return Err(Error::AlreadyRegistered);
        }
        verify_signature(&env, &authorization.verifier_key, &id, &signature);
        storage::set_participation(&env, &id, &authorization);
        env.events().publish(
            (Symbol::new(&env, "ap2_pool_reg"), id.clone()),
            (
                authorization.registry.clone(),
                authorization.pool_id.clone(),
                authorization.mandate_id.clone(),
            ),
        );
        Ok(id)
    }

    /// Called only from Composite's pooled money path for the exact winning leg.
    pub fn consume_pool(
        env: Env,
        participation_id: BytesN<32>,
        capture: PoolCapture,
    ) -> Result<(), Error> {
        let authorization = storage::get_participation(&env, &participation_id)
            .ok_or(Error::ParticipationNotFound)?;
        authorization.registry.require_auth();
        validate_common(
            &env,
            authorization.version,
            &authorization.network_id,
            authorization.max_amount,
            authorization.not_before,
            authorization.expires_at,
            &authorization.verifier_key,
        )?;
        if capture.amount <= 0 || capture.amount > authorization.max_amount {
            return Err(Error::CaptureExceedsParticipation);
        }
        if storage::is_consumed(&env, &participation_id) {
            return Err(Error::AlreadyConsumed);
        }
        storage::set_consumed(&env, &participation_id, authorization.expires_at);
        env.events().publish(
            (Symbol::new(&env, "ap2_pool_use"), participation_id.clone()),
            (
                authorization.registry,
                authorization.pool_id,
                authorization.mandate_id,
                capture.amount,
                capture.expected_seq,
                capture.outcome_root,
            ),
        );
        Ok(())
    }
}

fn execute_solo(
    env: &Env,
    authorization: CaptureAuthorization,
    signature: BytesN<64>,
) -> Result<(), Error> {
    authorization.agent.require_auth();
    let id = verify_capture(env, &authorization, &signature)?;
    storage::set_consumed(env, &id, authorization.expires_at);

    // The registry stores this extension as its agent. A direct call from this
    // contract therefore satisfies require_auth(agent); the registry still
    // checks sequence, scope, budget, expiry, state, and allowance.
    //
    // Read the mandate back first. Only `mandate_id`, `amount` and
    // `expected_seq` reach the registry, so without this the merchant and asset
    // a verifier signed into the authorization would never be compared against
    // the mandate the money actually moves under.
    match authorization.kind {
        CaptureKind::Simple => {
            let client = SimpleRegistryClient::new(env, &authorization.registry);
            let mandate = client.get_mandate(&authorization.mandate_id);
            require_registry_agrees(
                env,
                &authorization,
                &mandate.agent,
                &mandate.merchant,
                &mandate.asset,
            )?;
            client.execute_payment(
                &authorization.mandate_id,
                &authorization.amount,
                &authorization.expected_seq,
            );
        }
        CaptureKind::CompositeSolo => {
            let client = CompositeRegistryClient::new(env, &authorization.registry);
            let mandate = client.get_mandate(&authorization.mandate_id);
            require_registry_agrees(
                env,
                &authorization,
                &mandate.agent,
                &mandate.merchant,
                &mandate.asset,
            )?;
            client.execute_payment(
                &authorization.mandate_id,
                &authorization.amount,
                &authorization.expected_seq,
            );
        }
    }
    env.events().publish(
        (Symbol::new(env, "ap2_capture"), id),
        (
            authorization.registry,
            authorization.mandate_id,
            authorization.amount,
        ),
    );
    Ok(())
}

/// The registry mandate must be the one the AP2 evidence describes, and it must
/// name this extension as its agent — otherwise the shopping agent could spend
/// the mandate directly and skip the evidence check entirely.
fn require_registry_agrees(
    env: &Env,
    authorization: &CaptureAuthorization,
    agent: &Address,
    merchant: &Address,
    asset: &Address,
) -> Result<(), Error> {
    if *agent != env.current_contract_address()
        || *merchant != authorization.merchant
        || *asset != authorization.asset
    {
        return Err(Error::RegistryMandateMismatch);
    }
    Ok(())
}

fn verify_capture(
    env: &Env,
    authorization: &CaptureAuthorization,
    signature: &BytesN<64>,
) -> Result<BytesN<32>, Error> {
    validate_common(
        env,
        authorization.version,
        &authorization.network_id,
        authorization.amount,
        authorization.not_before,
        authorization.expires_at,
        &authorization.verifier_key,
    )?;
    let id = authorization_id(env, CAPTURE_DOMAIN, authorization);
    if storage::is_consumed(env, &id) {
        return Err(Error::AlreadyConsumed);
    }
    verify_signature(env, &authorization.verifier_key, &id, signature);
    Ok(id)
}

fn validate_common(
    env: &Env,
    version: u32,
    network_id: &BytesN<32>,
    amount: i128,
    not_before: u64,
    expires_at: u64,
    verifier_key: &BytesN<32>,
) -> Result<(), Error> {
    if version != AUTHORIZATION_VERSION {
        return Err(Error::UnsupportedVersion);
    }
    if *network_id != env.ledger().network_id() {
        return Err(Error::WrongNetwork);
    }
    if amount <= 0 {
        return Err(Error::InvalidAmount);
    }
    let now = env.ledger().timestamp();
    if not_before > now || expires_at <= now || not_before >= expires_at {
        return Err(Error::InvalidWindow);
    }
    // A replay marker is only useful while it is still stored. Soroban caps a
    // TTL extension at the network's max entry TTL, so an authorization allowed
    // to outlive that cap could have its `Consumed` marker archived while the
    // authorization itself was still valid. Bound the lifetime instead of
    // relying on archival behavior.
    if expires_at - not_before > MAX_AUTHORIZATION_LIFETIME_SECS {
        return Err(Error::AuthorizationTooLong);
    }
    if !storage::verifier_enabled(env, verifier_key) {
        return Err(Error::VerifierDisabled);
    }
    Ok(())
}

fn authorization_id<T>(env: &Env, domain: &[u8], authorization: &T) -> BytesN<32>
where
    T: Clone + IntoVal<Env, soroban_sdk::Val>,
{
    let mut bytes = Bytes::from_slice(env, domain);
    bytes.append(&authorization.clone().to_xdr(env));
    env.crypto().sha256(&bytes).into()
}

fn verify_signature(env: &Env, verifier_key: &BytesN<32>, id: &BytesN<32>, signature: &BytesN<64>) {
    let message = Bytes::from_array(env, &id.to_array());
    env.crypto()
        .ed25519_verify(verifier_key, &message, signature);
}

#[cfg(test)]
mod test;
