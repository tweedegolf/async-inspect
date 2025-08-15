mod dwarf_parser;
mod ui;

use anyhow::{Result, anyhow};
use ratatui::{Terminal, layout::Position};

use crate::backend::Backend;
use dwarf_parser::{DebugData, task_pool::TaskPoolValue};

use ui::UiState;

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ClickButton {
    Left,
    Middle,
    Right,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Click {
    pub pos: Position,
    pub button: ClickButton,
}

#[derive(Debug)]
pub enum Event {
    /// Window was resized of made invalid for a diffrent reason and needs te be redrawn.
    Redraw,

    /// The breakpoint with the given id was hit.
    Breakpoint(u64),
    Click(Click),
    Scroll(i32),
}

#[derive(Debug)]
pub struct EmbassyInspector<RB: ratatui::backend::Backend> {
    terminal: Terminal<RB>,
    poll_break_point_ids: Vec<u64>,

    ui_state: UiState,

    debug_data: DebugData,
    last_values: Vec<TaskPoolValue>,
}

impl<RB: ratatui::backend::Backend> EmbassyInspector<RB> {
    pub fn new<B: Backend>(ratatui_backend: RB, backend: &mut B) -> Result<Self> {
        let object_file = {
            let mut object_files = backend.get_objectfiles()?;
            object_files
                .next()
                .ok_or(anyhow!("Need atleast one objectfile"))?
        };

        let debug_data = DebugData::from_object_file(object_file)?;

        let mut poll_break_point_ids = Vec::new();
        for addr in &debug_data.poll_done_addresses {
            let id = backend.set_breakpoint(*addr)?;
            poll_break_point_ids.push(id);
        }

        let mut s = Self {
            terminal: Terminal::new(ratatui_backend)?,
            poll_break_point_ids,

            ui_state: UiState::new(),

            debug_data,
            last_values: Vec::new(),
        };
        s.update_values(backend);
        Ok(s)
    }

    fn update_values<B: Backend>(&mut self, backend: &mut B) {
        self.last_values.clear();

        for task_pool in &self.debug_data.task_pools {
            let bytes = match backend.read_memory(task_pool.address, task_pool.size) {
                Ok(bytes) => bytes,
                Err(e) => {
                    log::error!("{}", e);
                    continue;
                }
            };

            let task_pool_value = self.debug_data.get_taskpool_value(task_pool, &bytes);

            self.last_values.push(task_pool_value);
        }
    }

    pub fn handle_event<B: Backend>(&mut self, event: Event, backend: &mut B) -> Result<()> {
        let click = match event {
            Event::Redraw => {
                // we redraw afther every event.
                None
            }
            Event::Breakpoint(i) => {
                self.update_values(backend);

                if self.poll_break_point_ids.contains(&i) {
                    log::error!("Poll hit, coninuing");
                    backend.resume()?;
                }
                None
            }
            Event::Click(click) => Some(click),
            Event::Scroll(s) => {
                self.ui_state.apply_scroll(s);
                None
            }
        };

        self.terminal.draw(|frame| {
            let mut click = click;

            while let Err(event) = self.ui_state.draw(frame, click, &self.last_values) {
                self.ui_state.apply_event(event);
                click = None;

                frame.render_widget(ratatui::widgets::Clear, frame.area());
            }
        })?;

        Ok(())
    }
}
