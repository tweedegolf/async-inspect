use ratatui::{widgets::Paragraph, Terminal};

use crate::backend::Backend;

pub enum Event {
    /// Window was resized of made invalid for a diffrent reason and needs te be redrawn.
    Redraw,
}

#[derive(Debug)]
pub struct EmbassyInspector<RB: ratatui::backend::Backend> {
    terminal: Terminal<RB>,
}

impl<RB: ratatui::backend::Backend> EmbassyInspector<RB> {
    pub fn new(ratatui_backend: RB) -> std::io::Result<Self> {
        Ok(Self {
            terminal: Terminal::new(ratatui_backend)?,
        })
    }

    pub fn handle_event<B: Backend>(
        &mut self,
        event: Event,
        _backend: &mut B,
    ) -> std::io::Result<()> {
        match event {
            Event::Redraw => {
                // we redraw afther every event.
            }
        }

        self.terminal.draw(|frame| {
            let area = frame.area();
            frame.render_widget(Paragraph::new("Hello from ratatui!"), area);
        })?;

        Ok(())
    }
}
