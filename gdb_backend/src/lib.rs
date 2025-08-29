pub(crate) mod callback;
pub(crate) mod ratatui_backend;

use std::collections::HashMap;

use pyo3::{intern, prelude::*};

use embassy_inspect::{Click, EmbassyInspector, Event};

use callback::GdbCallback;
use ratatui_backend::GdbRatatuiBackend;

#[pymodule]
fn gdb_backend(m: &Bound<'_, PyModule>) -> PyResult<()> {
    pyo3_log::init();

    m.add_class::<GdbTui>()
}

#[pyclass]
pub struct GdbTui {
    inspector: EmbassyInspector<GdbRatatuiBackend>,

    breakpoint_reg: HashMap<u64, PyObject>,
}

#[pymethods]
impl GdbTui {
    #[new]
    fn new(tui_window: PyObject, py: Python) -> PyResult<Py<Self>> {
        let ratatui_backend = GdbRatatuiBackend::new(tui_window, py)?;

        let mut breakpoint_reg = HashMap::new();

        let mut callback = GdbCallback::new(py, &mut breakpoint_reg)?;
        let mut inspector = EmbassyInspector::new(ratatui_backend, &mut callback)?;
        inspector.handle_event(Event::Redraw, &mut callback)?;

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
            1 => embassy_inspect::ClickButton::Left,
            2 => embassy_inspect::ClickButton::Middle,
            3 => embassy_inspect::ClickButton::Right,
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

        if let Ok(breakpoints) = event.getattr(py, intern!(py, "breakpoints")) {
            for breakpoint in breakpoints.bind(py).try_iter()?.flatten() {
                for (id, reg_breakpoint) in &self.breakpoint_reg {
                    if breakpoint.eq(&reg_breakpoint.bind(py))? {
                        events.push(Event::Breakpoint(*id));
                    }
                }
            }
        }
        if events.is_empty() {
            events.push(Event::Stoped);
        }

        for event in events {
            self.send_event(event, py)?;
        }
        Ok(())
    }
}

impl GdbTui {
    fn send_event(&mut self, event: Event, py: Python) -> PyResult<()> {
        let mut callback = GdbCallback::new(py, &mut self.breakpoint_reg)?;
        self.inspector.handle_event(event, &mut callback)?;
        Ok(())
    }
}
