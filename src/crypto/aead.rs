use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{AeadInPlace, Buffer, KeyInit};
use chacha20poly1305::ChaCha20Poly1305;

use crate::crypto::{CipherSuite, CryptoError, LaneKey, NonceSequence};
use crate::wire::{FrameHeader, FrameKind, LaneId, TAG_LEN};

/// Boxed so the enum stays pointer-sized; the AES key schedule is an order of magnitude larger
/// than the ChaCha state.
enum Primitive {
    ChaCha20Poly1305(Box<ChaCha20Poly1305>),
    Aes256Gcm(Box<Aes256Gcm>),
}

/// Names the suite and nothing else: the ciphers hold expanded key material.
impl core::fmt::Debug for Primitive {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::ChaCha20Poly1305(_) => "ChaCha20Poly1305",
            Self::Aes256Gcm(_) => "Aes256Gcm",
        })
    }
}

/// Seals and opens frames on one lane, in one direction.
///
/// The frame header doubles as the associated data, so the length, kind, lane and sequence number
/// are all authenticated without occupying a single byte beyond the header itself.
#[derive(Debug)]
pub struct LaneCipher {
    primitive: Primitive,
    nonces: NonceSequence,
    lane: LaneId,
}

impl LaneCipher {
    pub fn new(suite: CipherSuite, key: &LaneKey, lane: LaneId) -> Self {
        let bytes = key.as_bytes().into();
        let primitive = match suite {
            CipherSuite::Aes256Gcm => Primitive::Aes256Gcm(Box::new(Aes256Gcm::new(bytes))),
            CipherSuite::ChaCha20Poly1305 => {
                Primitive::ChaCha20Poly1305(Box::new(ChaCha20Poly1305::new(bytes)))
            }
        };
        Self {
            primitive,
            nonces: NonceSequence::new(),
            lane,
        }
    }

    /// Encrypts `body` in place and appends its tag, returning the header bytes to send first.
    ///
    /// Generic over the buffer so a [`ChunkBuffer`](crate::io::ChunkBuffer) can be sealed where it
    /// already sits, page-aligned, with no intermediate copy.
    pub fn seal(
        &mut self,
        kind: FrameKind,
        body: &mut impl Buffer,
    ) -> Result<[u8; FrameHeader::LEN], CryptoError> {
        let header = FrameHeader::new(
            kind,
            self.lane,
            self.nonces.sequence_number(),
            body.as_ref().len(),
        )?;
        let associated_data = header.to_bytes();
        let nonce = self.nonces.nonce();

        let sealed = match &self.primitive {
            Primitive::Aes256Gcm(cipher) => {
                cipher.encrypt_in_place((&nonce).into(), &associated_data, body)
            }
            Primitive::ChaCha20Poly1305(cipher) => {
                cipher.encrypt_in_place((&nonce).into(), &associated_data, body)
            }
        };
        sealed.map_err(|_| CryptoError::NotAuthentic)?;
        self.nonces.advance()?;
        Ok(associated_data)
    }

    /// Authenticates and decrypts a frame body in place, returning the plaintext.
    pub fn open<'a>(
        &mut self,
        header: &FrameHeader,
        sealed: &'a mut [u8],
    ) -> Result<&'a [u8], CryptoError> {
        self.nonces.expect(header.seq())?;
        let sealed_len = sealed.len();
        if sealed_len != header.body_len() + TAG_LEN {
            return Err(CryptoError::MissingTag(sealed_len));
        }
        let (body, tag) = sealed.split_at_mut(header.body_len());
        let tag: &[u8; TAG_LEN] = tag
            .first_chunk::<TAG_LEN>()
            .ok_or(CryptoError::MissingTag(sealed_len))?;

        let associated_data = header.to_bytes();
        let nonce = self.nonces.nonce();
        let opened = match &self.primitive {
            Primitive::Aes256Gcm(cipher) => cipher.decrypt_in_place_detached(
                (&nonce).into(),
                &associated_data,
                body,
                tag.into(),
            ),
            Primitive::ChaCha20Poly1305(cipher) => cipher.decrypt_in_place_detached(
                (&nonce).into(),
                &associated_data,
                body,
                tag.into(),
            ),
        };
        opened.map_err(|_| CryptoError::NotAuthentic)?;
        self.nonces.advance()?;
        Ok(body)
    }

    pub const fn lane(&self) -> LaneId {
        self.lane
    }

    pub const fn frames_sealed(&self) -> u64 {
        self.nonces.counter()
    }
}
