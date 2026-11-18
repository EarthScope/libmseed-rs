//! Bit flags: parsing control and record-level header flags.
//!
//! Packing, reading into trace lists and trace list construction are configured with
//! the option structs in [`crate::options`].

bitflags::bitflags! {
    /// Flags controlling how records are parsed, by [`crate::Record::parse`] and
    /// [`crate::MsReader`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct ParseFlags: u32 {
        /// Decode data samples into [`crate::Record::samples`].
        const UNPACK_DATA = 0x0001;
        /// Skip input that cannot be identified as miniSEED instead of failing.
        const SKIP_NOT_DATA = 0x0002;
        /// Validate the CRC of miniSEED 3 records.
        const VALIDATE_CRC = 0x0004;
        /// Do not retain the raw record bytes: [`crate::Record::raw`] is `None`. With
        /// [`ParseFlags::UNPACK_DATA`] the samples are decoded first. The record cannot
        /// be unpacked or repacked afterwards. Ignored when reading into a trace list.
        const NO_RAW = 0x0008;
        /// The buffer ends at the end of the input. A miniSEED 2 record without a
        /// blockette 1000 then takes the length of the rest of the buffer, if that is a
        /// power of two. [`crate::MsReader`] sets this itself.
        const END_OF_INPUT = 0x0010;
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
