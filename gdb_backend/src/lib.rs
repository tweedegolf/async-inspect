pub mod backend;
pub mod ratatui_backend;

use pyo3::prelude::*;

#[pymodule]
fn gdb_backend(m: &Bound<'_, PyModule>) -> PyResult<()> {
    pyo3_log::init();

    m.add_class::<backend::GdbTui>()
}
