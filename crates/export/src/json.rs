use crate::Options;
use serde::Serialize;
use std::io::{self, Write};

/// Serializes borrowed metadata directly, bounding writes before allocation and observing cancellation.
pub(crate) fn encode(
    value: &impl Serialize,
    options: &Options<'_>,
    overhead: usize,
) -> Result<Vec<u8>, String> {
    struct Output<'a, 'cancel> {
        bytes: Vec<u8>,
        options: &'a Options<'cancel>,
        overhead: usize,
    }
    impl Write for Output<'_, '_> {
        fn write(&mut self, input: &[u8]) -> io::Result<usize> {
            let total = self
                .overhead
                .checked_add(self.bytes.len())
                .and_then(|n| n.checked_add(input.len()))
                .ok_or_else(|| io::Error::other("Export size overflow."))?;
            self.options.check(total).map_err(io::Error::other)?;
            let needed = self.bytes.len() + input.len();
            if needed > self.bytes.capacity() {
                let limit = self.options.maximum_bytes.min(u32::MAX as usize) - self.overhead;
                let capacity = self
                    .bytes
                    .capacity()
                    .max(256)
                    .saturating_mul(2)
                    .min(limit)
                    .max(needed);
                self.bytes
                    .try_reserve_exact(capacity - self.bytes.len())
                    .map_err(io::Error::other)?;
            }
            self.bytes.extend_from_slice(input);
            Ok(input.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut output = Output {
        bytes: vec![],
        options,
        overhead,
    };
    serde_json::to_writer(&mut output, value).map_err(|e| e.to_string())?;
    Ok(output.bytes)
}
