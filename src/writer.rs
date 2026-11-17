//! Writing records to files and streams.

use crate::error::{Error, Result};
use crate::options::PackOptions;
use crate::pack::Packed;
use crate::record::Record;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

impl Record {
    /// Pack this record and write each generated record to `out`.
    ///
    /// To append to a file, open it with [`std::fs::OpenOptions::append`] and pass it here.
    pub fn write_to(&self, mut out: impl Write, options: &PackOptions) -> Result<Packed> {
        self.pack_with(options, |rec| {
            out.write_all(rec).map_err(|e| Error::io(e, "Error writing to output"))
        })
    }

    /// Pack this record and write it to a file, which is created or replaced.
    pub fn write_file(&self, path: impl AsRef<Path>, options: &PackOptions) -> Result<Packed> {
        let path = path.as_ref();
        let mut out = create_output(path)?;
        let packed = self.write_to(&mut out, options)?;
        finish_output(out, path)?;
        Ok(packed)
    }
}

/// Create or replace a file for writing records.
pub(crate) fn create_output(path: &Path) -> Result<BufWriter<File>> {
    File::create(path)
        .map(BufWriter::new)
        .map_err(|e| Error::io(e, format_args!("Cannot open output file {}", path.display())))
}

/// Flush buffered output to the file.
pub(crate) fn finish_output(out: BufWriter<File>, path: &Path) -> Result<()> {
    out.into_inner().map(drop).map_err(|e| {
        Error::io(e.into_error(), format_args!("Error writing to output file {}", path.display()))
    })
}
