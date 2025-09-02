use std::collections::HashMap;

use anyhow::Result;
use pyo3::{
    intern,
    prelude::*,
    types::{PyBytes, PyDict},
};

use embassy_inspect::{Callback, Type};

pub(crate) struct GdbCallback<'a, 'py> {
    py: Python<'py>,
    gdb: Bound<'py, PyModule>,
    main: Bound<'py, PyModule>,

    breakpoint_reg: &'a mut HashMap<u64, PyObject>,
}

impl<'a, 'py> GdbCallback<'a, 'py> {
    pub(crate) fn new(
        py: Python<'py>,
        breakpoint_reg: &'a mut HashMap<u64, PyObject>,
    ) -> PyResult<Self> {
        let gdb = py.import(intern!(py, "gdb"))?;
        let main = py.import(intern!(py, "__main__"))?;

        Ok(Self {
            py,
            gdb,
            main,

            breakpoint_reg,
        })
    }

    fn gdb_gdb_type(&self, ty: &Type) -> Option<Bound<'py, PyAny>> {
        let py = self.gdb.py();

        match ty {
            Type::Unknown => return None,
            Type::Void => self
                .gdb
                .call_method0(intern!(py, "selected_inferior"))
                .ok()?
                .call_method0(intern!(py, "architecture"))
                .ok()?
                .call_method0(intern!(py, "void_type"))
                .ok(),
            Type::Array { inner, count } => self
                .gdb_gdb_type(&inner)?
                .call_method1(intern!(py, "vector"), (0, *count - 1))
                .ok(),
            Type::Pointer(inner) => self
                .gdb_gdb_type(&inner)?
                .call_method0(intern!(py, "pointer"))
                .ok(),
            Type::Refrence(inner) => self
                .gdb_gdb_type(&inner)?
                .call_method0(intern!(py, "reference"))
                .ok(),
            Type::Base(name) => self
                .gdb
                .call_method1(intern!(py, "lookup_type"), (name,))
                .ok(),
        }
    }
}

impl<'a, 'py> Callback for GdbCallback<'a, 'py> {
    fn get_objectfiles(&mut self) -> Result<impl Iterator<Item = String>> {
        let py = self.py;

        Ok(self
            .gdb
            .call_method0(intern!(py, "objfiles"))?
            .try_iter()?
            .filter_map(move |py_str| {
                Some(
                    py_str
                        .ok()?
                        .getattr(intern!(py, "filename"))
                        .ok()?
                        .extract::<String>()
                        .ok()?,
                )
            }))
    }

    fn set_breakpoint(&mut self, addr: u64) -> Result<u64> {
        let py = self.py;

        let breakpoint = self.main.getattr(intern!(py, "PyO3Breakpoint"))?;
        let breakpoint_type = self.gdb.getattr(intern!(py, "BP_HARDWARE_BREAKPOINT"))?;

        let kwargs = PyDict::new(py);
        kwargs.set_item(intern!(py, "internal"), true)?;
        kwargs.set_item(intern!(py, "type"), breakpoint_type)?;

        let breakpoint = breakpoint.call((format!("*{addr}"),), Some(&kwargs))?;

        let id = breakpoint.hash()? as usize as u64;
        self.breakpoint_reg.insert(id, breakpoint.unbind());
        Ok(id)
    }

    fn resume(&mut self) -> Result<()> {
        let py = self.py;

        #[pyfunction]
        fn continue_lambda<'py>(py: Python<'py>) -> PyResult<()> {
            py.import(intern!(py, "gdb"))?
                .call_method1(intern!(py, "execute"), (intern!(py, "continue"),))?;
            Ok(())
        }
        let continue_lambda_object = wrap_pyfunction!(continue_lambda)(py)?;

        // Using post_event to not block the current thread with the continue command
        let _ = self
            .gdb
            .call_method1(intern!(py, "post_event"), (&continue_lambda_object,));

        Ok(())
    }

    fn read_memory(&mut self, addr: u64, len: u64) -> Result<Vec<u8>> {
        let py = self.py;

        let memory_view = self
            .gdb
            .call_method0(intern!(py, "selected_inferior"))?
            .call_method1(intern!(py, "read_memory"), (addr, len))?;

        let bytes = memory_view.call_method0(intern!(py, "tobytes"))?;
        let bytes = bytes.downcast::<PyBytes>().map_err(PyErr::from)?;
        let bytes = bytes.as_bytes().to_vec();

        Ok(bytes)
    }

    fn try_format_value(&mut self, bytes: &[u8], ty: &Type) -> Option<String> {
        let py = self.py;

        let gdb_type = self.gdb_gdb_type(ty)?;

        let value = self.gdb.getattr(intern!(py, "Value")).ok()?;
        let value = value.call1((bytes, gdb_type)).ok()?;

        let kwargs = PyDict::new(py);
        kwargs.set_item(intern!(py, "styling"), true).ok()?;

        let value = value
            .call_method(intern!(py, "format_string"), (), Some(&kwargs))
            .ok()?;
        value.extract().ok()
    }
}
