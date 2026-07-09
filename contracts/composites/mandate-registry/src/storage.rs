//! The ONLY module that touches `env.storage`. Centralizing persistence here
//! means a change to key layout or TTL strategy touches exactly one file.

use soroban_sdk::{contracttype, BytesN, Env, Vec};

use crate::error::Error;
use crate::mandate::Mandate;
use crate::pooltypes::ClearingPool;

// ~5s ledgers → bump TTL well past a typical mandate's life.
const DAY_IN_LEDGERS: u32 = 17_280;
const TTL_THRESHOLD: u32 = DAY_IN_LEDGERS;
const TTL_EXTEND: u32 = 30 * DAY_IN_LEDGERS;
const SECS_PER_LEDGER: u64 = 5;

#[contracttype]
pub enum DataKey {
    Mandate(BytesN<32>),
    Pool(BytesN<32>),
    PoolMembers(BytesN<32>),
}

pub fn has_mandate(env: &Env, id: &BytesN<32>) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::Mandate(id.clone()))
}

pub fn get_mandate(env: &Env, id: BytesN<32>) -> Result<Mandate, Error> {
    let key = DataKey::Mandate(id);
    env.storage()
        .persistent()
        .get::<DataKey, Mandate>(&key)
        .ok_or(Error::NotFound)
}

pub fn set_mandate(env: &Env, id: &BytesN<32>, mandate: &Mandate) {
    let key = DataKey::Mandate(id.clone());
    env.storage().persistent().set(&key, mandate);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND);
}

pub fn has_pool(env: &Env, id: &BytesN<32>) -> bool {
    env.storage().persistent().has(&DataKey::Pool(id.clone()))
}

pub fn get_pool(env: &Env, id: BytesN<32>) -> Result<ClearingPool, Error> {
    let key = DataKey::Pool(id);
    env.storage()
        .persistent()
        .get::<DataKey, ClearingPool>(&key)
        .ok_or(Error::PoolNotFound)
}

pub fn set_pool(env: &Env, id: &BytesN<32>, pool: &ClearingPool) {
    let key = DataKey::Pool(id.clone());
    env.storage().persistent().set(&key, pool);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND);
}

/// Missing member list == empty (a pool is registered with no members).
pub fn get_pool_members(env: &Env, id: &BytesN<32>) -> Vec<BytesN<32>> {
    env.storage()
        .persistent()
        .get::<DataKey, Vec<BytesN<32>>>(&DataKey::PoolMembers(id.clone()))
        .unwrap_or_else(|| Vec::new(env))
}

pub fn set_pool_members(env: &Env, id: &BytesN<32>, members: &Vec<BytesN<32>>) {
    let key = DataKey::PoolMembers(id.clone());
    env.storage().persistent().set(&key, members);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND);
}

/// Bump an entry's TTL to cover at least `horizon_secs` from now (5s/ledger
/// estimate, 2x margin), never below the standard extension. `extend_ttl`
/// only ever extends, so a longer-lived entry is untouched. Every pool
/// touchpoint calls this for the pool, its member list, and the touched child,
/// so a pool can never be archived inside its own live window.
fn extend_to_horizon(env: &Env, key: &DataKey, horizon_secs: u64) {
    let needed = (horizon_secs / SECS_PER_LEDGER) * 2;
    let ledgers = needed.max(TTL_EXTEND as u64).min(u32::MAX as u64) as u32;
    env.storage().persistent().extend_ttl(key, ledgers, ledgers);
}

pub fn bump_pool_horizon(env: &Env, pool_id: &BytesN<32>, horizon_secs: u64) {
    extend_to_horizon(env, &DataKey::Pool(pool_id.clone()), horizon_secs);
    extend_to_horizon(env, &DataKey::PoolMembers(pool_id.clone()), horizon_secs);
}

pub fn bump_mandate_horizon(env: &Env, id: &BytesN<32>, horizon_secs: u64) {
    extend_to_horizon(env, &DataKey::Mandate(id.clone()), horizon_secs);
}
