use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Error {
    UnsupportedVersion = 1,
    WrongNetwork = 2,
    InvalidAmount = 3,
    InvalidWindow = 4,
    VerifierDisabled = 5,
    AlreadyConsumed = 6,
    AlreadyRegistered = 7,
    ParticipationNotFound = 8,
    CaptureExceedsParticipation = 9,
    WrongCaptureKind = 10,
}
