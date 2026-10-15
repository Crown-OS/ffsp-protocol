use snow::{Builder, HandshakeState};

use crate::crypto::{Direction, SessionSecret};
use crate::device::DeviceId;
use crate::link::{LinkSecret, PairingError, StaticIdentity, VerificationCode};

/// Noise pattern for meeting a device for the first time: mutual authentication with static keys
/// exchanged inside the handshake, so neither key is visible to an observer.
const PAIR_PATTERN: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";

/// Noise pattern for a device already linked. The initiator knows the responder's static key, so
/// this completes in one round trip instead of two.
const RECONNECT_PATTERN: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2s";

const SESSION_SECRET_CONTEXT: &str = "crownos.ffsp.v1 session secret";
const LINK_SECRET_CONTEXT: &str = "crownos.ffsp.v1 link secret";

/// What a completed handshake yields.
#[derive(Debug)]
pub struct HandshakeOutcome {
    pub peer: DeviceId,
    pub peer_public_key: [u8; StaticIdentity::KEY_LEN],
    pub session_secret: SessionSecret,
    /// Present only for a first pairing, and then persisted for the life of the link.
    ///
    /// Discovery beacons are keyed with it, so it has to outlive sessions. It cannot be re-derived
    /// on reconnect: every handshake mixes in fresh ephemeral keys, so a re-derived value would
    /// differ each time and the two devices would stop recognising each other. `None` on reconnect
    /// makes overwriting the stored secret impossible rather than merely inadvisable.
    pub link_secret: Option<LinkSecret>,
    pub direction: Direction,
}

/// Drives a Noise handshake to completion.
/// Whether this handshake establishes a link or resumes one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Intent {
    Pair,
    Reconnect,
}

#[derive(Debug)]
pub struct Handshake {
    state: HandshakeState,
    direction: Direction,
    intent: Intent,
}

impl Handshake {
    /// Largest Noise handshake message either pattern produces.
    pub const MAX_MESSAGE_LEN: usize = 1024;

    pub fn pair_initiator(identity: &StaticIdentity) -> Result<Self, PairingError> {
        Self::build(
            PAIR_PATTERN,
            identity,
            None,
            Direction::Initiator,
            Intent::Pair,
        )
    }

    pub fn pair_responder(identity: &StaticIdentity) -> Result<Self, PairingError> {
        Self::build(
            PAIR_PATTERN,
            identity,
            None,
            Direction::Responder,
            Intent::Pair,
        )
    }

    pub fn reconnect_initiator(
        identity: &StaticIdentity,
        peer_public_key: &[u8; StaticIdentity::KEY_LEN],
    ) -> Result<Self, PairingError> {
        Self::build(
            RECONNECT_PATTERN,
            identity,
            Some(peer_public_key),
            Direction::Initiator,
            Intent::Reconnect,
        )
    }

    pub fn reconnect_responder(identity: &StaticIdentity) -> Result<Self, PairingError> {
        Self::build(
            RECONNECT_PATTERN,
            identity,
            None,
            Direction::Responder,
            Intent::Reconnect,
        )
    }

    fn build(
        pattern: &str,
        identity: &StaticIdentity,
        remote_public_key: Option<&[u8; StaticIdentity::KEY_LEN]>,
        direction: Direction,
        intent: Intent,
    ) -> Result<Self, PairingError> {
        let params = pattern
            .parse()
            .map_err(|_| PairingError::Pattern(pattern.to_owned()))?;
        let mut builder = Builder::new(params)
            .local_private_key(identity.secret_key())
            .map_err(|error| to_pattern_error(&error))?;
        if let Some(remote) = remote_public_key {
            builder = builder
                .remote_public_key(remote)
                .map_err(|error| to_pattern_error(&error))?;
        }
        let state = match direction {
            Direction::Initiator => builder.build_initiator(),
            Direction::Responder => builder.build_responder(),
        }
        .map_err(|error| to_pattern_error(&error))?;
        Ok(Self {
            state,
            direction,
            intent,
        })
    }

    pub fn write_message(&mut self, into: &mut [u8]) -> Result<usize, PairingError> {
        if !self.state.is_my_turn() {
            return Err(PairingError::OutOfTurn);
        }
        self.state
            .write_message(&[], into)
            .map_err(|_| PairingError::BufferTooSmall {
                capacity: into.len(),
            })
    }

    pub fn read_message(&mut self, message: &[u8]) -> Result<(), PairingError> {
        let mut payload = [0u8; Self::MAX_MESSAGE_LEN];
        self.state
            .read_message(message, &mut payload)
            .map_err(|_| PairingError::Rejected)?;
        Ok(())
    }

    pub fn is_finished(&self) -> bool {
        self.state.is_handshake_finished()
    }

    pub fn is_my_turn(&self) -> bool {
        self.state.is_my_turn()
    }

    pub const fn direction(&self) -> Direction {
        self.direction
    }

    /// The code a person compares on both screens.
    ///
    /// It commits to the whole transcript, including both static keys, so a machine that
    /// substituted a key of its own cannot make the two screens agree.
    pub fn verification_code(&self) -> VerificationCode {
        VerificationCode::from_transcript(self.state.get_handshake_hash())
    }

    pub fn peer_public_key(&self) -> Option<[u8; StaticIdentity::KEY_LEN]> {
        self.state.get_remote_static()?.try_into().ok()
    }

    /// Consumes the handshake, deriving the keys the session will use.
    ///
    /// The raw Noise split is taken rather than Noise's transport mode because this protocol keys
    /// every lane separately and manages its own nonces; a single transport cipher would put all
    /// lanes back on one counter.
    pub fn finish(mut self) -> Result<HandshakeOutcome, PairingError> {
        if !self.state.is_handshake_finished() {
            return Err(PairingError::Unfinished);
        }
        let peer_public_key = self.peer_public_key().ok_or(PairingError::Unfinished)?;
        let transcript = self.state.get_handshake_hash().to_vec();
        let (first, second) = self.state.dangerously_get_raw_split();

        let mut session = blake3::Hasher::new_derive_key(SESSION_SECRET_CONTEXT);
        session.update(&first);
        session.update(&second);
        session.update(&transcript);

        let link_secret = matches!(self.intent, Intent::Pair).then(|| {
            let mut link = blake3::Hasher::new_derive_key(LINK_SECRET_CONTEXT);
            link.update(&first);
            link.update(&second);
            LinkSecret::from_bytes(*link.finalize().as_bytes())
        });

        Ok(HandshakeOutcome {
            peer: DeviceId::from_public_key(&peer_public_key),
            peer_public_key,
            session_secret: SessionSecret::from_bytes(*session.finalize().as_bytes()),
            link_secret,
            direction: self.direction,
        })
    }
}

/// Generates a fresh long-term identity.
pub fn generate_identity() -> Result<StaticIdentity, PairingError> {
    let params = PAIR_PATTERN
        .parse()
        .map_err(|_| PairingError::Pattern(PAIR_PATTERN.to_owned()))?;
    let keypair = Builder::new(params)
        .generate_keypair()
        .map_err(|error| to_pattern_error(&error))?;
    let secret = keypair
        .private
        .as_slice()
        .try_into()
        .map_err(|_| PairingError::MalformedPeerKey(keypair.private.len()))?;
    let public = keypair
        .public
        .as_slice()
        .try_into()
        .map_err(|_| PairingError::MalformedPeerKey(keypair.public.len()))?;
    Ok(StaticIdentity::new(secret, public))
}

fn to_pattern_error(error: &snow::Error) -> PairingError {
    PairingError::Pattern(error.to_string())
}
