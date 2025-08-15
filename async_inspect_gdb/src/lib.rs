pub mod backend;
pub mod embassy_inspector;
mod scroll_view;

use pyo3::prelude::*;

#[pymodule]
fn async_inspect_gdb(m: &Bound<'_, PyModule>) -> PyResult<()> {
    pyo3_log::init();

    m.add_class::<backend::gdb_backend::GdbTui>()
}
