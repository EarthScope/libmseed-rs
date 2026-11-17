//! Data sample encoding and decoding.
//!
//! [`Samples`] holds decoded data samples, and [`ByteOrder`] is the byte order of a
//! record header or payload. Decoding and encoding are done internally when unpacking
//! and packing records.

mod decode;
pub(crate) mod encode;

pub(crate) use decode::decode;

use crate::error::{Error, Result};
use crate::lookup::SampleType;

fn type_mismatch(a: &Samples, b: &Samples) -> Error {
    Error::Samples(format!(
        "Sample types do not match ({:?} and {:?})",
        a.sample_type(),
        b.sample_type()
    ))
}

/// Byte order of multi-byte values in encoded data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    /// Least significant byte first.
    Little,
    /// Most significant byte first.
    Big,
}

impl ByteOrder {
    /// Byte order of the host.
    pub const NATIVE: ByteOrder =
        if cfg!(target_endian = "little") { ByteOrder::Little } else { ByteOrder::Big };

    pub(crate) fn u16(self, b: &[u8]) -> u16 {
        let a = [b[0], b[1]];
        match self {
            ByteOrder::Little => u16::from_le_bytes(a),
            ByteOrder::Big => u16::from_be_bytes(a),
        }
    }
    pub(crate) fn u32(self, b: &[u8]) -> u32 {
        let a = [b[0], b[1], b[2], b[3]];
        match self {
            ByteOrder::Little => u32::from_le_bytes(a),
            ByteOrder::Big => u32::from_be_bytes(a),
        }
    }
    pub(crate) fn u64(self, b: &[u8]) -> u64 {
        let a = [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
        match self {
            ByteOrder::Little => u64::from_le_bytes(a),
            ByteOrder::Big => u64::from_be_bytes(a),
        }
    }
}

/// Decoded data samples, in host form.
#[derive(Debug, Clone, PartialEq)]
pub enum Samples {
    /// Text (UTF-8) bytes.
    Text(Vec<u8>),
    /// 32-bit integers.
    Int32(Vec<i32>),
    /// 32-bit floats.
    Float32(Vec<f32>),
    /// 64-bit floats.
    Float64(Vec<f64>),
}

impl Samples {
    /// Number of samples.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Samples::Text(v) => v.len(),
            Samples::Int32(v) => v.len(),
            Samples::Float32(v) => v.len(),
            Samples::Float64(v) => v.len(),
        }
    }

    /// True if there are no samples.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The [`SampleType`] of the samples.
    #[must_use]
    pub fn sample_type(&self) -> SampleType {
        match self {
            Samples::Text(_) => SampleType::Text,
            Samples::Int32(_) => SampleType::Int32,
            Samples::Float32(_) => SampleType::Float32,
            Samples::Float64(_) => SampleType::Float64,
        }
    }

    /// An empty container of the given type.
    #[must_use]
    pub fn empty(t: SampleType) -> Samples {
        match t {
            SampleType::Text => Samples::Text(Vec::new()),
            SampleType::Int32 => Samples::Int32(Vec::new()),
            SampleType::Float32 => Samples::Float32(Vec::new()),
            SampleType::Float64 => Samples::Float64(Vec::new()),
        }
    }

    /// Text bytes, if text samples.
    #[must_use]
    pub fn as_text(&self) -> Option<&[u8]> {
        if let Samples::Text(v) = self { Some(v) } else { None }
    }
    /// Integer samples, if 32-bit integers.
    #[must_use]
    pub fn as_i32(&self) -> Option<&[i32]> {
        if let Samples::Int32(v) = self { Some(v) } else { None }
    }
    /// Float samples, if 32-bit floats.
    #[must_use]
    pub fn as_f32(&self) -> Option<&[f32]> {
        if let Samples::Float32(v) = self { Some(v) } else { None }
    }
    /// Double samples, if 64-bit floats.
    #[must_use]
    pub fn as_f64(&self) -> Option<&[f64]> {
        if let Samples::Float64(v) = self { Some(v) } else { None }
    }

    /// Remove and drop the first `n` samples.
    ///
    /// # Panics
    ///
    /// Panics if `n` is greater than [`Samples::len`].
    pub fn drain_front(&mut self, n: usize) {
        match self {
            Samples::Text(v) => drop(v.drain(..n)),
            Samples::Int32(v) => drop(v.drain(..n)),
            Samples::Float32(v) => drop(v.drain(..n)),
            Samples::Float64(v) => drop(v.drain(..n)),
        }
    }

    /// Insert samples of the same type at the front.
    ///
    /// Fails with [`Error::Samples`] if the sample types differ.
    pub fn prepend(&mut self, other: &Samples) -> Result<()> {
        fn pre<T: Clone>(a: &mut Vec<T>, b: &[T]) {
            let mut v = Vec::with_capacity(a.len() + b.len());
            v.extend_from_slice(b);
            v.extend_from_slice(a);
            *a = v;
        }
        match (self, other) {
            (Samples::Text(a), Samples::Text(b)) => pre(a, b),
            (Samples::Int32(a), Samples::Int32(b)) => pre(a, b),
            (Samples::Float32(a), Samples::Float32(b)) => pre(a, b),
            (Samples::Float64(a), Samples::Float64(b)) => pre(a, b),
            (a, b) => return Err(type_mismatch(a, b)),
        }
        Ok(())
    }

    /// Append samples of the same type.
    ///
    /// Fails with [`Error::Samples`], appending nothing, if the sample types differ.
    pub fn extend_from(&mut self, other: &Samples) -> Result<()> {
        match (self, other) {
            (Samples::Text(a), Samples::Text(b)) => a.extend_from_slice(b),
            (Samples::Int32(a), Samples::Int32(b)) => a.extend_from_slice(b),
            (Samples::Float32(a), Samples::Float32(b)) => a.extend_from_slice(b),
            (Samples::Float64(a), Samples::Float64(b)) => a.extend_from_slice(b),
            (a, b) => return Err(type_mismatch(a, b)),
        }
        Ok(())
    }
}
