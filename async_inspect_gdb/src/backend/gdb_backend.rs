use std::time::Instant;

use pyo3::{exceptions::PyTypeError, intern, prelude::*};
use ratatui::{buffer::Cell, style::Modifier};

use crate::{
    backend::gdb_backend::gdb_ratatui_backend::GdbRatatuiBackend,
    embassy_inspector::{EmbassyInspector, Event},
};

pub mod gdb_ratatui_backend;

#[pyclass]
pub struct GdbTui {
    inspector: EmbassyInspector<gdb_ratatui_backend::GdbRatatuiBackend>,
}

struct GdbBackend<'py> {
    gdb: Bound<'py, PyModule>,
}

impl<'py> GdbBackend<'py> {
    fn new(py: Python<'py>) -> PyResult<Self> {
        let gdb = py.import(intern!(py, "gdb"))?;

        Ok(Self { gdb })
    }
}

impl<'py> super::Backend for GdbBackend<'py> {}

#[pymethods]
impl GdbTui {
    #[new]
    fn new(tui_window: PyObject, py: Python) -> PyResult<Self> {
        let ratatui_backend = GdbRatatuiBackend::new(tui_window, py)?;
        let mut inspector = EmbassyInspector::new(ratatui_backend)?;

        let mut backend = GdbBackend::new(py)?;
        inspector.handle_event(Event::Redraw, &mut backend)?;

        Ok(Self { inspector })
    }

    /// When the TUI window is closed, the gdb.TuiWindow object will be put into an invalid state. At this time, GDB will call close method on the window object.
    /// After this method is called, GDB will discard any references it holds on this window object, and will no longer call methods on this object.
    fn close(&self) {}

    /// In some situations, a TUI window can change size. For example, this can happen if the user resizes the terminal, or changes the layout. When this happens, GDB will call the render method on the window object.
    /// If your window is intended to update in response to changes in the inferior, you will probably also want to register event listeners and send output to the gdb.TuiWindow.
    fn render(&mut self, py: Python) -> PyResult<()> {
        let mut backend = GdbBackend::new(py)?;
        self.inspector.handle_event(Event::Redraw, &mut backend)?;
        Ok(())
    }

    /// This is a request to scroll the window horizontally. num is the amount by which to scroll, with negative numbers meaning to scroll right. In the TUI model, it is the viewport that moves, not the contents. A positive argument should cause the viewport to move right, and so the content should appear to move to the left.
    fn hscroll(&self, num: i32) {}

    /// This is a request to scroll the window vertically. num is the amount by which to scroll, with negative numbers meaning to scroll backward. In the TUI model, it is the viewport that moves, not the contents. A positive argument should cause the viewport to move down, and so the content should appear to move up.
    fn vscroll(&self, num: i32) {}

    /// This is called on a mouse click in this window. x and y are the mouse coordinates inside the window (0-based, from the top left corner), and button specifies which mouse button was used, whose values can be 1 (left), 2 (middle), or 3 (right).
    /// When TUI mouse events are disabled by turning off the tui mouse-events setting (see set tui mouse-events), then click will not be called.
    fn click(&self, x: i32, y: i32, button: u8, py: Python) -> PyResult<()> {
        Ok(())
    }
}
