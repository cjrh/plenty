//! Checked formatting sink. Formatting is completed before exposing any bytes.
use crate::memory::{self, AllocError};

#[derive(Default)]
pub(crate) struct Buffer {
    bytes: Vec<u8>,
    error: Option<AllocError>,
    /// Where a failed `display` or `debug` method's `Result` goes. Only
    /// `print` and `str.repr` pass one, so other renderings, such as `main`'s
    /// error report, never run user methods.
    hooks: Option<*mut u128>,
    hook_failed: bool,
}
impl Buffer {
    /// A buffer whose rendering calls the types' formatting adapters.
    pub fn hooked(cell: *mut u128) -> Self {
        Self {
            hooks: (!cell.is_null()).then_some(cell),
            ..Self::default()
        }
    }
    pub fn hooks(&self) -> bool {
        self.hooks.is_some()
    }
    pub fn failed(&self) -> bool {
        self.error.is_some() || self.hook_failed
    }
    /// Whether a formatting method failed; its error is in the cell.
    pub fn hook_failed(&self) -> bool {
        self.hook_failed
    }
    /// Stop formatting after a method returned `result`, an `Err`.
    pub fn fail_hook(&mut self, result: u128) {
        if let Some(cell) = self.hooks {
            // SAFETY: the caller's cell outlives the rendering.
            unsafe { cell.write(result) };
        }
        self.hook_failed = true;
    }
    pub fn fail_alloc(&mut self, error: AllocError) {
        self.error.get_or_insert(error);
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
