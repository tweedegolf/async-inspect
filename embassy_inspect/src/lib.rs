mod callback;
mod dwarf_parser;
mod scroll_view;
mod ui;

use std::collections::HashMap;

use anyhow::{Result, anyhow};
use ratatui::{
    Terminal,
    layout::Position,
    style::Stylize,
    text::{Line, Span},
};

use dwarf_parser::{DebugData, task_pool::TaskPoolValue};
use ui::{UiDrawCtx, UiState};

pub use crate::callback::Callback;
pub use dwarf_parser::ty;

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
    Click(Click),
    Scroll(i32),

    /// The breakpoint with the given id was hit.
    Breakpoint(u64),
    /// The target was stopped for any reason except a breakpoint.
    Stoped,
}

#[derive(Debug)]
pub struct EmbassyInspector<RB: ratatui::backend::Backend> {
    terminal: Terminal<RB>,
    poll_break_point_ids: Vec<u64>,

    ui_state: UiState,

    debug_data: DebugData,
    last_values: Vec<TaskPoolValue>,
    // Gdb can only format values containing pointers when the target is stopt, so we cache formated
    // values here to use if the screen needs to be refreshed for for example scrolling while the
    // target is running.
    formating_cache: HashMap<(Vec<u8>, ty::Type), Line<'static>>,
}

impl<RB: ratatui::backend::Backend> EmbassyInspector<RB> {
    pub fn new<C: Callback>(ratatui_backend: RB, callback: &mut C) -> Result<Self> {
        let object_file = {
            let mut object_files = callback.get_objectfiles()?;
            object_files
                .next()
                .ok_or(anyhow!("Need at least one objectfile"))?
        };

        let debug_data = DebugData::from_object_file(object_file)?;

        let mut poll_break_point_ids = Vec::new();
        for addr in &debug_data.poll_done_addresses {
            let id = callback.set_breakpoint(*addr)?;
            poll_break_point_ids.push(id);
        }

        let mut s = Self {
            terminal: Terminal::new(ratatui_backend)?,
            poll_break_point_ids,

            ui_state: UiState::new(),

            debug_data,
            last_values: Vec::new(),
            formating_cache: HashMap::new(),
        };
        s.update_values(callback);
        s.handle_event(Event::Redraw, callback)?;
        Ok(s)
    }

    fn update_values<C: Callback>(&mut self, callback: &mut C) {
        self.last_values.clear();
        self.formating_cache.clear();

        for task_pool in &self.debug_data.task_pools {
            let bytes = match callback.read_memory(task_pool.address, task_pool.size) {
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

    pub fn handle_event<C: Callback>(&mut self, event: Event, callback: &mut C) -> Result<()> {
        let click = match event {
            Event::Redraw => {
                // we redraw afther every event.
                None
            }
            Event::Click(click) => Some(click),
            Event::Scroll(s) => {
                self.ui_state.apply_scroll(s);
                None
            }
            Event::Breakpoint(i) => {
                self.update_values(callback);

                if self.poll_break_point_ids.contains(&i) {
                    log::error!("Poll hit, coninuing");
                    callback.resume()?;
                }
                None
            }
            Event::Stoped => {
                self.update_values(callback);
                None
            }
        };

        self.terminal.draw(|frame| {
            let mut ctx = UiDrawCtx {
                frame,
                click,
                values: &self.last_values,
                try_format_value: &mut |b, ty| {
                    self.formating_cache
                        .entry((b.to_vec(), ty.clone()))
                        .or_insert_with_key(|(b, t)| format_value(b, t, callback))
                        .clone()
                },
            };

            while let Err(event) = self.ui_state.draw(&mut ctx) {
                self.ui_state.apply_event(event);
                ctx.click = None;

                ctx.frame
                    .render_widget(ratatui::widgets::Clear, ctx.frame.area());
            }
        })?;

        Ok(())
    }
}

fn format_value<C: Callback>(bytes: &[u8], ty: &ty::Type, callback: &mut C) -> Line<'static> {
    match callback
        .try_format_value(&bytes, &ty)
        .and_then(|formatted| ansi_to_tui::IntoText::into_text(&formatted).ok())
        .map(|text| text.into_iter().flatten())
    {
        Some(spans) => Line::from_iter(spans),
        None => Line::from_iter([
            Span::raw("bytes ["),
            Span::raw(
                bytes
                    .iter()
                    .map(|b| format!(" {b:0>2x}"))
                    .collect::<String>(),
            )
            .blue(),
            Span::raw(" ]"),
        ]),
    }
}
