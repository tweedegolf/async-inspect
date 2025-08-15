use std::ops::{Deref, DerefMut};

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Paragraph},
};

use crate::scroll_view::ScrollView;

use super::{Click, ClickButton, dwarf_parser::task_pool::TaskPoolValue};

fn is_clicked_left(area: &Rect, click: Option<Click>) -> bool {
    match click {
        Some(click) => click.button == ClickButton::Left && area.contains(click.pos),
        None => false,
    }
}

#[derive(Debug)]
pub enum UiEvent {
    Back,
    AddPage(Box<dyn UiPage + Sync + Send>),
    SetScroll(i32),
}

pub trait UiPage: std::fmt::Debug {
    fn apply_scroll(&mut self, _scroll: i32);

    fn apply_event(&mut self, event: UiEvent);

    fn title(&self, values: &[TaskPoolValue]) -> String;

    fn draw(
        &self,
        frame: &mut Frame,
        area: Rect,
        click: Option<Click>,
        values: &[TaskPoolValue],
    ) -> Result<(), UiEvent>;
}

#[derive(Debug, Clone)]
struct MainMenu {
    scroll: i32,
}

impl UiPage for MainMenu {
    fn apply_scroll(&mut self, scroll: i32) {
        self.scroll += scroll;
        self.scroll = self.scroll.max(0);
    }

    fn apply_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::SetScroll(scroll) => self.scroll = scroll,
            _ => {}
        }
    }

    fn title(&self, _values: &[TaskPoolValue]) -> String {
        String::from("Main menu")
    }

    fn draw(
        &self,
        frame: &mut Frame,
        area: Rect,
        click: Option<Click>,
        values: &[TaskPoolValue],
    ) -> Result<(), UiEvent> {
        let [header, rest] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);

        frame.render_widget(Text::from("Found task pools:"), header);

        let mut scroll_view = ScrollView::new(rest.as_size(), self.scroll);
        for (i, pool) in values.iter().enumerate() {
            let mut text = Text::from(vec![
                Line::from(vec![
                    Span::from("Pool size: "),
                    pool.task_pool.number_of_tasks.to_string().light_blue(),
                ]),
                Line::from("Future state machine:"),
            ]);
            text.extend(Text::from(pool.task_pool.async_fn_type.layout.to_string()));
            let height = text.height() + 2;

            let text = Paragraph::new(text)
                .block(Block::bordered().title(pool.task_pool.path.clone().light_blue()));

            let area = scroll_view.render_next_widget(&text, height as u16);
            if is_clicked_left(&area, click) {
                return Err(UiEvent::AddPage(Box::new(TaskPool {
                    pool_idx: i,
                    scroll: 0,
                })));
            }
        }

        if scroll_view.max_scroll() < self.scroll {
            return Err(UiEvent::SetScroll(scroll_view.max_scroll()));
        }

        frame.render_widget(scroll_view, rest);

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct TaskPool {
    pool_idx: usize,
    scroll: i32,
}

impl UiPage for TaskPool {
    fn apply_scroll(&mut self, scroll: i32) {
        self.scroll += scroll;
        self.scroll = self.scroll.max(0);
    }

    fn apply_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::SetScroll(scroll) => self.scroll = scroll,
            _ => {}
        }
    }

    fn title(&self, values: &[TaskPoolValue]) -> String {
        format!("Task pool: {}", values[self.pool_idx].task_pool.path)
    }

    fn draw(
        &self,
        frame: &mut Frame,
        area: Rect,
        click: Option<Click>,
        values: &[TaskPoolValue],
    ) -> Result<(), UiEvent> {
        let pool = &values[self.pool_idx];

        let mut scroll_view = ScrollView::new(area.as_size(), self.scroll);
        for (_i, value) in pool.async_fn_values.iter().enumerate() {
            let text = Text::from(format!("{:?}\n", value.state_value));

            scroll_view.render_next_widget(&text, text.height() as u16);
        }

        if scroll_view.max_scroll() < self.scroll {
            return Err(UiEvent::SetScroll(scroll_view.max_scroll()));
        }

        frame.render_widget(scroll_view, area);

        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct UiState {
    page_stack: Vec<Box<dyn UiPage + Sync + Send>>,
}

impl UiState {
    pub(crate) fn new() -> Self {
        Self {
            page_stack: vec![Box::new(MainMenu { scroll: 0 })],
        }
    }

    fn top(&self) -> &dyn UiPage {
        self.page_stack.last().map(Deref::deref).unwrap()
    }

    fn top_mut(&mut self) -> &mut dyn UiPage {
        self.page_stack.last_mut().map(DerefMut::deref_mut).unwrap()
    }

    pub(crate) fn apply_scroll(&mut self, scroll: i32) {
        self.top_mut().apply_scroll(scroll);
    }

    pub(crate) fn apply_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::Back => {
                if self.page_stack.len() != 1 {
                    self.page_stack.pop();
                }
            }
            UiEvent::AddPage(page) => {
                self.page_stack.push(page);
            }
            other => self.top_mut().apply_event(other),
        }
    }

    fn draw_title_bar(
        &self,
        frame: &mut Frame,
        mut area: Rect,
        click: Option<Click>,
        values: &[TaskPoolValue],
    ) -> Result<(), UiEvent> {
        if self.page_stack.len() > 1 {
            let [back_area, rest_area] =
                Layout::horizontal([Constraint::Length(6), Constraint::Fill(1)]).areas(area);
            area = rest_area;

            if is_clicked_left(&back_area, click) {
                return Err(UiEvent::Back);
            }

            let back = Line::styled("Back", Modifier::UNDERLINED)
                .alignment(ratatui::layout::Alignment::Center)
                .black()
                .on_white();

            frame.render_widget(back, back_area);
        }

        let title = self.top().title(values);

        let title = Line::raw(title)
            .alignment(ratatui::layout::Alignment::Center)
            .black()
            .on_white();

        frame.render_widget(title, area);

        Ok(())
    }

    pub fn draw(
        &self,
        frame: &mut Frame,
        click: Option<Click>,
        values: &[TaskPoolValue],
    ) -> Result<(), UiEvent> {
        if let Some(click) = click
            && click.button == ClickButton::Right
        {
            return Err(UiEvent::Back);
        }

        let area = frame.area();
        let [title_area, rest_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
        self.draw_title_bar(frame, title_area, click, values)?;

        self.top().draw(frame, rest_area, click, values)
    }
}
