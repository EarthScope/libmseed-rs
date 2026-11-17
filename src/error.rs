//! Error type for the crate.

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by library functions.
///
/// Variants say what kind of problem occurred; the message says where and why, usually
/// starting with the source identifier of the record concerned.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The data is not miniSEED.
    #[error("not miniSEED: {0}")]
    NotSeed(String),
    /// A record length is outside the supported range.
    #[error("record length out of range: {0}")]
    OutOfRange(String),
    /// The input ended before a complete record was available.
    #[error("incomplete record, {needed} more bytes needed")]
    Incomplete {
        /// The number of additional bytes needed, at least.
        needed: usize,
    },
    /// A record's CRC does not match its contents.
    #[error("invalid CRC: {0}")]
    InvalidCrc(String),
    /// A record is malformed: inconsistent header values, blockettes or data payload.
    #[error("invalid record: {0}")]
    InvalidRecord(String),
    /// A data encoding is not supported for the operation.
    #[error("unsupported encoding: {0}")]
    UnsupportedEncoding(String),
    /// Steim compressed data is invalid.
    #[error("invalid Steim data: {0}")]
    Steim(String),
    /// Data samples cannot be decoded, converted or combined as requested.
    #[error("sample data: {0}")]
    Samples(String),
    /// Records cannot be packed from the given record or trace list.
    #[error("cannot pack: {0}")]
    Pack(String),
    /// A time is invalid or cannot be represented.
    #[error("invalid time: {0}")]
    InvalidTime(String),
    /// A source identifier is invalid or cannot be converted.
    #[error("invalid source identifier: {0}")]
    InvalidSourceId(String),
    /// Extra headers are invalid JSON, too long, or a JSON Pointer operation failed.
    #[error("extra headers: {0}")]
    ExtraHeaders(String),
    /// A data selection is invalid.
    #[error("invalid selection: {0}")]
    Selection(String),
    /// An argument or object state does not allow the operation.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    /// An I/O error, with context added to its message.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// An error from caller code, such as a record handler; see [`Error::other`].
    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl Error {
    /// Wrap an error from caller code, for example to fail from a record handler:
    ///
    /// ```
    /// let err = libmseed::Error::other("output is full");
    /// assert_eq!(err.to_string(), "output is full");
    /// ```
    pub fn other(err: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        Error::Other(err.into())
    }

    /// An [`Error::Io`] keeping the [`std::io::ErrorKind`] of `err` and adding `context`
    /// to its message.
    pub(crate) fn io(err: std::io::Error, context: impl std::fmt::Display) -> Self {
        Error::Io(std::io::Error::new(err.kind(), format!("{context}: {err}")))
    }

    /// The same error with `context` prefixed to its message.
    pub(crate) fn context(self, context: impl std::fmt::Display) -> Self {
        use Error as E;
        let add = |m: String| format!("{context}: {m}");
        match self {
            E::NotSeed(m) => E::NotSeed(add(m)),
            E::OutOfRange(m) => E::OutOfRange(add(m)),
            E::InvalidCrc(m) => E::InvalidCrc(add(m)),
            E::InvalidRecord(m) => E::InvalidRecord(add(m)),
            E::UnsupportedEncoding(m) => E::UnsupportedEncoding(add(m)),
            E::Steim(m) => E::Steim(add(m)),
            E::Samples(m) => E::Samples(add(m)),
            E::Pack(m) => E::Pack(add(m)),
            E::InvalidTime(m) => E::InvalidTime(add(m)),
            E::InvalidSourceId(m) => E::InvalidSourceId(add(m)),
            E::ExtraHeaders(m) => E::ExtraHeaders(add(m)),
            E::Selection(m) => E::Selection(add(m)),
            E::InvalidArgument(m) => E::InvalidArgument(add(m)),
            E::Io(e) => Error::io(e, context),
            e @ (E::Incomplete { .. } | E::Other(_)) => e,
        }
    }
}
