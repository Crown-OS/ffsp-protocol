/// Randomness, injected so pairing and key generation are reproducible under test.
pub trait Entropy: Send + Sync {
    fn fill(&self, buffer: &mut [u8]);
}
