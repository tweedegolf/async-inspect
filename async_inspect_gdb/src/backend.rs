use anyhow::Result;

pub mod gdb_backend;

pub trait Backend {
    /// Get the paths to any known objectfiles.
    fn get_objectfiles(&mut self) -> Result<impl Iterator<Item = String>>;

    /// Sets a new breakpoint at the given function, returning a unique id used for events.
    fn set_breakpoint(&mut self, function_name: &str) -> Result<u64>;

    /// Resume executing code on the target. Do nothing if aleardy executing.
    fn resume(&mut self) -> Result<()>;

    /// Read `len` bytes at `addr`
    fn read_memory(&mut self, addr: u64, len: u64) -> Result<Vec<u8>>;
}
