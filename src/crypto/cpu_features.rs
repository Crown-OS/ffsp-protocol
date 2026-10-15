#[cfg(target_arch = "x86_64")]
cpufeatures::new!(aes_instructions, "aes", "sse2");

#[cfg(target_arch = "aarch64")]
cpufeatures::new!(aes_instructions, "aes");

/// Whether this processor accelerates AES.
///
/// Decided at runtime, not build time: one binary ships to every device, and an ARMv7 handset and
/// an ARMv8 handset answer differently.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub fn has_aes_hardware() -> bool {
    aes_instructions::get()
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub fn has_aes_hardware() -> bool {
    false
}
