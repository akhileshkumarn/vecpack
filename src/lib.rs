//! # vecpack
//!
//! Lightweight, from-scratch compression codecs with a strong emphasis on
//! **correctness** and **honest, reproducible benchmarking**.
//!
//! This is a learning/research project (see `README.md` and
//! `docs/DECISIONS.md`). The v0 building blocks are the primitives shared by
//! both columnar-analytics compression and ML embedding/tensor compression:
//!
//! - [`bitpack`]: fixed-width bit-packing of unsigned integers.
//! - [`frame_of_reference`]: subtract a per-block minimum, then bit-pack the
//!   (smaller) residuals. Good for values in a narrow range.
//! - [`delta`]: store per-block consecutive differences (zig-zag encoded), then
//!   bit-pack. Good for monotonic/trending data (timestamps, ids).
//! - [`rle`]: consecutive equal values become `(value, count)` runs.
//! - [`dictionary`]: replace values with small indices into a first-seen table.
//!
//! All four implement [`codec::Codec`] and can be selected at runtime via
//! [`codec::Scheme`]. Full 1024-value blocks of FOR/delta bit-packing use the
//! transposed layout in [`transposed`].
//!
//! ## The `BitPackable` abstraction
//!
//! The codecs are generic over unsigned integer widths via [`BitPackable`], so
//! the exact same, tested code path handles both `u32` and `u64`. The core
//! bit-shuffling uses a 128-bit accumulator, which comfortably holds up to
//! `64 (max width) + 7 (carry)` bits at once.

/// Unsigned integer types that can be bit-packed.
///
/// Implemented for `u32` and `u64`. Ordering/comparison is done through
/// [`BitPackable::to_u64`], which is monotonic for unsigned integers, so we do
/// not require an `Ord` bound.
pub trait BitPackable: Copy {
    /// Total bit width of the type (32 or 64).
    const WIDTH_BITS: u32;
    /// Byte width of the type (4 or 8).
    const BYTES: usize;

    /// Zero-extend the value to `u64`.
    fn to_u64(self) -> u64;
    /// Truncate/convert a `u64` back to this type.
    fn from_u64(v: u64) -> Self;
    /// Wrapping subtraction (used to compute frame-of-reference residuals).
    fn wrapping_sub(self, other: Self) -> Self;
    /// Wrapping addition (used to reconstruct values from residuals).
    fn wrapping_add(self, other: Self) -> Self;
    /// Append the little-endian bytes of `self` to `out`.
    fn write_le(self, out: &mut Vec<u8>);
    /// Read a value from the first [`BitPackable::BYTES`] bytes of `bytes`.
    fn read_le(bytes: &[u8]) -> Self;
}

macro_rules! impl_bitpackable {
    ($t:ty, $bits:expr, $bytes:expr) => {
        impl BitPackable for $t {
            const WIDTH_BITS: u32 = $bits;
            const BYTES: usize = $bytes;

            #[inline]
            fn to_u64(self) -> u64 {
                self as u64
            }
            #[inline]
            fn from_u64(v: u64) -> Self {
                v as $t
            }
            #[inline]
            fn wrapping_sub(self, other: Self) -> Self {
                <$t>::wrapping_sub(self, other)
            }
            #[inline]
            fn wrapping_add(self, other: Self) -> Self {
                <$t>::wrapping_add(self, other)
            }
            #[inline]
            fn write_le(self, out: &mut Vec<u8>) {
                out.extend_from_slice(&self.to_le_bytes());
            }
            #[inline]
            fn read_le(bytes: &[u8]) -> Self {
                let mut b = [0u8; $bytes];
                b.copy_from_slice(&bytes[..$bytes]);
                <$t>::from_le_bytes(b)
            }
        }
    };
}

impl_bitpackable!(u32, 32, 4);
impl_bitpackable!(u64, 64, 8);

pub mod bitpack;
pub mod cascade;
pub mod codec;
pub mod delta;
pub mod dictionary;
pub mod frame_of_reference;
pub mod rle;
pub mod transposed;

pub use codec::{Codec, Scheme};
