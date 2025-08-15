use std::collections::HashMap;

use anyhow::Result;
use pyo3::{
    intern,
    prelude::*,
    types::{PyBytes, PyDict},
};

use crate::{
    backend::gdb_backend::gdb_ratatui_backend::GdbRatatuiBackend,
    embassy_inspector::{Click, EmbassyInspector, Event},
};

pub mod gdb_ratatui_backend;

#[pyclass]
pub struct GdbTui {
    inspector: EmbassyInspector<gdb_ratatui_backend::GdbRatatuiBackend>,

    breakpoint_reg: HashMap<u64, PyObject>,
}

struct GdbBackend<'a, 'py> {
    py: Python<'py>,
    gdb: Bound<'py, PyModule>,
    main: Bound<'py, PyModule>,

    breakpoint_reg: &'a mut HashMap<u64, PyObject>,
}

impl<'a, 'py> GdbBackend<'a, 'py> {
    fn new(py: Python<'py>, breakpoint_reg: &'a mut HashMap<u64, PyObject>) -> PyResult<Self> {
        let gdb = py.import(intern!(py, "gdb"))?;
        let main = py.import(intern!(py, "__main__"))?;

        Ok(Self {
            py,
            gdb,
            main,

            breakpoint_reg,
        })
    }
}

impl<'a, 'py> super::Backend for GdbBackend<'a, 'py> {
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

    fn try_format_value(&mut self, bytes: &[u8], type_name: &str) -> Option<String> {
        let py = self.py;

        let gdb_type = self
            .gdb
            .call_method1(intern!(py, "lookup_type"), (type_name,))
            .ok()?;

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

#[pymethods]
impl GdbTui {
    #[new]
    fn new(tui_window: PyObject, py: Python) -> PyResult<Py<Self>> {
        let ratatui_backend = GdbRatatuiBackend::new(tui_window, py)?;

        let mut breakpoint_reg = HashMap::new();

        let mut backend = GdbBackend::new(py, &mut breakpoint_reg)?;
        let mut inspector = EmbassyInspector::new(ratatui_backend, &mut backend)?;
        inspector.handle_event(Event::Redraw, &mut backend)?;

        let s = Bound::new(
            py,
            Self {
                inspector,
                breakpoint_reg,
            },
        )?;
        let stop_event_handler = s.getattr(intern!(py, "stop_event"))?;

        let gdb = py.import(intern!(py, "gdb"))?;
        gdb.getattr(intern!(py, "events"))?
            .getattr(intern!(py, "stop"))?
            .call_method1(intern!(py, "connect"), (stop_event_handler,))?;

        Ok(s.unbind())
    }

    /// When the TUI window is closed, the gdb.TuiWindow object will be put into an invalid state. At this time, GDB will call close method on the window object.
    /// After this method is called, GDB will discard any references it holds on this window object, and will no longer call methods on this object.
    fn close(&self) {}

    /// In some situations, a TUI window can change size. For example, this can happen if the user resizes the terminal, or changes the layout. When this happens, GDB will call the render method on the window object.
    /// If your window is intended to update in response to changes in the inferior, you will probably also want to register event listeners and send output to the gdb.TuiWindow.
    fn render(&mut self, py: Python) -> PyResult<()> {
        self.send_event(Event::Redraw, py)
    }

    /// This is a request to scroll the window horizontally. num is the amount by which to scroll, with negative numbers meaning to scroll right. In the TUI model, it is the viewport that moves, not the contents. A positive argument should cause the viewport to move right, and so the content should appear to move to the left.
    fn hscroll(&self, _num: i32) {}

    /// This is a request to scroll the window vertically. num is the amount by which to scroll, with negative numbers meaning to scroll backward. In the TUI model, it is the viewport that moves, not the contents. A positive argument should cause the viewport to move down, and so the content should appear to move up.
    fn vscroll(&mut self, num: i32, py: Python) -> PyResult<()> {
        self.send_event(Event::Scroll(num), py)
    }

    /// This is called on a mouse click in this window. x and y are the mouse coordinates inside the window (0-based, from the top left corner), and button specifies which mouse button was used, whose values can be 1 (left), 2 (middle), or 3 (right).
    /// When TUI mouse events are disabled by turning off the tui mouse-events setting (see set tui mouse-events), then click will not be called.
    fn click(&mut self, x: i32, y: i32, button: u8, py: Python) -> PyResult<()> {
        let button = match button {
            1 => crate::embassy_inspector::ClickButton::Left,
            2 => crate::embassy_inspector::ClickButton::Middle,
            3 => crate::embassy_inspector::ClickButton::Right,
            other => {
                log::error!("Unknown button id: {other}");
                return Ok(());
            }
        };
        let pos = ratatui::layout::Position::new(x as u16, y as u16);
        let click = Click { pos, button };

        self.send_event(Event::Click(click), py)
    }

    fn stop_event(&mut self, event: PyObject, py: Python) -> PyResult<()> {
        let mut events = Vec::new();

        {
            if let Ok(breakpoints) = event.getattr(py, intern!(py, "breakpoints")) {
                for breakpoint in breakpoints.bind(py).try_iter()?.flatten() {
                    for (id, reg_breakpoint) in &self.breakpoint_reg {
                        if breakpoint.eq(&reg_breakpoint.bind(py))? {
                            events.push(Event::Breakpoint(*id));
                        }
                    }
                }
            }
        }

        for event in events {
            self.send_event(event, py)?;
        }
        Ok(())
    }
}

impl GdbTui {
    fn send_event(&mut self, event: Event, py: Python) -> PyResult<()> {
        let mut backend = GdbBackend::new(py, &mut self.breakpoint_reg)?;
        self.inspector.handle_event(event, &mut backend)?;
        Ok(())
    }
}
