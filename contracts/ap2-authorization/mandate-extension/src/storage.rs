//! The only module that touches contract storage.

use soroban_sdk::{contracttype, BytesN, Env};

use crate::types::PoolParticipationAuthorization;

const DAY_IN_LEDGERS: u32 = 17_280;
const TTL_THRESHOLD: u32 = DAY_IN_LEDGERS;
const TTL_EXTEND: u32 = 30 * DAY_IN_LEDGERS;
const SECS_PER_LEDGER: u64 = 5;

#[contracttype]
pub enum DataKey {
    Admin,
    Verifier(BytesN<32>),
    Consumed(BytesN<32>),
    Participation(BytesN<32>),
}

pub fn set_admin(env: &Env, admin: &soroban_sdk::Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn get_admin(env: &Env) -> soroban_sdk::Address {
    env.storage().instance().get(&DataKey::Admin).unwrap()
}

pub fn set_verifier(env: &Env, key: &BytesN<32>, enabled: bool) {
    env.storage()
        .persistent()
        .set(&DataKey::Verifier(key.clone()), &enabled);
    extend(env, &DataKey::Verifier(key.clone()), u64::MAX);
}

pub fn verifier_enabled(env: &Env, key: &BytesN<32>) -> bool {
    env.storage()
        .persistent()
        .get(&DataKey::Verifier(key.clone()))
        .unwrap_or(false)
}

pub fn is_consumed(env: &Env, id: &BytesN<32>) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::Consumed(id.clone()))
}

pub fn set_consumed(env: &Env, id: &BytesN<32>, expires_at: u64) {
    let key = DataKey::Consumed(id.clone());
    env.storage().persistent().set(&key, &true);
    extend(
        env,
        &key,
        expires_at.saturating_sub(env.ledger().timestamp()),
    );
}

pub fn has_participation(env: &Env, id: &BytesN<32>) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::Participation(id.clone()))
}

pub fn set_participation(
    env: &Env,
    id: &BytesN<32>,
    authorization: &PoolParticipationAuthorization,
) {
    let key = DataKey::Participation(id.clone());
    env.storage().persistent().set(&key, authorization);
    extend(
        env,
        &key,
        authorization
            .expires_at
            .saturating_sub(env.ledger().timestamp()),
    );
}

pub fn get_participation(env: &Env, id: &BytesN<32>) -> Option<PoolParticipationAuthorization> {
    env.storage()
        .persistent()
        .get(&DataKey::Participation(id.clone()))
}

fn extend(env: &Env, key: &DataKey, horizon_secs: u64) {
    let needed = (horizon_secs / SECS_PER_LEDGER).saturating_mul(2);
    let ledgers = needed.max(TTL_EXTEND as u64).min(u32::MAX as u64) as u32;
    env.storage()
        .persistent()
        .extend_ttl(key, TTL_THRESHOLD, ledgers);
}
