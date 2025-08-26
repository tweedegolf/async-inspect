use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Stylize,
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph},
};

use crate::{
    embassy_inspector::dwarf_parser::{async_fn::Member, future::FutureValue},
    scroll_view::ScrollView,
};

use super::{
    Click, ClickButton, Type,
    dwarf_parser::{
        async_fn::{AsyncFnType, AsyncFnValue},
        future::FutureValueKind,
        task_pool::{TaskPoolValue, TaskValue},
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
    pub(crate) try_format_value: &'a mut dyn FnMut(&[u8], &Type) -> Line<'static>,
}

#[derive(Debug)]
pub enum UiEvent {
    Back,
    AddPage(Box<dyn UiPage + Sync + Send>),
    SetScroll(i32),
    ToggleClosed(Vec<u64>),
    ToggleDetails(Vec<u64>),
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

impl MainMenu {
    fn new() -> Self {
        Self { scroll: 0 }
    }
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

        let mut scroll_view = ScrollView::new(rest, self.scroll);

        for (pool_idx, pool) in ctx.values.iter().enumerate() {
            let area = scroll_view.next_area(3 + pool.task_pool.number_of_tasks as u16);

            let block = Block::bordered().title(pool.task_pool.path.clone().blue());
            scroll_view.render_widget(&block, area);

            let mut area = block.inner(area);
            area.height = 1;

            scroll_view.render_widget(Line::raw("Tasks in pool:"), area);
            area.y += 1;
            for (task_idx, task) in pool.task_values.iter().enumerate() {
                let init = match task {
                    TaskValue::Uninit => Span::raw("uninitialized").gray(),
                    TaskValue::Init(_) => Span::raw("spawned").blue(),
                };
                let vis_area = scroll_view.render_widget(
                    Line::from_iter([Span::raw(format!("- {task_idx}: ")), init]),
                    area,
                );
                if is_clicked_left(&vis_area, ctx.click) {
                    return Err(UiEvent::AddPage(Box::new(Task::new(pool_idx, task_idx))));
                }
                area.y += 1;
            }
        }

        scroll_view.render_next_widget(Line::raw("Click on a task for details"), 1);

        if scroll_view.max_scroll() < self.scroll {
            return Err(UiEvent::SetScroll(scroll_view.max_scroll()));
        }

        ctx.frame.render_widget(scroll_view, rest);

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct ItemState {
    closed: bool,
    details_open: bool,
    children: HashMap<u64, ItemState>,
}

impl Default for ItemState {
    fn default() -> Self {
        Self {
            closed: false,
            details_open: false,
            children: HashMap::new(),
        }
    }
}

impl ItemState {
    fn toggle_closed(&mut self, path: &[u64]) {
        match path {
            [head, rest @ ..] => {
                self.children.entry(*head).or_default().toggle_closed(rest);
            }
            [] => {
                self.closed = !self.closed;
            }
        }
    }
    fn toggle_details(&mut self, path: &[u64]) {
        match path {
            [head, rest @ ..] => {
                self.children.entry(*head).or_default().toggle_details(rest);
            }
            [] => {
                self.details_open = !self.details_open;
            }
        }
    }
}

struct TreeData<'a> {
    value: &'a FutureValue,
    path: Vec<u64>,
    item_state: &'a ItemState,
}

#[derive(Debug, Clone)]
struct Task {
    pool_idx: usize,
    task_idx: usize,

    item_state: ItemState,
    scroll: i32,
}

impl Task {
    fn new(pool_idx: usize, task_idx: usize) -> Self {
        Self {
            pool_idx,
            task_idx,
            item_state: ItemState::default(),
            scroll: 0,
        }
    }

    fn add_future(
        tree_data: &TreeData,
        scroll_view: &mut ScrollView,
        ctx: &mut UiDrawCtx,
    ) -> Result<(), UiEvent> {
        let mut children = Vec::<(&FutureValue, u64)>::new();

        let line = match &tree_data.value.kind {
            FutureValueKind::AsyncFn(value) => {
                let mut line = Line::from_iter([
                    Span::raw("Function "),
                    Span::raw(tree_data.value.ty.to_string()).blue(),
                    Span::raw(" is waiting at "),
                ]);
                match &value.state_value {
                    Ok(state) => {
                        line.push_span(Span::raw(&state.state.name).blue());
                        if let Some(source) = &state.state.source {
                            line.push_span(Span::raw(" ("));
                            line.push_span(Span::raw(source.to_string()).blue());
                            line.push_span(Span::raw(")"));
                        }
                        if let Some(awaitee) = &state.awaitee {
                            line.push_span(Span::raw(" on:"));

                            children.push((awaitee, state.state.discriminant_value));
                        }
                    }
                    Err(err_discr) => {
                        line.push_span(format!("<invalid discriminant {err_discr}>").blue());
                    }
                }
                line
            }
            FutureValueKind::SelectValue(value) => {
                let line = Line::from_iter([
                    Span::raw("Select waiting on one off "),
                    Span::raw(value.awaitees.len().to_string()).blue(),
                    Span::raw(" futures:"),
                ]);
                for (i, awaitee) in value.awaitees.iter().enumerate() {
                    children.push((awaitee, i as u64));
                }
                line
            }
            FutureValueKind::JoinValue(value) => {
                let line = Line::from_iter([
                    Span::raw("Join waiting on "),
                    Span::raw(value.awaitees.len().to_string()).blue(),
                    Span::raw(" futures:"),
                ]);
                for (i, awaitee) in value.awaitees.iter().enumerate() {
                    children.push((awaitee, i as u64));
                }
                line
            }
            FutureValueKind::Unknown { .. } => Line::raw(tree_data.value.ty.to_string()),
        };
        let details = if tree_data.item_state.details_open {
            let text = match &tree_data.value.kind {
                FutureValueKind::AsyncFn(value) => {
                    let mut text = Text::raw("");
                    text.extend(async_fn_to_text(
                        &value.ty,
                        Some(value),
                        &mut ctx.try_format_value,
                    ));
                    text
                }
                FutureValueKind::SelectValue(_) => {
                    Text::from("Select polls ready the moment one of its childs is ready")
                }
                FutureValueKind::JoinValue(_) => {
                    Text::from("Select polls ready once all of its children have polled ready once")
                }
                FutureValueKind::Unknown(bytes) => {
                    Text::from((ctx.try_format_value)(bytes, &tree_data.value.ty))
                }
            };

            Some(Paragraph::new(text).wrap(Default::default()))
        } else {
            None
        };

        let indent = tree_data.path.len() as u16 * 2;
        let text_width = scroll_view
            .frame_size()
            .width
            .saturating_sub(indent)
            .saturating_sub(3);
        if text_width == 0 {
            return Ok(());
        }

        let line = Paragraph::new(line).wrap(Default::default());

        let line_height = line.line_count(text_width);
        let detail_height = if let Some(details) = &details {
            // Adding one for the border
            details.line_count(text_width) + 1
        } else {
            0
        };
        let total_height = line_height + detail_height;

        let mut area = scroll_view.next_area(total_height as u16);
        area.x += indent;
        area.width -= indent;

        let mut button_area = area;
        button_area.width = 2;
        button_area.height = 1;
        let button_area = scroll_view.render_widget(
            Span::raw(match tree_data.item_state.closed {
                true => "-",
                false => "+",
            }),
            button_area,
        );
        if is_clicked_left(&button_area, ctx.click) {
            return Err(UiEvent::ToggleClosed(tree_data.path.clone()));
        }

        area.x += 1;
        area.width = area.width.saturating_sub(1);
        if let Some(detail) = details {
            let block = Block::bordered().padding(Padding::top(line_height as u16 - 1));
            let detail_area = block.inner(area);
            scroll_view.render_widget(block, area);
            let area = scroll_view.render_widget(detail, detail_area);
            if is_clicked_left(&area, ctx.click) {
                return Err(UiEvent::ToggleDetails(tree_data.path.clone()));
            }
        }

        area.x += 1;
        area.width = area.width.saturating_sub(2); // Minus 2 to leave space for border if details are open
        area.height = line_height as u16;
        let area = scroll_view.render_widget(line, area);
        if is_clicked_left(&area, ctx.click) {
            return Err(UiEvent::ToggleDetails(tree_data.path.clone()));
        }

        if tree_data.item_state.closed {
            return Ok(());
        }

        for (child_value, path_id) in children {
            let mut child_path = tree_data.path.clone();
            child_path.push(path_id);

            let item_state = match tree_data.item_state.children.get(&path_id) {
                Some(item_state) => item_state,
                None => &ItemState::default(),
            };

            let child_tree_data = TreeData {
                value: child_value,
                path: child_path,
                item_state,
            };

            Self::add_future(&child_tree_data, scroll_view, ctx)?;
        }

        Ok(())
    }
}

impl UiPage for Task {
    fn apply_scroll(&mut self, scroll: i32) {
        self.scroll += scroll;
        self.scroll = self.scroll.max(0);
    }

    fn apply_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::SetScroll(scroll) => self.scroll = scroll,
            UiEvent::ToggleClosed(path) => {
                self.item_state.toggle_closed(&path);
            }
            UiEvent::ToggleDetails(path) => {
                self.item_state.toggle_details(&path);
            }
            _ => {}
        }
    }

    fn title(&self, values: &[TaskPoolValue]) -> String {
        format!(
            "Task: {}[{}]",
            values[self.pool_idx].task_pool.path, self.task_idx
        )
    }

    fn draw(&self, ctx: &mut UiDrawCtx, area: Rect) -> Result<(), UiEvent> {
        let Some(pool) = ctx.values.get(self.pool_idx) else {
            return Err(UiEvent::Back);
        };
        let Some(task) = pool.task_values.get(self.task_idx) else {
            return Err(UiEvent::Back);
        };

        let mut scroll_view = ScrollView::new(area, self.scroll);

        match task {
            TaskValue::Uninit => {
                scroll_view.render_next_widget(Line::raw("Task is uninitialized"), 1);
            }
            TaskValue::Init(value) => {
                scroll_view.render_next_widget(Line::raw("Await point backtrace:"), 1);

                let tree_data = TreeData {
                    value,
                    path: Vec::new(),
                    item_state: &self.item_state,
                };

                Self::add_future(&tree_data, &mut scroll_view, ctx)?;

                scroll_view.render_next_widget(Line::default(), 1);
                scroll_view.render_next_widget(
                    Line::raw(
                        "Click on a future to see details. Use the +/- to collapse/open awaitee's",
                    ),
                    1,
                );
            }
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
            page_stack: vec![Box::new(MainMenu::new())],
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
    F: FnMut(&[u8], &Type) -> Line<'static>,
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

    for member in &ty.members {
        let pos = add_member(member);
        member_positions.push(pos);
    }
    let state_pos = add_member(&ty.state_member);

    let awaitee_pos = add_col(Span::raw("awaitee"));

    let mut text = Text::from_iter([members_line, Line::default()]);

    for state in &ty.states {
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
                awaitee.offset, awaitee.size, awaitee.ty
            )));
        }

        if highlight {
            text.push_line(line.on_blue());
        } else {
            text.push_line(line);
        }
    }
    text.push_line(Line::default());

    for member in &ty.members {
        let mut line = Line::raw(format!(
            "{:>2}[{}] {:<15}: {}",
            member.offset, member.size, member.name, member.ty
        ));

        if let Some(value) = value
            && let Ok(state) = &value.state_value
            && let Some(member_value) = state.members.iter().find(|m| &m.member == member)
        {
            line.push_span(" = ");
            line.extend(try_format_value(&member_value.bytes, &member.ty));
        } else {
            line = line.gray();
        }

        text.push_line(line);
    }

    text
}
