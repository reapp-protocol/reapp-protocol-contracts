//! Minimal administration plane: authority rotation, emergency pause, and
//! same-address WASM upgrades. All state access remains centralized in
//! `storage`; Soroban host authorization rejects every unauthorized call.

use soroban_sdk::{contracttype, Address, BytesN, Env};

use crate::{events, storage, Error};

pub const UPGRADE_DELAY_SECONDS: u64 = 86_400;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingUpgrade {
    pub wasm_hash: BytesN<32>,
    pub execute_after: u64,
}

pub fn get_admin(env: &Env) -> Address {
    storage::get_admin(env)
}

pub fn set_admin(env: &Env, new_admin: Address) {
    let current = storage::get_admin(env);
    current.require_auth();
    storage::set_admin(env, &new_admin);
    events::admin_set(env, &new_admin);
}

pub fn is_paused(env: &Env) -> bool {
    storage::is_paused(env)
}

pub fn pause(env: &Env) {
    let admin = storage::get_admin(env);
    admin.require_auth();
    if !storage::is_paused(env) {
        storage::set_paused(env, true);
        events::paused(env, &admin);
    }
}

pub fn unpause(env: &Env) {
    let admin = storage::get_admin(env);
    admin.require_auth();
    if storage::is_paused(env) {
        storage::set_paused(env, false);
        events::unpaused(env, &admin);
    }
}

pub fn schedule_upgrade(env: &Env, new_wasm_hash: BytesN<32>) -> Result<u64, Error> {
    let admin = storage::get_admin(env);
    admin.require_auth();
    if storage::get_pending_upgrade(env).is_some() {
        return Err(Error::UpgradeAlreadyScheduled);
    }

    let pending = PendingUpgrade {
        wasm_hash: new_wasm_hash,
        execute_after: env
            .ledger()
            .timestamp()
            .saturating_add(UPGRADE_DELAY_SECONDS),
    };
    storage::set_pending_upgrade(env, &pending);
    events::upgrade_scheduled(env, &admin, &pending.wasm_hash, pending.execute_after);
    Ok(pending.execute_after)
}

pub fn cancel_upgrade(env: &Env) -> Result<(), Error> {
    let admin = storage::get_admin(env);
    admin.require_auth();
    let pending = storage::get_pending_upgrade(env).ok_or(Error::UpgradeNotScheduled)?;
    storage::remove_pending_upgrade(env);
    events::upgrade_cancelled(env, &admin, &pending.wasm_hash);
    Ok(())
}

pub fn execute_upgrade(env: &Env) -> Result<(), Error> {
    let admin = storage::get_admin(env);
    admin.require_auth();
    let pending = storage::get_pending_upgrade(env).ok_or(Error::UpgradeNotScheduled)?;
    if env.ledger().timestamp() < pending.execute_after {
        return Err(Error::UpgradeNotReady);
    }
    if !storage::is_paused(env) {
        return Err(Error::UpgradeRequiresPause);
    }

    storage::remove_pending_upgrade(env);
    events::upgrade_executed(env, &admin, &pending.wasm_hash);
    env.deployer()
        .update_current_contract_wasm(pending.wasm_hash);
    Ok(())
}
