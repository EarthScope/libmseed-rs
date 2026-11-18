//! Bit flags: parsing control and record-level header flags.
//!
//! Packing, reading into trace lists and trace list construction are configured with
//! the option structs in [`crate::options`].
//!
//! ## C libmseed flags
//!
//! | C flag | Rust |
//! |---|---|
//! | `MSF_UNPACKDATA` | [`ParseFlags::UNPACK_DATA`] |
//! | `MSF_SKIPNOTDATA` | [`ParseFlags::SKIP_NOT_DATA`] |
//! | `MSF_VALIDATECRC` | [`ParseFlags::VALIDATE_CRC`] |
//! | `MSF_PNAMERANGE` | [`ReadOptions::path_range`], [`MsReader::open_path_range`] |
//! | `MSF_ATENDOFFILE` | [`ParseFlags::END_OF_INPUT`] |
//! | `MSF_SEQUENCE` | not supported |
//! | `MSF_FLUSHDATA` | [`PackOptions::flush`] |
//! | `MSF_PACKVER2` | [`PackOptions::format`] |
//! | `MSF_RECORDLIST` | [`TraceListOptions::record_list`] |
//! | `MSF_MAINTAINMSTL` | [`PackOptions::keep_data`] |
//! | `MSF_PPUPDATETIME` | [`TraceListOptions::update_time`] |
//! | `MSF_SPLITISVERSION` | [`Versions::Assign`] in [`TraceListOptions::versions`] |
//! | `MSF_SKIPADJACENTDUPLICATES` | [`ReadOptions::skip_adjacent_duplicates`] |
//! | `MSF_RECORDLIST_NOEXTRAS` | [`RecordList::WithoutExtras`] |
//! | none | [`ParseFlags::NO_RAW`] |
//!
//! [`ReadOptions::path_range`]: crate::ReadOptions::path_range
//! [`MsReader::open_path_range`]: crate::MsReader::open_path_range
//! [`PackOptions::flush`]: crate::PackOptions::flush
//! [`PackOptions::format`]: crate::PackOptions::format
//! [`PackOptions::keep_data`]: crate::PackOptions::keep_data
//! [`TraceListOptions::record_list`]: crate::TraceListOptions::record_list
//! [`TraceListOptions::update_time`]: crate::TraceListOptions::update_time
//! [`TraceListOptions::versions`]: crate::TraceListOptions::versions
//! [`Versions::Assign`]: crate::options::Versions::Assign
//! [`ReadOptions::skip_adjacent_duplicates`]: crate::ReadOptions::skip_adjacent_duplicates
//! [`RecordList::WithoutExtras`]: crate::options::RecordList::WithoutExtras

bitflags::bitflags! {
    /// Flags controlling how records are parsed, by [`crate::Record::parse`] and
    /// [`crate::MsReader`].
    ///
    /// A flag with a C libmseed equivalent has the C bit value; a flag without one uses a
    /// bit from 0x10000 up, clear of the C flags (see the [module docs](self)).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct ParseFlags: u32 {
        /// Decode data samples into [`crate::Record::samples`].
        const UNPACK_DATA = 0x0001;
        /// Skip input that cannot be identified as miniSEED instead of failing.
        const SKIP_NOT_DATA = 0x0002;
        /// Validate the CRC of miniSEED 3 records.
        const VALIDATE_CRC = 0x0004;
        /// The buffer ends at the end of the input. A miniSEED 2 record without a
        /// blockette 1000 then takes the length of the rest of the buffer, if that is a
        /// power of two. [`crate::MsReader`] sets this itself.
        const END_OF_INPUT = 0x0010;

        // No C equivalent: bits from 0x10000 up
        /// Do not retain the raw record bytes: [`crate::Record::raw`] is `None`. With
        /// [`ParseFlags::UNPACK_DATA`] the samples are decoded first. The record cannot
        /// be unpacked or repacked afterwards. Ignored when reading into a trace list.
        const NO_RAW = 0x10000;
    }
}

bitflags::bitflags! {
    /// Record-level flags of a record header ([`crate::Record::flags`]).
    ///
    /// Undefined bits are retained.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct RecordFlags: u8 {
        /// Calibration signals present.
        const CALIBRATION = 0x01;
        /// Time tag is questionable.
        const TIME_QUESTIONABLE = 0x02;
        /// Clock locked.
        const CLOCK_LOCKED = 0x04;
        // Keep undefined bits when parsing and repacking
        const _ = !0;
    }
}

#[cfg(test)]
mod tests {
    use super::ParseFlags;

    /// Bits used by the C libmseed `MSF_*` flags.
    const C_FLAG_BITS: u32 = 0xFFFF;

    /// Parse flags without a C equivalent.
    const RUST_ONLY: ParseFlags = ParseFlags::NO_RAW;

    #[test]
    fn c_flag_values() {
        assert_eq!(ParseFlags::UNPACK_DATA.bits(), 0x0001); // MSF_UNPACKDATA
        assert_eq!(ParseFlags::SKIP_NOT_DATA.bits(), 0x0002); // MSF_SKIPNOTDATA
        assert_eq!(ParseFlags::VALIDATE_CRC.bits(), 0x0004); // MSF_VALIDATECRC
        assert_eq!(ParseFlags::END_OF_INPUT.bits(), 0x0010); // MSF_ATENDOFFILE
    }

    #[test]
    fn rust_only_flags_clear_of_c_bits() {
        assert_eq!(RUST_ONLY.bits() & C_FLAG_BITS, 0);
        let c_equivalent = ParseFlags::all() - RUST_ONLY;
        assert_eq!(c_equivalent.bits() & !C_FLAG_BITS, 0);
    }
}
