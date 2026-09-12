//! Shared codec interface.
//!
//! Every lossless integer codec in this crate implements [`Codec`]:
//! `decode(encode(x)) == x`. A runtime [`Scheme`] exists so the benchmark
//! harness (and later a cascade selector) can try several encodings without
//! duplicating call sites.
//!
//! Introduced at L2 because we now have more than one encoding; a trait that
//! existed at L1 would have been premature abstraction.

use crate::BitPackable;
use crate::{delta, dictionary, frame_of_reference, rle};

/// A lossless integer codec over [`BitPackable`] columns.
pub trait Codec {
    fn encode<T: BitPackable>(values: &[T]) -> Vec<u8>;
    fn decode<T: BitPackable>(bytes: &[u8]) -> Vec<T>;
}

/// Runtime dispatch over the L2 encodings. Used by the bench harness and
/// by tests that want to run the same invariant over every scheme.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scheme {
    FrameOfReference,
    Delta,
    RunLength,
    Dictionary,
}

impl Scheme {
    pub const ALL: [Scheme; 4] = [
        Scheme::FrameOfReference,
        Scheme::Delta,
        Scheme::RunLength,
        Scheme::Dictionary,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Scheme::FrameOfReference => "for",
            Scheme::Delta => "delta",
            Scheme::RunLength => "rle",
            Scheme::Dictionary => "dict",
        }
    }

    pub fn encode<T: BitPackable>(self, values: &[T]) -> Vec<u8> {
        match self {
            Scheme::FrameOfReference => frame_of_reference::encode(values),
            Scheme::Delta => delta::encode(values),
            Scheme::RunLength => rle::encode(values),
            Scheme::Dictionary => dictionary::encode(values),
        }
    }

    pub fn decode<T: BitPackable>(self, bytes: &[u8]) -> Vec<T> {
        match self {
            Scheme::FrameOfReference => frame_of_reference::decode(bytes),
            Scheme::Delta => delta::decode(bytes),
            Scheme::RunLength => rle::decode(bytes),
            Scheme::Dictionary => dictionary::decode(bytes),
        }
    }
}

pub struct FrameOfReference;
pub struct Delta;
pub struct RunLength;
pub struct Dictionary;

impl Codec for FrameOfReference {
    fn encode<T: BitPackable>(values: &[T]) -> Vec<u8> {
        frame_of_reference::encode(values)
    }
    fn decode<T: BitPackable>(bytes: &[u8]) -> Vec<T> {
        frame_of_reference::decode(bytes)
    }
}

impl Codec for Delta {
    fn encode<T: BitPackable>(values: &[T]) -> Vec<u8> {
        delta::encode(values)
    }
    fn decode<T: BitPackable>(bytes: &[u8]) -> Vec<T> {
        delta::decode(bytes)
    }
}

impl Codec for RunLength {
    fn encode<T: BitPackable>(values: &[T]) -> Vec<u8> {
        rle::encode(values)
    }
    fn decode<T: BitPackable>(bytes: &[u8]) -> Vec<T> {
        rle::decode(bytes)
    }
}

impl Codec for Dictionary {
    fn encode<T: BitPackable>(values: &[T]) -> Vec<u8> {
        dictionary::encode(values)
    }
    fn decode<T: BitPackable>(bytes: &[u8]) -> Vec<T> {
        dictionary::decode(bytes)
    }
}
