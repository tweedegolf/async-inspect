use pyo3::{exceptions::PyTypeError, intern, prelude::*};
use ratatui::{buffer::Cell, style::Modifier};

/// Wrapper around the gdb provided TuiWindow class
struct TuiWindow(PyObject);

impl TuiWindow {
    fn new(obj: PyObject, py: Python) -> PyResult<Self> {
        let tui_window_py_type = py
            .import(intern!(py, "gdb"))?
            .getattr(intern!(py, "TuiWindow"))?;

        if !obj.bind(py).is_instance(&tui_window_py_type)? {
            return Err(PyTypeError::new_err("Excpected TuiWindow").into());
        }

        Ok(Self(obj))
    }

    //// Get the width and height in characters of the window.
    #[expect(dead_code)]
    fn get_size(&self, py: Python) -> PyResult<(u32, u32)> {
        let width = self.0.getattr(py, intern!(py, "width"))?.extract(py)?;
        let height = self.0.getattr(py, intern!(py, "height"))?.extract(py)?;
        Ok((width, height))
    }

    /// Set the attribute that holds the window’s title with a string. This is normally displayed
    /// above the window
    #[expect(dead_code)]
    fn set_title(&self, title: &str, py: Python) -> PyResult<()> {
        self.0.setattr(py, intern!(py, "title"), title)
    }

    /// get the attribute that holds the window’s title that is normally displayed above the window.
    #[expect(dead_code)]
    fn get_title(&self, py: Python) -> PyResult<String> {
        self.0.getattr(py, intern!(py, "title"))?.extract(py)
    }

    /// Write `s` to the window. string can contain ANSI terminal escape styling sequences; GDB
    /// will translate these as appropriate for the terminal. The string should contains the full
    /// contents of the window.
    fn write(&self, s: &str, py: Python) -> PyResult<()> {
        self.0.call_method1(py, intern!(py, "write"), (s, true))?;

        Ok(())
    }
}

pub struct GdbRatatuiBackend {
    tui_window: TuiWindow,

    // The gdb tui does not suppert move ansi sequences so we have store our own buffer to be able
    // to support the ratatui api.
    // Using 2 vectors to more easaly support resizing.
    buffer: Vec<Vec<ratatui::buffer::Cell>>,
    cursor_pos: ratatui::layout::Position,
}
impl GdbRatatuiBackend {
    pub(crate) fn new(tui_window: PyObject, py: Python) -> PyResult<Self> {
        let tui_window = TuiWindow::new(tui_window, py)?;

        Ok(Self {
            tui_window,
            buffer: Vec::new(),
            cursor_pos: ratatui::layout::Position::ORIGIN,
        })
    }
}

fn py_error_to_io_error(py_err: PyErr) -> std::io::Error {
    std::io::Error::other(py_err)
}

impl ratatui::backend::Backend for GdbRatatuiBackend {
    fn draw<'a, I>(&mut self, content: I) -> std::io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
    {
        let size = self.size()?;

        self.buffer.resize(size.height as usize, Vec::new());
        for row in &mut self.buffer {
            row.resize(size.width as usize, Cell::EMPTY);
        }

        for (x, y, new_cell) in content {
            self.buffer[y as usize][x as usize].clone_from(new_cell);
        }

        Ok(())
    }

    fn hide_cursor(&mut self) -> std::io::Result<()> {
        // Not supported by GDB
        Ok(())
    }

    fn show_cursor(&mut self) -> std::io::Result<()> {
        // Not supported by GDB
        Ok(())
    }

    fn get_cursor_position(&mut self) -> std::io::Result<ratatui::prelude::Position> {
        // Not supported by GDB
        Ok(self.cursor_pos)
    }

    fn set_cursor_position<P: Into<ratatui::prelude::Position>>(
        &mut self,
        position: P,
    ) -> std::io::Result<()> {
        self.cursor_pos = position.into();
        Ok(())
    }

    fn clear(&mut self) -> std::io::Result<()> {
        for row in &mut self.buffer {
            row.fill(Cell::EMPTY);
        }
        Ok(())
    }

    fn size(&self) -> std::io::Result<ratatui::prelude::Size> {
        let (width, height) =
            Python::with_gil(|py| self.tui_window.get_size(py)).map_err(py_error_to_io_error)?;

        Ok(ratatui::prelude::Size::new(width as u16, height as u16))
    }

    fn window_size(&mut self) -> std::io::Result<ratatui::backend::WindowSize> {
        // This function seems to be never called by ratatui so its fine to return unsupported.
        Err(std::io::ErrorKind::Unsupported.into())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        use std::fmt::Write;

        // + 5 New line and ansi reset
        let line_length = self.buffer.first().map_or(0, Vec::len) + 5;
        let mut s = String::with_capacity(
            self.buffer.len() * line_length + 100, // Some extra space for the ansi escape codes.
        );

        for row in &self.buffer {
            write!(
                s,
                "{}{}{}",
                termion::color::Fg(termion::color::Reset),
                termion::color::Bg(termion::color::Reset),
                termion::style::Reset,
            )
            .unwrap();

            let mut modifier = ratatui::style::Modifier::empty();
            let mut fg = ratatui::style::Color::Reset;
            let mut bg = ratatui::style::Color::Reset;

            for cell in row {
                write!(
                    s,
                    "{}",
                    ModifierDiff {
                        from: modifier,
                        to: cell.modifier
                    }
                )
                .unwrap();
                modifier = cell.modifier;

                if cell.fg != fg {
                    write_color_fg(&mut s, &cell.fg);
                    fg = cell.fg;
                }

                if cell.bg != bg {
                    write_color_bg(&mut s, &cell.bg);
                    bg = cell.bg;
                }

                s.push_str(cell.symbol());
            }
        }

        Python::with_gil(|py| self.tui_window.write(&s, py)).map_err(py_error_to_io_error)?;

        Ok(())
    }
}

fn write_color_fg(s: &mut String, color: &ratatui::style::Color) {
    use ratatui::style::Color;
    use std::fmt::Write;
    use termion::color::Fg;

    match color {
        Color::Reset => write!(s, "{}", Fg(termion::color::Reset)),
        Color::Black => write!(s, "{}", Fg(termion::color::Black)),
        Color::Red => write!(s, "{}", Fg(termion::color::Red)),
        Color::Green => write!(s, "{}", Fg(termion::color::Green)),
        Color::Yellow => write!(s, "{}", Fg(termion::color::Yellow)),
        Color::Blue => write!(s, "{}", Fg(termion::color::Blue)),
        Color::Magenta => write!(s, "{}", Fg(termion::color::Magenta)),
        Color::Cyan => write!(s, "{}", Fg(termion::color::Cyan)),
        Color::Gray => write!(s, "{}", Fg(termion::color::White)),
        Color::DarkGray => write!(s, "{}", Fg(termion::color::LightBlack)),
        Color::LightRed => write!(s, "{}", Fg(termion::color::LightRed)),
        Color::LightGreen => write!(s, "{}", Fg(termion::color::LightGreen)),
        Color::LightYellow => write!(s, "{}", Fg(termion::color::LightYellow)),
        Color::LightBlue => write!(s, "{}", Fg(termion::color::LightBlue)),
        Color::LightMagenta => write!(s, "{}", Fg(termion::color::LightMagenta)),
        Color::LightCyan => write!(s, "{}", Fg(termion::color::LightCyan)),
        Color::White => write!(s, "{}", Fg(termion::color::White)),
        Color::Rgb(r, g, b) => write!(s, "{}", Fg(termion::color::Rgb(*r, *g, *b))),
        Color::Indexed(i) => write!(s, "{}", Fg(termion::color::AnsiValue(*i))),
    }
    .unwrap();
}

fn write_color_bg(s: &mut String, color: &ratatui::style::Color) {
    use ratatui::style::Color;
    use std::fmt::Write;
    use termion::color::Bg;

    match color {
        Color::Reset => write!(s, "{}", Bg(termion::color::Reset)),
        Color::Black => write!(s, "{}", Bg(termion::color::Black)),
        Color::Red => write!(s, "{}", Bg(termion::color::Red)),
        Color::Green => write!(s, "{}", Bg(termion::color::Green)),
        Color::Yellow => write!(s, "{}", Bg(termion::color::Yellow)),
        Color::Blue => write!(s, "{}", Bg(termion::color::Blue)),
        Color::Magenta => write!(s, "{}", Bg(termion::color::Magenta)),
        Color::Cyan => write!(s, "{}", Bg(termion::color::Cyan)),
        Color::Gray => write!(s, "{}", Bg(termion::color::White)),
        Color::DarkGray => write!(s, "{}", Bg(termion::color::LightBlack)),
        Color::LightRed => write!(s, "{}", Bg(termion::color::LightRed)),
        Color::LightGreen => write!(s, "{}", Bg(termion::color::LightGreen)),
        Color::LightYellow => write!(s, "{}", Bg(termion::color::LightYellow)),
        Color::LightBlue => write!(s, "{}", Bg(termion::color::LightBlue)),
        Color::LightMagenta => write!(s, "{}", Bg(termion::color::LightMagenta)),
        Color::LightCyan => write!(s, "{}", Bg(termion::color::LightCyan)),
        Color::White => write!(s, "{}", Bg(termion::color::White)),
        Color::Rgb(r, g, b) => write!(s, "{}", Bg(termion::color::Rgb(*r, *g, *b))),
        Color::Indexed(i) => write!(s, "{}", Bg(termion::color::AnsiValue(*i))),
    }
    .unwrap();
}

// Taken from the [`ratatui::TermionBackend`] implementation
struct ModifierDiff {
    from: Modifier,
    to: Modifier,
}

impl std::fmt::Display for ModifierDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let remove = self.from - self.to;
        if remove.contains(Modifier::REVERSED) {
            write!(f, "{}", termion::style::NoInvert)?;
        }
        if remove.contains(Modifier::BOLD) {
            // XXX: the termion NoBold flag actually enables double-underline on ECMA-48 compliant
            // terminals, and NoFaint additionally disables bold... so we use this trick to get
            // the right semantics.
            write!(f, "{}", termion::style::NoFaint)?;
            if self.to.contains(Modifier::DIM) {
                write!(f, "{}", termion::style::Faint)?;
            }
        }
        if remove.contains(Modifier::ITALIC) {
            write!(f, "{}", termion::style::NoItalic)?;
        }
        if remove.contains(Modifier::UNDERLINED) {
            write!(f, "{}", termion::style::NoUnderline)?;
        }
        if remove.contains(Modifier::DIM) {
            write!(f, "{}", termion::style::NoFaint)?;
            // XXX: the NoFaint flag additionally disables bold as well, so we need to re-enable it
            // here if we want it.
            if self.to.contains(Modifier::BOLD) {
                write!(f, "{}", termion::style::Bold)?;
            }
        }
        if remove.contains(Modifier::CROSSED_OUT) {
            write!(f, "{}", termion::style::NoCrossedOut)?;
        }
        if remove.contains(Modifier::SLOW_BLINK) || remove.contains(Modifier::RAPID_BLINK) {
            write!(f, "{}", termion::style::NoBlink)?;
        }
        let add = self.to - self.from;
        if add.contains(Modifier::REVERSED) {
            write!(f, "{}", termion::style::Invert)?;
        }
        if add.contains(Modifier::BOLD) {
            write!(f, "{}", termion::style::Bold)?;
        }
        if add.contains(Modifier::ITALIC) {
            write!(f, "{}", termion::style::Italic)?;
        }
        if add.contains(Modifier::UNDERLINED) {
            write!(f, "{}", termion::style::Underline)?;
        }
        if add.contains(Modifier::DIM) {
            write!(f, "{}", termion::style::Faint)?;
        }
        if add.contains(Modifier::CROSSED_OUT) {
            write!(f, "{}", termion::style::CrossedOut)?;
        }
        if add.contains(Modifier::SLOW_BLINK) || add.contains(Modifier::RAPID_BLINK) {
            write!(f, "{}", termion::style::Blink)?;
        }
        Ok(())
    }
}
