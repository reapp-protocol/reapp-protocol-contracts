//! AP2-aware pool integration tests. These use the real authorization
//! extension, Composite registry and SEP-41 token in one Soroban environment.

#![cfg(test)]

use ap2_authorization_extension::{
    Ap2AuthorizationExtension, Ap2AuthorizationExtensionClient, CaptureAuthorization, CaptureKind,
};
use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{Address, Bytes, BytesN, Env, Vec};

use crate::{
    Ap2PoolPolicy, ClearingKind, Error, MandateRegistry, MandateRegistryClient,
    PoolParticipationAuthorization, PoolState, PoolStatus, SchedulePoint,
};

const NOW: u64 = 1_000;
const DEADLINE: u64 = 2_000;
const EXPIRY: u64 = 200_000;
const NETWORK: [u8; 32] = [9; 32];
const MAX_AMOUNT: i128 = 500;
const PARTICIPATION_DOMAIN: &[u8] = b"REAPP\0AP2\0POOL-PARTICIPATION\0V1\0";

struct World {
    env: Env,
    registry: Address,
    extension: Address,
    originator: Address,
    merchant: Address,
    asset: Address,
    agent: Address,
    verifier: SigningKey,
}

impl World {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(NOW);
        env.ledger().set_network_id(NETWORK);

        let admin = Address::generate(&env);
        let registry = env.register(MandateRegistry, (admin.clone(),));
        let extension = env.register(Ap2AuthorizationExtension, (&admin,));
        let verifier = SigningKey::from_bytes(&[7; 32]);
        Ap2AuthorizationExtensionClient::new(&env, &extension).set_verifier(
            &BytesN::from_array(&env, &verifier.verifying_key().to_bytes()),
            &true,
        );

        let asset_admin = Address::generate(&env);
        let asset = env
            .register_stellar_asset_contract_v2(asset_admin)
            .address();
        Self {
            originator: Address::generate(&env),
            merchant: Address::generate(&env),
            agent: Address::generate(&env),
            env,
            registry,
            extension,
            asset,
            verifier,
        }
    }

    fn client(&self) -> MandateRegistryClient<'_> {
        MandateRegistryClient::new(&self.env, &self.registry)
    }

    fn extension_client(&self) -> Ap2AuthorizationExtensionClient<'_> {
        Ap2AuthorizationExtensionClient::new(&self.env, &self.extension)
    }

    fn schedule(&self) -> Vec<SchedulePoint> {
        soroban_sdk::vec![
            &self.env,
            SchedulePoint {
                unit_price: 125,
                max_qty: 1,
            }
        ]
    }

    fn pool(&self, ap2: bool) -> BytesN<32> {
        if ap2 {
            self.client().register_pool_ap2(
                &self.originator,
                &self.merchant,
                &self.asset,
                &ClearingKind::ThresholdFloor,
                &1,
                &0,
                &0,
                &DEADLINE,
                &BytesN::from_array(&self.env, &[1; 32]),
                &self.extension,
            )
        } else {
            self.client().register_pool(
                &self.originator,
                &self.merchant,
                &self.asset,
                &ClearingKind::ThresholdFloor,
                &1,
                &0,
                &0,
                &DEADLINE,
                &BytesN::from_array(&self.env, &[2; 32]),
            )
        }
    }

    fn child(&self, pool_id: &BytesN<32>, id_byte: u8) -> (Address, BytesN<32>) {
        let user = Address::generate(&self.env);
        let mandate_id = BytesN::from_array(&self.env, &[id_byte; 32]);
        StellarAssetClient::new(&self.env, &self.asset).mint(&user, &1_000);
        TokenClient::new(&self.env, &self.asset).approve(&user, &self.registry, &1_000, &100_000);
        self.client().register_mandate(
            &user,
            &self.extension,
            &self.merchant,
            &self.asset,
            &MAX_AMOUNT,
            &EXPIRY,
            &mandate_id,
            &Some(pool_id.clone()),
            &self.schedule(),
        );
        (user, mandate_id)
    }

    fn authorization(
        &self,
        pool_id: &BytesN<32>,
        mandate_id: &BytesN<32>,
    ) -> PoolParticipationAuthorization {
        PoolParticipationAuthorization {
            version: 1,
            network_id: BytesN::from_array(&self.env, &NETWORK),
            registry: self.registry.clone(),
            pool_id: pool_id.clone(),
            mandate_id: mandate_id.clone(),
            agent: self.agent.clone(),
            merchant: self.merchant.clone(),
            asset: self.asset.clone(),
            max_amount: MAX_AMOUNT,
            schedule_hash: self.client().ap2_schedule_hash(&self.schedule()),
            open_checkout_evidence: BytesN::from_array(&self.env, &[10; 32]),
            closed_checkout_evidence: BytesN::from_array(&self.env, &[11; 32]),
            open_participation_evidence: BytesN::from_array(&self.env, &[12; 32]),
            closed_participation_evidence: BytesN::from_array(&self.env, &[13; 32]),
            nonce: BytesN::from_array(&self.env, &[14; 32]),
            verifier_key: BytesN::from_array(&self.env, &self.verifier.verifying_key().to_bytes()),
            not_before: NOW - 1,
            expires_at: EXPIRY,
        }
    }

    fn id_and_signature(
        &self,
        authorization: &PoolParticipationAuthorization,
    ) -> (BytesN<32>, BytesN<64>) {
        let mut bytes = Bytes::from_slice(&self.env, PARTICIPATION_DOMAIN);
        bytes.append(&authorization.clone().to_xdr(&self.env));
        let id: BytesN<32> = self.env.crypto().sha256(&bytes).into();
        let signature =
            BytesN::from_array(&self.env, &self.verifier.sign(&id.to_array()).to_bytes());
        (id, signature)
    }
}

#[test]
fn ap2_pool_commits_and_captures_through_real_extension() {
    let world = World::new();
    let pool_id = world.pool(true);
    let (user, mandate_id) = world.child(&pool_id, 3);
    let authorization = world.authorization(&pool_id, &mandate_id);
    let (participation_id, signature) = world.id_and_signature(&authorization);

    world
        .client()
        .commit_child_ap2(&mandate_id, &authorization, &signature);
    assert_eq!(
        world.client().get_ap2_pool_policy(&pool_id),
        Ap2PoolPolicy {
            extension: world.extension.clone()
        }
    );
    assert_eq!(
        world
            .client()
            .get_ap2_mandate_policy(&mandate_id)
            .participation_id,
        participation_id
    );
    assert_eq!(
        world.client().try_commit_child(&mandate_id),
        Err(Ok(Error::Ap2Required))
    );

    world.env.ledger().set_timestamp(DEADLINE);
    assert_eq!(
        world.client().try_clear_pool(&pool_id),
        Err(Ok(Error::Ap2Required))
    );
    world.client().clear_pool_ap2(&pool_id);

    let mandate = world.client().get_mandate(&mandate_id);
    assert_eq!(mandate.pool_state, PoolState::Captured);
    assert_eq!(mandate.spent, 1);
    assert_eq!(mandate.seq, 1);
    assert_eq!(
        world.client().get_pool(&pool_id).status,
        PoolStatus::Cleared
    );
    assert_eq!(
        TokenClient::new(&world.env, &world.asset).balance(&world.merchant),
        1
    );
    assert_eq!(
        TokenClient::new(&world.env, &world.asset).balance(&user),
        999
    );
    assert!(world.extension_client().is_consumed(&participation_id));
}

#[test]
fn pool_modes_cannot_be_mixed_or_bypassed() {
    let world = World::new();
    let ap2_pool_id = world.pool(true);
    let (_, ap2_mandate_id) = world.child(&ap2_pool_id, 3);
    let mut authorization = world.authorization(&ap2_pool_id, &ap2_mandate_id);
    let (_, signature) = world.id_and_signature(&authorization);

    assert_eq!(
        world.client().try_commit_child(&ap2_mandate_id),
        Err(Ok(Error::Ap2Required))
    );
    authorization.schedule_hash = BytesN::from_array(&world.env, &[99; 32]);
    assert_eq!(
        world
            .client()
            .try_commit_child_ap2(&ap2_mandate_id, &authorization, &signature),
        Err(Ok(Error::Ap2AuthorizationMismatch))
    );
    assert_eq!(
        world.client().get_mandate(&ap2_mandate_id).pool_state,
        PoolState::Unlinked
    );

    let legacy_pool_id = world.pool(false);
    let (_, legacy_mandate_id) = world.child(&legacy_pool_id, 4);
    let authorization = world.authorization(&legacy_pool_id, &legacy_mandate_id);
    let (_, signature) = world.id_and_signature(&authorization);
    assert_eq!(
        world
            .client()
            .try_commit_child_ap2(&legacy_mandate_id, &authorization, &signature),
        Err(Ok(Error::Ap2NotEnabled))
    );
    assert_eq!(
        world.client().try_clear_pool_ap2(&legacy_pool_id),
        Err(Ok(Error::Ap2NotEnabled))
    );
}

#[test]
fn participation_must_cover_the_full_capture_window() {
    let world = World::new();
    let pool_id = world.pool(true);
    let (_, mandate_id) = world.child(&pool_id, 3);
    let mut authorization = world.authorization(&pool_id, &mandate_id);
    authorization.expires_at = DEADLINE + crate::pooltypes::CAPTURE_WINDOW_SECS;
    let (_, signature) = world.id_and_signature(&authorization);

    assert_eq!(
        world
            .client()
            .try_commit_child_ap2(&mandate_id, &authorization, &signature),
        Err(Ok(Error::Ap2AuthorizationTooShort))
    );
    assert_eq!(
        world.client().get_mandate(&mandate_id).pool_state,
        PoolState::Unlinked
    );
}

#[test]
fn typescript_and_composite_schedule_hash_vectors_match() {
    let world = World::new();
    assert_eq!(
        world
            .client()
            .ap2_schedule_hash(&world.schedule())
            .to_array(),
        [
            0x98, 0x44, 0x52, 0x1e, 0xa8, 0x76, 0x9e, 0xbb, 0x50, 0x26, 0x65, 0x20, 0x3f, 0xd9,
            0x7c, 0x61, 0x7f, 0x19, 0xe1, 0x7c, 0x6c, 0x2d, 0x5b, 0xa9, 0x0c, 0xbb, 0xd7, 0x9b,
            0xed, 0xdd, 0x82, 0x51,
        ]
    );
}

#[test]
fn extension_rejection_rolls_composite_state_and_transfers_back() {
    let world = World::new();
    let pool_id = world.pool(true);
    let (user, mandate_id) = world.child(&pool_id, 3);
    let authorization = world.authorization(&pool_id, &mandate_id);
    let (participation_id, signature) = world.id_and_signature(&authorization);
    world
        .client()
        .commit_child_ap2(&mandate_id, &authorization, &signature);

    world.extension_client().set_verifier(
        &BytesN::from_array(&world.env, &world.verifier.verifying_key().to_bytes()),
        &false,
    );
    world.env.ledger().set_timestamp(DEADLINE);
    assert!(world.client().try_clear_pool_ap2(&pool_id).is_err());

    assert_eq!(world.client().get_pool(&pool_id).status, PoolStatus::Open);
    let mandate = world.client().get_mandate(&mandate_id);
    assert_eq!(mandate.pool_state, PoolState::Committed);
    assert_eq!(mandate.spent, 0);
    assert_eq!(mandate.seq, 0);
    assert_eq!(
        TokenClient::new(&world.env, &world.asset).balance(&world.merchant),
        0
    );
    assert_eq!(
        TokenClient::new(&world.env, &world.asset).balance(&user),
        1_000
    );
    assert!(!world.extension_client().is_consumed(&participation_id));

    world.extension_client().set_verifier(
        &BytesN::from_array(&world.env, &world.verifier.verifying_key().to_bytes()),
        &true,
    );
    world.client().clear_pool_ap2(&pool_id);
    assert!(world.extension_client().is_consumed(&participation_id));
}

#[test]
fn released_ap2_child_uses_composite_solo_route() {
    let world = World::new();
    let pool_id = world.client().register_pool_ap2(
        &world.originator,
        &world.merchant,
        &world.asset,
        &ClearingKind::ThresholdFloor,
        &2,
        &0,
        &0,
        &DEADLINE,
        &BytesN::from_array(&world.env, &[21; 32]),
        &world.extension,
    );
    let (user, mandate_id) = world.child(&pool_id, 3);
    let participation = world.authorization(&pool_id, &mandate_id);
    let (_, participation_signature) = world.id_and_signature(&participation);
    world
        .client()
        .commit_child_ap2(&mandate_id, &participation, &participation_signature);

    world.env.ledger().set_timestamp(DEADLINE);
    world.client().clear_pool_ap2(&pool_id);
    let released = world.client().get_mandate(&mandate_id);
    assert_eq!(released.pool_state, PoolState::Released);
    assert_eq!(released.agent, world.extension);

    let capture = CaptureAuthorization {
        version: 1,
        network_id: BytesN::from_array(&world.env, &NETWORK),
        registry: world.registry.clone(),
        kind: CaptureKind::CompositeSolo,
        mandate_id: mandate_id.clone(),
        agent: world.agent.clone(),
        merchant: world.merchant.clone(),
        asset: world.asset.clone(),
        amount: 25,
        expected_seq: 0,
        open_checkout_evidence: BytesN::from_array(&world.env, &[31; 32]),
        closed_checkout_evidence: BytesN::from_array(&world.env, &[32; 32]),
        open_payment_evidence: BytesN::from_array(&world.env, &[33; 32]),
        closed_payment_evidence: BytesN::from_array(&world.env, &[34; 32]),
        nonce: BytesN::from_array(&world.env, &[35; 32]),
        verifier_key: BytesN::from_array(&world.env, &world.verifier.verifying_key().to_bytes()),
        not_before: DEADLINE,
        expires_at: DEADLINE + 600,
    };
    let capture_id = world.extension_client().capture_id(&capture);
    let capture_signature = BytesN::from_array(
        &world.env,
        &world.verifier.sign(&capture_id.to_array()).to_bytes(),
    );
    world
        .extension_client()
        .execute_composite_solo(&capture, &capture_signature);

    let paid = world.client().get_mandate(&mandate_id);
    assert_eq!(paid.spent, 25);
    assert_eq!(paid.seq, 1);
    assert_eq!(
        TokenClient::new(&world.env, &world.asset).balance(&world.merchant),
        25
    );
    assert_eq!(
        TokenClient::new(&world.env, &world.asset).balance(&user),
        975
    );
}
