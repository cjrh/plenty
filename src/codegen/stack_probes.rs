//! Identifies pinned Cranelift 0.137 AArch64 loop probes for signal diagnostics.
//!
//! The backend's `StackProbeLoop` emitter probes below SP using x16, without
//! moving SP. Its trusted stores carry no MachTrap records. Register the exact
//! store PCs in the executable's trap table, so the runtime never has to read
//! arbitrary instruction memory or mistake another below-SP access for a probe.
//! If upstream changes this sequence, matching fails closed: stack protection
//! remains enabled, but the overflow integration tests require updating this
//! diagnostic recognizer before the new backend can be qualified.

/// Store offsets in the four-instruction loop emitted by AArch64's
/// `StackProbeLoop`: sub x16,4096; str wzr,[sp,x16]; cmn x16,x17; b.gt -12.
/// Inline unrolled probes move SP themselves and need no special registration.
pub(super) fn arm64_loop_stores(bytes: &[u8]) -> impl Iterator<Item = u32> + '_ {
    const LOOP: [u32; 4] = [0xd140_0610, 0xb830_6bff, 0xab11_021f, 0x54ff_ffac];
    bytes
        .windows(16)
        .step_by(4)
        .enumerate()
        .filter_map(|(index, window)| {
            if LOOP
                .iter()
                .zip(window.chunks_exact(4))
                .all(|(&word, bytes)| word == u32::from_le_bytes(bytes.try_into().unwrap()))
            {
                Some(u32::try_from(index * 4 + 4).expect("function offset fits u32"))
            } else {
                None
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe() -> Vec<u8> {
        [0xd140_0610u32, 0xb830_6bff, 0xab11_021f, 0x54ff_ffac]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect()
    }

    #[test]
    fn exact_aligned_sequences_register_only_the_store_pc() {
        let mut bytes = vec![0; 8];
        bytes.extend(probe());
        bytes.extend([0; 4]);
        bytes.extend(probe());
        assert_eq!(arm64_loop_stores(&bytes).collect::<Vec<_>>(), [12, 32]);
        assert_eq!(arm64_loop_stores(&bytes[..23]).count(), 0);
    }

    #[test]
    fn changed_instructions_and_unaligned_data_are_not_probes() {
        for byte in 0..16 {
            let mut bytes = probe();
            bytes[byte] ^= 1;
            assert_eq!(arm64_loop_stores(&bytes).count(), 0);
        }
        let mut bytes = vec![0];
        bytes.extend(probe());
        assert_eq!(arm64_loop_stores(&bytes).count(), 0);
    }
}
