mod dwarf_parser;

use anyhow::{Result, anyhow};
use ratatui::{Terminal, text::Text};

use crate::{
    backend::Backend,
    embassy_inspector::dwarf_parser::{DebugData, async_fn::describe_async_fn},
};

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
    debug_data: DebugData,
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

        let poll_break_point = backend.set_breakpoint(debug_data.poll_done_addres)?;

        Ok(Self {
            terminal: Terminal::new(ratatui_backend)?,
            poll_break_point,
            debug_data,
        })
    }

    pub fn handle_event<B: Backend>(&mut self, event: Event, backend: &mut B) -> Result<()> {
        match event {
            Event::Redraw => {
                // we redraw afther every event.
            }
            Event::Breakpoint(i) => {
                if i == self.poll_break_point {
                    log::error!("Poll hit, coninuing");
                    backend.resume()?;
                }
            }
        }

        self.terminal.draw(|frame| {
            let mut area = frame.area();

            for task_pool in &self.debug_data.task_pools {
                let bytes = backend
                    .read_memory(task_pool.address, task_pool.size)
                    .unwrap_or_default();
                let len_single_task = task_pool.size / task_pool.number_of_tasks as u64;

                let mut text = Text::from(format!("{}", task_pool.path));

                for task in 0..task_pool.number_of_tasks {
                    let mut s = String::new();

                    let bytes_offset = len_single_task + task as u64 * len_single_task
                        - task_pool.async_fn_type.layout.total_size;
                    let bytes = bytes[bytes_offset as usize..].as_ptr();

                    unsafe {
                        describe_async_fn(
                            &task_pool.async_fn_type.layout,
                            bytes,
                            "  ",
                            &self.debug_data.async_fn_types,
                            &mut s,
                        )
                    }

                    let task_text = Text::from(s);
                    for line in task_text.lines {
                        text.push_line(line);
                    }
                    text.push_line("");
                }

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
            if area.height == 0 {
                return;
            }

            for async_fn_type in &self.debug_data.async_fn_types {
                let text = Text::from(async_fn_type.to_string());

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
