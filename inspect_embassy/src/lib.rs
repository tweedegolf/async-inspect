//! Frontend library implementing the memory model and the TUI.
//!
//! # As debugger
//! This library can't be used on it's own to debug a target and should be used via a backend. See
//! their documentation on how to use it.
//!
//! # Creating a backend
//! (Also see the `Architecture.md` file in this project repository)
//!
//! Backend should create a [`EmbassyInspector`] before starting its own an event loop.
//! Events should then be sent to via [`EmbassyInspector::handle_event`]. See the [`Callback`] trait
//! for what operations you will have to be able to implement.

mod callback;
mod model;
mod ui;

use std::collections::HashMap;

use anyhow::{Result, anyhow};
use ratatui::{
    Terminal,
    layout::Position,
    style::Stylize,
    text::{Line, Span},
};

use model::{DebugData, task_pool::TaskPoolValue};
use ui::{UiDrawCtx, UiState};

pub use crate::callback::Callback;
pub use model::ty::Type;

/// The mouse button that was used for a click.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ClickButton {
    Left,
    Middle,
    Right,
}

/// A single mouse click.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Click {
    pub pos: Position,
    pub button: ClickButton,
}

/// External event to be send to an [`EmbassyInspector`].
#[derive(Debug)]
pub enum Event {
    /// Window was resized or made invalid for a different reason.
    ///
    /// This event will fore the TUI the be redrawn.
    Redraw,
    /// The user clicked on the TUI.
    Click(Click),
    /// The user scrolled in the TUI.
    ///
    /// A negative number indicates scrolling down, the magnitude is the amount of lines to scroll.
    Scroll(i32),

    /// The breakpoint with the given id was hit. See [`Callback`] for how the id's work.
    ///
    /// **The target should be readable when this event is triggered.**
    Breakpoint(u64),
    /// The target was stopped for any other reason.
    ///
    /// **The target should be readable when this event is triggered.**
    Stoped,
}

/// Contains the full state of the debugger
///
/// Create an instance of this struct on startup before stating the event loop. Relevant events
/// should be sent to [`handle_event`](Self::handle_event), the TUI will then automatically be
/// redrawn when needed. See [`Event`] for what events to handle.
#[derive(Debug)]
pub struct EmbassyInspector<RB: ratatui::backend::Backend> {
    terminal: Terminal<RB>,
    ui_state: UiState,

    poll_break_point_ids: Vec<u64>,

    debug_data: DebugData,
    last_values: Vec<TaskPoolValue>,
    // GDB can only format values containing pointers when the target has been stopped, so we cache
    // formatted values here to use if the screen needs to be refreshed for for example scrolling
    // while the target is still running.
    //
    // This does not work in all cases, but it does help in a lot of them.
    formating_cache: HashMap<(Vec<u8>, Type), Line<'static>>,
}

impl<RB: ratatui::backend::Backend> EmbassyInspector<RB> {
    /// Create a new [`EmbassyInspector`].
    ///
    /// The `ratatui_backend` will be drawn to automatically when needed.
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

    /// Process a new external [`Event`]
    ///
    /// See [`Event`] for all possible event and whether or not the target needs to be readable when
    /// the event is dispatched.
    pub fn handle_event<C: Callback>(&mut self, event: Event, callback: &mut C) -> Result<()> {
        let click = match event {
            Event::Redraw => {
                // We redraw after every event anyway so nothing to do here.
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

/// Format a value using the callback.
///
/// Falls back to just printing a list of bytes if the formatter in the backend fails.
fn format_value<C: Callback>(bytes: &[u8], ty: &Type, callback: &mut C) -> Line<'static> {
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
