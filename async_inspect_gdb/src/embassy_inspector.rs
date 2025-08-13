mod dwarf_parser;

use anyhow::{Result, anyhow};
use ratatui::{
    Terminal,
    layout::{Position, Rect},
    text::Text,
};

use crate::backend::Backend;
use dwarf_parser::{DebugData, task_pool::TaskPoolValue};

pub enum ClickButton {
    Left,
    Middle,
    Right,
}

pub enum Event {
    /// Window was resized of made invalid for a diffrent reason and needs te be redrawn.
    Redraw,

    /// The breakpoint with the given id was hit.
    Breakpoint(u64),
    Click(Position, ClickButton),
    Scroll(i32),
}

#[derive(Debug, Clone, PartialEq)]
enum UiState {
    MainMenu,
    TaskPool(usize),
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

        Ok(Self {
            terminal: Terminal::new(ratatui_backend)?,
            poll_break_point_ids,

            ui_state: UiState::MainMenu,

            debug_data,
            last_values: Vec::new(),
        })
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
                if self.poll_break_point_ids.contains(&i) {
                    log::error!("Poll hit, coninuing");
                    backend.resume()?;
                }
                None
            }
            Event::Click(pos, _) => Some(pos),
            Event::Scroll(_) => None,
        };

        self.update_values(backend);

        let old_ui_state = self.ui_state.clone();

        self.terminal.draw(|frame| {
            let mut area = frame.area();

            match &self.ui_state {
                UiState::MainMenu => {
                    for (i, value) in self.last_values.iter().enumerate() {
                        let text = Text::from(format!(
                            "{}\n{}",
                            value.task_pool.path, value.task_pool.async_fn_type
                        ));

                        let height = text.height();
                        let mut text_area = area;
                        text_area.height = height as u16;

                        if is_clicked(&text_area, click) {
                            self.ui_state = UiState::TaskPool(i)
                        }

                        frame.render_widget(text, text_area);

                        area.height = area.height.saturating_sub(text_area.height + 1);
                        area.y += text_area.height + 1;
                        if area.height == 0 {
                            break;
                        }
                    }
                }
                UiState::TaskPool(i) => {
                    let task_pool = &self.last_values[*i];

                    for (i, value) in task_pool.async_fn_values.iter().enumerate() {
                        let text = match &value.state_value {
                            Ok(state) => Text::from(format!("{i}: {}", state.state.name)),
                            Err(discriminiant) => {
                                Text::from(format!("{i}: unknown discriminiant = {discriminiant}"))
                            }
                        };

                        let height = text.height();
                        let mut text_area = area;
                        text_area.height = height as u16;
                        frame.render_widget(text, text_area);

                        area.height = area.height.saturating_sub(text_area.height + 1);
                        area.y += text_area.height + 1;
                        if area.height == 0 {
                            break;
                        }
                    }
                }
            }
        })?;

        if self.ui_state != old_ui_state {
            return self.handle_event(Event::Redraw, backend);
        }

        Ok(())
    }
}

fn is_clicked(area: &Rect, click: Option<Position>) -> bool {
    match click {
        Some(pos) => area.contains(pos),
        None => false,
    }
}

// struct Ui {

// }
