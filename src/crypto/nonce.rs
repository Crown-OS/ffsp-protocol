use crate::crypto::{CipherSuite, CryptoError};
use crate::wire::SequenceNumber;

/// Per-lane nonce counter.
///
/// The counter never travels: both sides increment in lockstep along a reliable ordered lane, and
/// the frame header's 16-bit sequence number is a desync check rather than the source of truth.
/// That keeps the nonce entirely off the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NonceSequence {
    counter: u64,
}

impl NonceSequence {
    pub const fn new() -> Self {
        Self { counter: 0 }
    }

    pub const fn counter(&self) -> u64 {
        self.counter
    }

    /// The sequence number the current nonce is stamped with.
    pub const fn sequence_number(&self) -> SequenceNumber {
        let [low, high, ..] = self.counter.to_le_bytes();
        SequenceNumber::new(u16::from_le_bytes([low, high]))
    }

    pub const fn nonce(&self) -> [u8; CipherSuite::NONCE_LEN] {
        let [a, b, c, d, e, f, g, h] = self.counter.to_le_bytes();
        [a, b, c, d, e, f, g, h, 0, 0, 0, 0]
    }

    /// Advances the counter, refusing to wrap.
    pub const fn advance(&mut self) -> Result<(), CryptoError> {
        match self.counter.checked_add(1) {
            Some(next) => {
                self.counter = next;
                Ok(())
            }
            None => Err(CryptoError::NonceSpaceExhausted),
        }
    }

    /// Confirms an arriving frame is the one this counter expects.
    pub const fn expect(&self, received: SequenceNumber) -> Result<(), CryptoError> {
        let expected = self.sequence_number();
        if expected.as_u16() == received.as_u16() {
            Ok(())
        } else {
            Err(CryptoError::SequenceDesync { expected, received })
        }
    }
}
