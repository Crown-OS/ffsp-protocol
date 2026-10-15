use core::fmt;

/// The short code a person compares on both screens before a link is accepted.
///
/// Derived from the handshake transcript, so a machine in the middle that substituted its own key
/// cannot make both screens agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerificationCode(u32);

impl VerificationCode {
    pub const DIGITS: u32 = 6;
    const MODULUS: u32 = 1_000_000;

    pub fn from_transcript(transcript: &[u8]) -> Self {
        let digest = blake3::keyed_hash(&[0u8; 32], transcript);
        let [a, b, c, d, ..] = *digest.as_bytes();
        Self(u32::from_le_bytes([a, b, c, d]) % Self::MODULUS)
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl fmt::Display for VerificationCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:06}", self.0)
    }
}
