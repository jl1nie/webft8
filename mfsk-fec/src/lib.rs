//! # mfsk-fec
//!
//! Forward-error-correction codecs shared across WSJT-family protocols.
//!
//! Each codec implements [`mfsk_core::FecCodec`] so generic pipeline code can
//! treat it uniformly. Protocol crates pick the codec via the
//! `type Fec = …;` associated type on [`mfsk_core::Protocol`].
//!
//! ## Contents
//!
//! | Family                   | Module          | Shared by               |
//! |--------------------------|-----------------|-------------------------|
//! | LDPC (174, 91) + CRC-14  | [`ldpc`]        | FT8, FT4                |
//! | LDPC (240, 101) + CRC-24 | [`ldpc240_101`] | FST4, FST4W (scaffold)  |
//! | (future) RS (63, 12)     | `rs`            | JT65                    |
//! | (future) Conv. + Fano    | `conv`          | JT9, WSPR               |

pub mod ldpc;
pub mod ldpc240_101;

pub use ldpc::Ldpc174_91;
pub use ldpc240_101::Ldpc240_101;
