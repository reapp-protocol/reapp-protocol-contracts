use ed25519_dalek::{Signer, SigningKey};
use simple_mandate_registry::{MandateRegistry, MandateRegistryClient};
use soroban_sdk::{
    contract, contracterror, contractimpl,
    testutils::{Address as _, Ledger, MockAuth, MockAuthInvoke},
    token::{StellarAssetClient, TokenClient},
    Address, BytesN, Env, IntoVal, String,
};

use crate::{
    Ap2AuthorizationExtension, Ap2AuthorizationExtensionClient, CaptureAuthorization, CaptureKind,
    Error, PoolCapture, PoolParticipationAuthorization, AUTHORIZATION_VERSION,
};

const NOW: u64 = 1_800_000_000;
const NETWORK: [u8; 32] = [9; 32];

#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
enum MockError {
    ForcedFailure = 1,
}

#[contract]
struct MockSimpleRegistry;

#[contractimpl]
impl MockSimpleRegistry {
    pub fn __constructor(env: Env, agent: Address) {
        env.storage().instance().set(&0u32, &agent);
        env.storage().instance().set(&1u32, &0u32);
        env.storage().instance().set(&2u32, &false);
    }

    pub fn set_fail(env: Env, fail: bool) {
        env.storage().instance().set(&2u32, &fail);
    }

    pub fn count(env: Env) -> u32 {
        env.storage().instance().get(&1u32).unwrap_or(0)
    }

    pub fn execute_payment(env: Env, _mandate_id: BytesN<32>, amount: i128, _expected_seq: u32) {
        let agent: Address = env.storage().instance().get(&0u32).unwrap();
        agent.require_auth();
        if env.storage().instance().get(&2u32).unwrap_or(false) {
            soroban_sdk::panic_with_error!(&env, MockError::ForcedFailure);
        }
        assert!(amount > 0);
        let count: u32 = env.storage().instance().get(&1u32).unwrap_or(0);
        env.storage().instance().set(&1u32, &(count + 1));
    }
}

struct World {
    env: Env,
    extension: Address,
    verifier: SigningKey,
    registry: Address,
    agent: Address,
    merchant: Address,
    asset: Address,
}

impl World {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(NOW);
        env.ledger().set_network_id(NETWORK);
        let admin = Address::generate(&env);
        let extension = env.register(Ap2AuthorizationExtension, (&admin,));
        let registry = env.register(MockSimpleRegistry, (&extension,));
        let verifier = SigningKey::from_bytes(&[7; 32]);
        Ap2AuthorizationExtensionClient::new(&env, &extension).set_verifier(
            &BytesN::from_array(&env, &verifier.verifying_key().to_bytes()),
            &true,
        );
        Self {
            agent: Address::generate(&env),
            merchant: Address::generate(&env),
            asset: Address::generate(&env),
            env,
            extension,
            verifier,
            registry,
        }
    }

    fn client(&self) -> Ap2AuthorizationExtensionClient<'_> {
        Ap2AuthorizationExtensionClient::new(&self.env, &self.extension)
    }

    fn capture(&self) -> CaptureAuthorization {
        CaptureAuthorization {
            version: AUTHORIZATION_VERSION,
            network_id: BytesN::from_array(&self.env, &NETWORK),
            registry: self.registry.clone(),
            kind: CaptureKind::Simple,
            mandate_id: BytesN::from_array(&self.env, &[1; 32]),
            agent: self.agent.clone(),
            merchant: self.merchant.clone(),
            asset: self.asset.clone(),
            amount: 100,
            expected_seq: 0,
            open_checkout_evidence: BytesN::from_array(&self.env, &[2; 32]),
            closed_checkout_evidence: BytesN::from_array(&self.env, &[3; 32]),
            open_payment_evidence: BytesN::from_array(&self.env, &[4; 32]),
            closed_payment_evidence: BytesN::from_array(&self.env, &[5; 32]),
            nonce: BytesN::from_array(&self.env, &[6; 32]),
            verifier_key: BytesN::from_array(&self.env, &self.verifier.verifying_key().to_bytes()),
            not_before: NOW - 1,
            expires_at: NOW + 600,
        }
    }

    fn participation(&self) -> PoolParticipationAuthorization {
        PoolParticipationAuthorization {
            version: AUTHORIZATION_VERSION,
            network_id: BytesN::from_array(&self.env, &NETWORK),
            registry: self.registry.clone(),
            pool_id: BytesN::from_array(&self.env, &[10; 32]),
            mandate_id: BytesN::from_array(&self.env, &[11; 32]),
            agent: self.agent.clone(),
            merchant: self.merchant.clone(),
            asset: self.asset.clone(),
            max_amount: 500,
            schedule_hash: BytesN::from_array(&self.env, &[12; 32]),
            open_checkout_evidence: BytesN::from_array(&self.env, &[13; 32]),
            closed_checkout_evidence: BytesN::from_array(&self.env, &[14; 32]),
            open_participation_evidence: BytesN::from_array(&self.env, &[15; 32]),
            closed_participation_evidence: BytesN::from_array(&self.env, &[16; 32]),
            nonce: BytesN::from_array(&self.env, &[17; 32]),
            verifier_key: BytesN::from_array(&self.env, &self.verifier.verifying_key().to_bytes()),
            not_before: NOW - 1,
            expires_at: NOW + 600,
        }
    }

    fn sign(&self, id: &BytesN<32>) -> BytesN<64> {
        BytesN::from_array(&self.env, &self.verifier.sign(&id.to_array()).to_bytes())
    }
}

#[test]
fn exact_simple_authorization_routes_once() {
    let world = World::new();
    let authorization = world.capture();
    let id = world.client().capture_id(&authorization);
    let signature = world.sign(&id);

    world.client().execute_simple(&authorization, &signature);
    assert!(world.client().is_consumed(&id));
    assert_eq!(
        MockSimpleRegistryClient::new(&world.env, &world.registry).count(),
        1
    );
    assert_eq!(
        world
            .client()
            .try_execute_simple(&authorization, &signature),
        Err(Ok(Error::AlreadyConsumed))
    );
}

#[test]
fn downstream_failure_rolls_back_extension_consumption() {
    let world = World::new();
    MockSimpleRegistryClient::new(&world.env, &world.registry).set_fail(&true);
    let authorization = world.capture();
    let id = world.client().capture_id(&authorization);
    let signature = world.sign(&id);

    assert!(world
        .client()
        .try_execute_simple(&authorization, &signature)
        .is_err());
    assert!(!world.client().is_consumed(&id));
    assert_eq!(
        MockSimpleRegistryClient::new(&world.env, &world.registry).count(),
        0
    );
}

#[test]
fn pool_participation_registers_then_consumes_exactly_once() {
    let world = World::new();
    let authorization = world.participation();
    let id = world.client().participation_id(&authorization);
    let signature = world.sign(&id);
    assert_eq!(
        world
            .client()
            .register_pool_participation(&authorization, &signature),
        id
    );

    let capture = PoolCapture {
        amount: 250,
        expected_seq: 3,
        outcome_root: BytesN::from_array(&world.env, &[18; 32]),
    };
    world.client().consume_pool(&id, &capture);
    assert!(world.client().is_consumed(&id));
    assert_eq!(
        world.client().try_consume_pool(&id, &capture),
        Err(Ok(Error::AlreadyConsumed))
    );
}

#[test]
fn network_window_kind_and_maximum_fail_closed() {
    let world = World::new();
    let mut wrong_kind = world.capture();
    wrong_kind.kind = CaptureKind::CompositeSolo;
    let id = world.client().capture_id(&wrong_kind);
    assert_eq!(
        world
            .client()
            .try_execute_simple(&wrong_kind, &world.sign(&id)),
        Err(Ok(Error::WrongCaptureKind))
    );

    let mut wrong_network = world.capture();
    wrong_network.network_id = BytesN::from_array(&world.env, &[99; 32]);
    let id = world.client().capture_id(&wrong_network);
    assert_eq!(
        world
            .client()
            .try_execute_simple(&wrong_network, &world.sign(&id)),
        Err(Ok(Error::WrongNetwork))
    );

    let authorization = world.participation();
    let id = world.client().participation_id(&authorization);
    world
        .client()
        .register_pool_participation(&authorization, &world.sign(&id));
    assert_eq!(
        world.client().try_consume_pool(
            &id,
            &PoolCapture {
                amount: authorization.max_amount + 1,
                expected_seq: 0,
                outcome_root: BytesN::from_array(&world.env, &[1; 32]),
            },
        ),
        Err(Ok(Error::CaptureExceedsParticipation))
    );
}

#[test]
fn composite_solo_route_is_separate_from_simple() {
    let world = World::new();
    let mut authorization = world.capture();
    authorization.kind = CaptureKind::CompositeSolo;
    let id = world.client().capture_id(&authorization);
    let signature = world.sign(&id);

    world
        .client()
        .execute_composite_solo(&authorization, &signature);
    assert!(world.client().is_consumed(&id));
    assert_eq!(
        MockSimpleRegistryClient::new(&world.env, &world.registry).count(),
        1
    );

    let simple = world.capture();
    let simple_id = world.client().capture_id(&simple);
    assert_eq!(
        world
            .client()
            .try_execute_composite_solo(&simple, &world.sign(&simple_id)),
        Err(Ok(Error::WrongCaptureKind))
    );
}

#[test]
fn typescript_and_contract_capture_id_vector_match() {
    let env = Env::default();
    env.ledger().set_timestamp(NOW);
    env.ledger().set_network_id(NETWORK);
    let admin = Address::generate(&env);
    let extension = env.register(Ap2AuthorizationExtension, (&admin,));
    let parse_address = |value: &str| Address::from_string(&String::from_str(&env, value));
    let authorization = CaptureAuthorization {
        version: AUTHORIZATION_VERSION,
        network_id: BytesN::from_array(&env, &NETWORK),
        registry: parse_address("GCFIRY65OQE7DFP5KLNS2PF2LVZMUZYJX4OZIEQ36N2IQANUB5XVYOJR"),
        kind: CaptureKind::Simple,
        mandate_id: BytesN::from_array(&env, &[1; 32]),
        agent: parse_address("GCATS5YOVB6ROX2WUNKGNQ2MP3GMXDMKSG2O4N5CLX3A6W4PZGZZI55U"),
        merchant: parse_address("GDWUSKGGFDI4FRXK5EBTRECZSVQSSWJHHJOGH6JWG3AUMFFMQ435DIAG"),
        asset: parse_address("GDFJHLAXAUMHA4OWPOB4P7YO72AQR2HMIUYFOXLXE2DZGM633K7HZDQP"),
        amount: 100,
        expected_seq: 0,
        open_checkout_evidence: BytesN::from_array(&env, &[2; 32]),
        closed_checkout_evidence: BytesN::from_array(&env, &[3; 32]),
        open_payment_evidence: BytesN::from_array(&env, &[4; 32]),
        closed_payment_evidence: BytesN::from_array(&env, &[5; 32]),
        nonce: BytesN::from_array(&env, &[6; 32]),
        verifier_key: BytesN::from_array(
            &env,
            &[
                0xea, 0x4a, 0x6c, 0x63, 0xe2, 0x9c, 0x52, 0x0a, 0xbe, 0xf5, 0x50, 0x7b, 0x13, 0x2e,
                0xc5, 0xf9, 0x95, 0x47, 0x76, 0xae, 0xbe, 0xbe, 0x7b, 0x92, 0x42, 0x1e, 0xea, 0x69,
                0x14, 0x46, 0xd2, 0x2c,
            ],
        ),
        not_before: NOW - 1,
        expires_at: NOW + 600,
    };

    assert_eq!(
        Ap2AuthorizationExtensionClient::new(&env, &extension)
            .capture_id(&authorization)
            .to_array(),
        [
            0x89, 0x93, 0xa7, 0x24, 0x30, 0xd4, 0xf6, 0x00, 0xf1, 0x51, 0xb0, 0xfa, 0xfd, 0x8a,
            0xb2, 0x4a, 0x15, 0xcf, 0x06, 0x64, 0x30, 0x65, 0x12, 0x30, 0x3d, 0xf3, 0xb7, 0xd9,
            0x7d, 0x23, 0x96, 0x63,
        ]
    );
}

#[test]
fn unchanged_simple_registry_moves_funds_and_keeps_the_allowance() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(NOW);
    env.ledger().set_network_id(NETWORK);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let actual_agent = Address::generate(&env);
    let merchant = Address::generate(&env);
    let extension = env.register(Ap2AuthorizationExtension, (&admin,));
    let simple = env.register(MandateRegistry, (admin.clone(),));
    let asset = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let verifier = SigningKey::from_bytes(&[7; 32]);
    let verifier_key = BytesN::from_array(&env, &verifier.verifying_key().to_bytes());
    Ap2AuthorizationExtensionClient::new(&env, &extension).set_verifier(&verifier_key, &true);

    let mandate_id = BytesN::from_array(&env, &[21; 32]);
    MandateRegistryClient::new(&env, &simple).register_mandate(
        &user,
        &extension,
        &merchant,
        &asset,
        &500,
        &(NOW + 1_000),
        &mandate_id,
    );
    StellarAssetClient::new(&env, &asset).mint(&user, &1_000);
    TokenClient::new(&env, &asset).approve(&user, &simple, &500, &100_000);

    let authorization = CaptureAuthorization {
        version: AUTHORIZATION_VERSION,
        network_id: BytesN::from_array(&env, &NETWORK),
        registry: simple.clone(),
        kind: CaptureKind::Simple,
        mandate_id: mandate_id.clone(),
        agent: actual_agent.clone(),
        merchant: merchant.clone(),
        asset: asset.clone(),
        amount: 125,
        expected_seq: 0,
        open_checkout_evidence: BytesN::from_array(&env, &[22; 32]),
        closed_checkout_evidence: BytesN::from_array(&env, &[23; 32]),
        open_payment_evidence: BytesN::from_array(&env, &[24; 32]),
        closed_payment_evidence: BytesN::from_array(&env, &[25; 32]),
        nonce: BytesN::from_array(&env, &[26; 32]),
        verifier_key,
        not_before: NOW - 1,
        expires_at: NOW + 600,
    };
    let client = Ap2AuthorizationExtensionClient::new(&env, &extension);
    let id = client.capture_id(&authorization);
    let signature = BytesN::from_array(&env, &verifier.sign(&id.to_array()).to_bytes());

    // Only the real shopping agent authorizes the extension entrypoint.
    // Simple's require_auth(extension) is satisfied by the direct contract
    // invocation, without giving the extension a token allowance.
    env.set_auths(&[]);
    client
        .mock_auths(&[MockAuth {
            address: &actual_agent,
            invoke: &MockAuthInvoke {
                contract: &extension,
                fn_name: "execute_simple",
                args: (authorization.clone(), signature.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .execute_simple(&authorization, &signature);

    assert_eq!(TokenClient::new(&env, &asset).balance(&merchant), 125);
    assert_eq!(
        TokenClient::new(&env, &asset).allowance(&user, &simple),
        375
    );
    assert_eq!(
        TokenClient::new(&env, &asset).allowance(&user, &extension),
        0
    );
    let mandate = MandateRegistryClient::new(&env, &simple).get_mandate(&mandate_id);
    assert_eq!(mandate.spent, 125);
    assert_eq!(mandate.seq, 1);
}
