use anyhow::Result;

use crate::Type;

/// Trait with methods an [`EmbassyInspector`](crate::EmbassyInspector) can call.
///
/// A backend should have a single implementation of this trait, providing it when calling methods
/// on a [`EmbassyInspector`](crate::EmbassyInspector).
///
/// All methods are allowed to error, these errors will be passed back to the backend from the
/// methods on [`EmbassyInspector`](crate::EmbassyInspector).
pub trait Callback {
    /// Get the paths to any known objectfiles.
    fn get_objectfiles(&mut self) -> Result<impl Iterator<Item = String>>;

    /// Sets a new breakpoint at the given address, returning a unique id used for events.
    ///
    /// It is valid to just use the address as the id, but some backend allow setting multiple
    /// breakpoints on the same address, these could for example return a hash of the breakpoint
    /// object.
    fn set_breakpoint(&mut self, addr: u64) -> Result<u64>;

    /// Resume executing code on the target. Do nothing if already executing.
    ///
    /// Take care to implement this in a non blocking way.
    fn resume(&mut self) -> Result<()>;

    /// Read `len` bytes at `addr` from the target.
    fn read_memory(&mut self, addr: u64, len: u64) -> Result<Vec<u8>>;

    /// Try to format the given bytes as a type of the given name, this function if allowed to
    /// return `None` if the bytes are invalid or if this backend does not support formatting
    /// values.
    ///
    /// The returned string is allowed to contain ANSI escape codes for coloring.
    fn try_format_value(&mut self, bytes: &[u8], ty: &Type) -> Option<String>;
}
