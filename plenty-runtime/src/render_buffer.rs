//! Checked formatting sink. Formatting is completed before exposing any bytes.
use crate::memory::{self, AllocError};

#[derive(Default)]
pub(crate) struct Buffer {
    bytes: Vec<u8>,
    error: Option<AllocError>,
}
impl Buffer {
    pub fn failed(&self) -> bool {
        self.error.is_some()
    }
    pub fn push(&mut self, byte: u8) {
        self.extend_from_slice(&[byte]);
    }
    pub fn extend_from_slice(&mut self, bytes: &[u8]) {
        if self.failed() {
            return;
        }
        let result = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .ok_or(AllocError::CapacityOverflow)
            .and_then(memory::checked_capacity::<u8>)
            .and_then(|_| {
                self.bytes
                    .try_reserve(bytes.len())
                    .map_err(|_| AllocError::OutOfMemory)
            });
        match result {
            Ok(()) => self.bytes.extend_from_slice(bytes),
            Err(error) => self.error = Some(error),
        }
    }
    pub fn finish(self) -> Result<Vec<u8>, AllocError> {
        match self.error {
            Some(error) => Err(error),
            None => Ok(self.bytes),
        }
    }
}
impl std::io::Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        // Keep the original allocation error rather than allocating an I/O
        // diagnostic. Callers must finish() before using the buffered output.
        self.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
