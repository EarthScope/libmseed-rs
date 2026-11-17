//! Data encodings and sample types.

use std::fmt;

/// A miniSEED data encoding value.
///
/// Any numeric value can be represented so that unknown encodings read from data
/// can be reported; use [`Encoding::sample_type`] to test for support.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Encoding(pub u8);

impl Encoding {
    /// Text encoding (UTF-8).
    pub const TEXT: Encoding = Encoding(0);
    /// 16-bit integer.
    pub const INT16: Encoding = Encoding(1);
    /// 32-bit integer.
    pub const INT32: Encoding = Encoding(3);
    /// 32-bit float (IEEE).
    pub const FLOAT32: Encoding = Encoding(4);
    /// 64-bit float (IEEE).
    pub const FLOAT64: Encoding = Encoding(5);
    /// Steim-1 compressed integers.
    pub const STEIM1: Encoding = Encoding(10);
    /// Steim-2 compressed integers.
    pub const STEIM2: Encoding = Encoding(11);
    /// \[Legacy\] GEOSCOPE 24-bit integer.
    pub const GEOSCOPE24: Encoding = Encoding(12);
    /// \[Legacy\] GEOSCOPE 16-bit gain ranged, 3-bit exponent.
    pub const GEOSCOPE163: Encoding = Encoding(13);
    /// \[Legacy\] GEOSCOPE 16-bit gain ranged, 4-bit exponent.
    pub const GEOSCOPE164: Encoding = Encoding(14);
    /// \[Legacy\] CDSN 16-bit gain ranged.
    pub const CDSN: Encoding = Encoding(16);
    /// \[Legacy\] SRO 16-bit gain ranged.
    pub const SRO: Encoding = Encoding(30);
    /// \[Legacy\] DWWSSN 16-bit gain ranged.
    pub const DWWSSN: Encoding = Encoding(32);

    /// Decoded sample type for this encoding, `None` if unsupported.
    #[must_use]
    pub fn sample_type(self) -> Option<SampleType> {
        match self {
            Self::TEXT => Some(SampleType::Text),
            Self::INT16
            | Self::INT32
            | Self::STEIM1
            | Self::STEIM2
            | Self::CDSN
            | Self::SRO
            | Self::DWWSSN => Some(SampleType::Int32),
            Self::FLOAT32 | Self::GEOSCOPE24 | Self::GEOSCOPE163 | Self::GEOSCOPE164 => {
                Some(SampleType::Float32)
            }
            Self::FLOAT64 => Some(SampleType::Float64),
            _ => None,
        }
    }

    /// Descriptive string.
    #[must_use]
    pub fn description(self) -> &'static str {
        match self.0 {
            0 => "Text",
            1 => "16-bit integer",
            2 => "24-bit integer",
            3 => "32-bit integer",
            4 => "32-bit float (IEEE single)",
            5 => "64-bit float (IEEE double)",
            10 => "STEIM-1 integer compression",
            11 => "STEIM-2 integer compression",
            12 => "GEOSCOPE Muxed 24-bit integer",
            13 => "GEOSCOPE Muxed 16/3-bit gain/exp",
            14 => "GEOSCOPE Muxed 16/4-bit gain/exp",
            15 => "US National Network compression",
            16 => "CDSN 16-bit gain ranged",
            17 => "Graefenberg 16-bit gain ranged",
            18 => "IPG - Strasbourg 16-bit gain",
            19 => "STEIM-3 integer compression",
            30 => "SRO gain ranged",
            31 => "HGLP",
            32 => "DWWSSN",
            33 => "RSTN 16 bit gain ranged",
            _ => "Unknown",
        }
    }
}

impl fmt::Debug for Encoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Encoding({}: {})", self.0, self.description())
    }
}

impl fmt::Display for Encoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.description())
    }
}

/// Type of decoded data samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampleType {
    /// Text (`'t'`), 1 byte per sample.
    Text,
    /// 32-bit integer (`'i'`).
    Int32,
    /// 32-bit float (`'f'`).
    Float32,
    /// 64-bit float (`'d'`).
    Float64,
}

impl SampleType {
    /// Size in bytes of one sample.
    #[must_use]
    pub fn size(self) -> usize {
        match self {
            SampleType::Text => 1,
            SampleType::Int32 | SampleType::Float32 => 4,
            SampleType::Float64 => 8,
        }
    }

    /// The libmseed single-character type code: `t`, `i`, `f` or `d`.
    #[must_use]
    pub fn code(self) -> char {
        match self {
            SampleType::Text => 't',
            SampleType::Int32 => 'i',
            SampleType::Float32 => 'f',
            SampleType::Float64 => 'd',
        }
    }

    /// Parse a libmseed sample type code (`'a'` is accepted as text).
    #[must_use]
    pub fn from_code(c: char) -> Option<Self> {
        match c {
            't' | 'a' => Some(SampleType::Text),
            'i' => Some(SampleType::Int32),
            'f' => Some(SampleType::Float32),
            'd' => Some(SampleType::Float64),
            _ => None,
        }
    }
}
