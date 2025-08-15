use std::ops::{Deref, DerefMut};

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Stylize,
    text::{Line, Span, Text},
    widgets::{Block, Paragraph},
};

use crate::{embassy_inspector::dwarf_parser::async_fn::Member, scroll_view::ScrollView};

use super::{
    Click, ClickButton,
    dwarf_parser::{
        async_fn::{AsyncFnType, AsyncFnValue},
        task_pool::TaskPoolValue,
    },
};

fn is_clicked_left(area: &Rect, click: Option<Click>) -> bool {
    match click {
        Some(click) => click.button == ClickButton::Left && area.contains(click.pos),
        None => false,
    }
}

pub struct UiDrawCtx<'a, 'b> {
    pub(crate) frame: &'a mut Frame<'b>,
    pub(crate) click: Option<Click>,
    pub(crate) values: &'a [TaskPoolValue],
    pub(crate) try_format_value: &'a mut dyn FnMut(&[u8], &str) -> Option<String>,
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

    fn draw(&self, ctx: &mut UiDrawCtx, area: Rect) -> Result<(), UiEvent>;
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

    fn draw(&self, ctx: &mut UiDrawCtx, area: Rect) -> Result<(), UiEvent> {
        let [header, rest] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);

        ctx.frame
            .render_widget(Text::from("Found task pools:"), header);

        let mut scroll_view = ScrollView::new(rest.as_size(), self.scroll);
        for (i, pool) in ctx.values.iter().enumerate() {
            let mut text = Text::from(Line::from_iter([
                Span::from("Pool size: "),
                pool.task_pool.number_of_tasks.to_string().blue(),
            ]));

            if let [value] = pool.async_fn_values.as_slice() {
                text.push_line(Line::from("Future value:"));
                text.extend(async_fn_to_text(
                    &pool.task_pool.async_fn_type,
                    Some(value),
                    &mut ctx.try_format_value,
                ));
            } else {
                text.push_line(Line::from("Future state machine alyout:"));
                text.extend(
                    async_fn_to_text(
                        &pool.task_pool.async_fn_type,
                        None,
                        &mut ctx.try_format_value,
                    )
                    .into_iter()
                    .map(|l| l.gray()),
                );
                text.push_line(Line::from("Click to see individual tasks"));
            }

            let height = text.height() + 2;

            let text = Paragraph::new(text)
                .block(Block::bordered().title(pool.task_pool.path.clone().blue()));

            let area = scroll_view.render_next_widget(&text, height as u16);
            if is_clicked_left(&area, ctx.click) {
                return Err(UiEvent::AddPage(Box::new(TaskPool {
                    pool_idx: i,
                    scroll: 0,
                })));
            }
        }

        if scroll_view.max_scroll() < self.scroll {
            return Err(UiEvent::SetScroll(scroll_view.max_scroll()));
        }

        ctx.frame.render_widget(scroll_view, rest);

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

    fn draw(&self, ctx: &mut UiDrawCtx, area: Rect) -> Result<(), UiEvent> {
        let pool = &ctx.values[self.pool_idx];

        let mut scroll_view = ScrollView::new(area.as_size(), self.scroll);
        for (i, value) in pool.async_fn_values.iter().enumerate() {
            let text = async_fn_to_text(&value.ty, Some(value), &mut ctx.try_format_value);

            let height = text.height() + 2;
            let text =
                Paragraph::new(text).block(Block::bordered().title(format!("Task {i}").blue()));

            let _area = scroll_view.render_next_widget(&text, height as u16);
        }

        if scroll_view.max_scroll() < self.scroll {
            return Err(UiEvent::SetScroll(scroll_view.max_scroll()));
        }

        ctx.frame.render_widget(scroll_view, area);

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

    fn draw_title_bar(&self, ctx: &mut UiDrawCtx, mut area: Rect) -> Result<(), UiEvent> {
        if self.page_stack.len() > 1 {
            let [back_area, rest_area] =
                Layout::horizontal([Constraint::Length(6), Constraint::Fill(1)]).areas(area);
            area = rest_area;

            if is_clicked_left(&back_area, ctx.click) {
                return Err(UiEvent::Back);
            }

            let back = Line::raw("Back")
                .alignment(ratatui::layout::Alignment::Center)
                .black()
                .on_white();

            ctx.frame.render_widget(back, back_area);
        }

        let title = self.top().title(ctx.values);

        let title = Line::raw(title)
            .alignment(ratatui::layout::Alignment::Center)
            .black()
            .on_white();

        ctx.frame.render_widget(title, area);

        Ok(())
    }

    pub(crate) fn draw(&self, ctx: &mut UiDrawCtx) -> Result<(), UiEvent> {
        if let Some(click) = ctx.click
            && click.button == ClickButton::Right
        {
            return Err(UiEvent::Back);
        }

        let area = ctx.frame.area();
        let [title_area, rest_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
        self.draw_title_bar(ctx, title_area)?;

        self.top().draw(ctx, rest_area)?;

        Ok(())
    }
}

fn async_fn_to_text<'a, F>(
    ty: &'a AsyncFnType,
    value: Option<&AsyncFnValue>,
    try_format_value: &mut F,
) -> Text<'a>
where
    F: FnMut(&[u8], &str) -> Option<String>,
{
    let seperator: Span<'static> = Span::raw(" | ");

    let mut member_positions = Vec::new();

    let mut members_line: Line<'a> = Line::default();
    let mut members_current_col = 0;
    let mut add_col = |span: Span<'static>| {
        let span_size = span.content.len();
        let col = members_current_col;

        members_line.push_span(span);
        members_line.push_span(seperator.clone());

        members_current_col += span_size + seperator.content.len();

        (col, span_size)
    };

    add_col(Span::raw("           "));

    let mut add_member = |member: &Member| {
        add_col(Span::raw(format!(
            "{}[{}] {}",
            member.offset, member.size, member.name
        )))
    };

    for member in &ty.layout.members {
        let pos = add_member(member);
        member_positions.push(pos);
    }
    let state_pos = add_member(&ty.layout.state_member);

    let awaitee_pos = add_col(Span::raw("awaitee"));

    let mut text = Text::from_iter([members_line, Line::default()]);

    for state in &ty.layout.states {
        let (name, highlight) = if let Some(value) = value
            && let Ok(state_value) = &value.state_value
            && state_value.state.discriminant_value == state.discriminant_value
        {
            (format!("> {}", state.name), true)
        } else {
            (format!("  {}", state.name), false)
        };

        let mut current_col = name.len();
        let mut line = Line::raw(name);

        for active_members in &state.active_members {
            let (col, len) = member_positions[*active_members];

            line.push_span(Span::from(" ".repeat(col - current_col)));
            current_col = col;
            line.push_span(Span::from("-".repeat(len)));
            current_col += len;
        }

        line.push_span(Span::from(" ".repeat(state_pos.0 - current_col)));
        let discriminant = state.discriminant_value.to_string();
        line.push_span(Span::from(discriminant.clone()));
        line.push_span(Span::from(" ".repeat(state_pos.1 - discriminant.len())));
        current_col = state_pos.0 + state_pos.1;

        if let Some(awaitee) = &state.awaitee {
            line.push_span(Span::from(" ".repeat(awaitee_pos.0 - current_col)));
            line.push_span(Span::from(format!(
                "{}[{}] {}",
                awaitee.offset, awaitee.size, awaitee.type_name
            )));
        }

        if highlight {
            text.push_line(line.on_blue());
        } else {
            text.push_line(line);
        }
    }
    text.push_line(Line::default());

    for member in &ty.layout.members {
        let mut line = Line::raw(format!(
            "{:>2}[{}] {:<15}: ",
            member.offset, member.size, member.name
        ));

        if let Some(value) = value
            && let Ok(state) = &value.state_value
            && let Some(member_value) = state.members.iter().find(|m| &m.member == member)
        {
            match try_format_value(&member_value.bytes, &member.type_name)
                .and_then(|formatted| ansi_to_tui::IntoText::into_text(&formatted).ok())
                .map(|text| text.into_iter().flatten())
            {
                Some(spans) => line.extend(spans),
                None => line.extend([
                    Span::raw(format!("{} = bytes [", member.type_name)),
                    Span::raw(
                        member_value
                            .bytes
                            .iter()
                            .map(|b| format!(" {b:0>2x}"))
                            .collect::<String>(),
                    )
                    .blue(),
                    Span::raw(" ]"),
                ]),
            }
        }

        text.push_line(line);
    }

    text
}
