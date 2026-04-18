//! LDPC(240, 101) codec with CRC-24 — used by the WSJT FST4 / FST4W family.
//!
//! Differs from [`Ldpc174_91`](crate::Ldpc174_91) in three ways:
//! - codeword length (240 vs 174 bits),
//! - information length (101 vs 91 bits — FST4 reserves 24 bits for CRC
//!   vs FT8's 14),
//! - CRC polynomial (CRC-24Q? TBD from WSJT-X `lib/fst4/crc24.cpp`).
//!
//! The BP and OSD algorithms are structurally identical to the
//! [`Ldpc174_91`] ones, so when the parity-check tables (MN, NM, NRW)
//! and the generator sub-matrix are populated from WSJT-X's
//! `ldpc_240_101_c_parity.f90` / `ldpc_240_101_c_generator.f90`, both
//! decoders can be generalised. Until then this module is a scaffold
//! for architecture validation — calling `encode` or `decode_soft`
//! panics with a clear pointer to what's missing.

use mfsk_core::{FecCodec, FecOpts, FecResult};

/// Codeword length of the FST4 LDPC code.
pub const LDPC_N: usize = 240;
/// Information-bit length (77 message bits + 24 CRC).
pub const LDPC_K: usize = 101;
/// Parity-bit count.
pub const LDPC_M: usize = LDPC_N - LDPC_K; // 139

/// Zero-sized codec implementing [`FecCodec`] for LDPC(240, 101).
///
/// Currently a scaffold — see module docs.
#[derive(Copy, Clone, Debug, Default)]
pub struct Ldpc240_101;

const SCAFFOLD_MSG: &str = "\
Ldpc240_101: parity-check tables not yet populated. \
To finish FST4 support, transcribe MN/NM/NRW from WSJT-X \
`lib/fst4/ldpc_240_101_c_parity.f90` and the generator sub-matrix \
from `ldpc_240_101_c_generator.f90`, then wire BP + OSD (the \
algorithms in mfsk-fec::ldpc::{bp,osd} generalise naturally once \
the code parameters are parameterised).";

impl FecCodec for Ldpc240_101 {
    const N: usize = LDPC_N;
    const K: usize = LDPC_K;

    fn encode(&self, _info: &[u8], _codeword: &mut [u8]) {
        unimplemented!("{SCAFFOLD_MSG}");
    }

    fn decode_soft(&self, _llr: &[f32], _opts: &FecOpts<'_>) -> Option<FecResult> {
        unimplemented!("{SCAFFOLD_MSG}");
    }
}
