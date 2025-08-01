use anyhow::{anyhow, Result};
use dwarf_reader::FutureType;
use ratatui::{text::Text, widgets::Paragraph, Terminal};

use crate::backend::Backend;

pub enum Event {
    /// Window was resized of made invalid for a diffrent reason and needs te be redrawn.
    Redraw,

    /// The breakpoint with the given id was hit.
    Breakpoint(u64),
}

#[derive(Debug)]
pub struct EmbassyInspector<RB: ratatui::backend::Backend> {
    terminal: Terminal<RB>,

    poll_break_point: u64,

    future_types: Vec<FutureType>,
}

impl<RB: ratatui::backend::Backend> EmbassyInspector<RB> {
    pub fn new<B: Backend>(ratatui_backend: RB, backend: &mut B) -> Result<Self> {
        let object_file = {
            let mut object_files = backend.get_objectfiles()?;
            object_files
                .next()
                .ok_or(anyhow!("Need atleast one objectfile"))?
        };

        let future_types = dwarf_reader::from_file(object_file)?;

        let poll_break_point =
            backend.set_breakpoint("embassy_executor::raw::SyncExecutor::poll::{{closure}}")?;

        Ok(Self {
            terminal: Terminal::new(ratatui_backend)?,

            poll_break_point,

            future_types,
        })
    }

    pub fn handle_event<B: Backend>(&mut self, event: Event, backend: &mut B) -> Result<()> {
        match event {
            Event::Redraw => {
                // we redraw afther every event.
            }
            Event::Breakpoint(i) => {
                log::error!("Poll hit, coninuing");
                backend.resume()?;
            }
        }

        self.terminal.draw(|frame| {
            let mut area = frame.area();

            for future_type in &self.future_types {
                let text = Text::from(future_type.to_string());

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
        })?;

        Ok(())
    }
}
