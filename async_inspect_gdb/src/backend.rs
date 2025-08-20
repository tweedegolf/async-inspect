use anyhow::Result;

use crate::embassy_inspector::Type;

pub mod gdb_backend;

pub trait Backend {
    /// Get the paths to any known objectfiles.
    fn get_objectfiles(&mut self) -> Result<impl Iterator<Item = String>>;

    /// Sets a new breakpoint at the given address, returning a unique id used for events.
    fn set_breakpoint(&mut self, addr: u64) -> Result<u64>;

    /// Resume executing code on the target. Do nothing if aleardy executing.
    fn resume(&mut self) -> Result<()>;

    /// Read `len` bytes at `addr`
    fn read_memory(&mut self, addr: u64, len: u64) -> Result<Vec<u8>>;

    /// Try to format the given bytes as a type of the given name, this function if allowd to return
    /// `None` if the bytes are invalid or if this backend does not support formatting values.
    ///
    /// The returned string is allowed to contain ansi escape codes for coloring.
    fn try_format_value(&mut self, bytes: &[u8], ty: &Type) -> Option<String>;
}
