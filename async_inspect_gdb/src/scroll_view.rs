//! Based on https://crates.io/crates/tui-scrollview. modified to allow tracking clicks, and
//! simplied for only my usecase.

use ratatui::{
    buffer::Buffer,
    layout::{Rect, Size},
    widgets::Widget,
};

pub struct ScrollView {
    buf: Buffer,
    scroll: i32,

    next_y: u16,
}

impl ScrollView {
    /// Create a new scroll view that is going to be rendered to a area with the given frame size.
    ///
    /// A positive scroll meas the content will be moved up, a negative scroll will move the content
    /// down.
    pub fn new(frame: Size, scroll: i32) -> Self {
        let area = Rect {
            x: 0,
            y: 0,
            width: frame.width,
            height: frame.height,
        };
        Self {
            buf: Buffer::empty(area),
            scroll,
            next_y: 0,
        }
    }

    /// Render a widget into the scroll buffer
    ///
    /// This is the equivalent of `Frame::render_widget`, but renders the widget into the scroll
    /// buffer rather than the main buffer. The widget will be rendered into the area of the buffer
    /// specified by the `area` parameter moved by the givven scroll.
    ///
    /// The returned rect will contain the area that will be visible on the main buffer. A rect of
    /// size zero will be returned if the widget is full off screen (the rect location is then
    /// unspecified).
    ///
    ///
    /// This should not be confused with the `render` method, which renders the visible area of the
    /// ScrollView into the main buffer.
    pub fn render_widget<W: Widget>(&mut self, widget: W, area: Rect) -> Rect {
        self.next_y = self.next_y.max(area.y + area.height);

        let buff_area_y = area.y as i32 - self.scroll;
        let buff_area_end = buff_area_y + area.height as i32;

        let full_of_screen = buff_area_end <= 0 || buff_area_y >= self.buf.area.height as i32;
        if full_of_screen {
            return Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            };
        }

        let virtual_buffer_needed = buff_area_y < 0
            || buff_area_y >= self.buf.area.height as i32
            || buff_area_end > self.buf.area.height as i32;

        if !virtual_buffer_needed {
            let area = Rect {
                x: area.x,
                y: buff_area_y as u16,
                width: area.width,
                height: area.height,
            };
            widget.render(area, &mut self.buf);
            return area;
        }

        let mut virtual_buff = Buffer::empty(Rect {
            x: 0,
            y: 0,
            width: self.buf.area.width.saturating_sub(area.x),
            height: area.height,
        });
        widget.render(virtual_buff.area, &mut virtual_buff);

        let overlap = self.buf.area.intersection(Rect {
            x: area.x,
            y: buff_area_y.max(0) as u16,
            width: area.width,
            // Shrinks the area if the y got cut off
            height: (area.height as i32 + buff_area_y.min(0)) as u16,
        });

        for buff_line in overlap.y..overlap.y + overlap.height {
            let virtual_line = buff_line as i32 - buff_area_y;
            debug_assert!(virtual_line >= 0);

            self.buf.content
                [self.buf.area.width as usize * buff_line as usize + overlap.x as usize..]
                [..overlap.width as usize]
                .clone_from_slice(
                    &virtual_buff.content
                        [virtual_buff.area.width as usize * virtual_line as usize..]
                        [..overlap.width as usize],
                );
        }

        return overlap;
    }

    /// Like [`Self::render_widget`] but automaticly places this widget directly underneath the
    /// lowest draw widget. Spanning the full width of the `ScrollView`.
    pub fn render_next_widget<W: Widget>(&mut self, widget: W, height: u16) -> Rect {
        self.render_widget(
            widget,
            Rect {
                x: 0,
                y: self.next_y,
                width: self.buf.area.width,
                height,
            },
        )
    }

    /// Returns the scroll value to use so that lowest rendered widget would just tuch the bottom of
    /// the view.
    ///
    /// If the widgets don't reach the bottom of the view, zero is returned.
    pub fn max_scroll(&self) -> i32 {
        (self.next_y as i32 - self.buf.area.height as i32).max(0)
    }
}

impl Widget for ScrollView {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let overlap = area.intersection(Rect {
            x: area.x,
            y: area.y,
            width: self.buf.area.width,
            height: self.buf.area.height,
        });

        // Can't directly copy lines as the buf could have a offset set.
        for (dst_row, src_row) in overlap.rows().zip(self.buf.area.rows()) {
            for (dst_col, src_col) in dst_row.columns().zip(src_row.columns()) {
                buf[dst_col] = self.buf[src_col].clone();
            }
        }
    }
}

#[cfg(test)]
mod test {
    use ratatui::text::Text;

    use super::*;

    fn draw_to_scroll_view(scroll_view: &mut ScrollView) {
        for y in 0..6 {
            let widget = Text::raw(format!("{}:\nABC\nDEF", y));

            let area = Rect::new(y, y * 3, 3, 3);
            scroll_view.render_widget(widget, area);
        }
    }

    #[test]
    fn only_full_widgets() {
        let mut main_buf = Buffer::empty(Rect::new(0, 0, 6, 6));

        let mut scroll_view = ScrollView::new(main_buf.area.as_size(), 0);
        draw_to_scroll_view(&mut scroll_view);

        scroll_view.render(main_buf.area, &mut main_buf);

        #[rustfmt::skip]
        assert_eq!(
            main_buf,
            Buffer::with_lines(vec![
                "0:    ",
                "ABC   ",
                "DEF   ",
                " 1:   ",
                " ABC  ",
                " DEF  ",
            ])
        );
    }

    #[test]
    fn widget_split() {
        let mut main_buf = Buffer::empty(Rect::new(0, 0, 6, 6));

        let mut scroll_view = ScrollView::new(main_buf.area.as_size(), 1);
        draw_to_scroll_view(&mut scroll_view);

        scroll_view.render(main_buf.area, &mut main_buf);

        #[rustfmt::skip]
        assert_eq!(
            main_buf,
            Buffer::with_lines(vec![
                "ABC   ",
                "DEF   ",
                " 1:   ",
                " ABC  ",
                " DEF  ",
                "  2:  ",
            ])
        );
    }

    #[test]
    fn negative_scroll() {
        let mut main_buf = Buffer::empty(Rect::new(0, 0, 6, 6));

        let mut scroll_view = ScrollView::new(main_buf.area.as_size(), -1);
        draw_to_scroll_view(&mut scroll_view);

        scroll_view.render(main_buf.area, &mut main_buf);

        #[rustfmt::skip]
        assert_eq!(
            main_buf,
            Buffer::with_lines(vec![
                "      ",
                "0:    ",
                "ABC   ",
                "DEF   ",
                " 1:   ",
                " ABC  ",
            ])
        );
    }

    #[test]
    fn x_cutoff() {
        let mut main_buf = Buffer::empty(Rect::new(0, 0, 2, 6));

        let mut scroll_view = ScrollView::new(main_buf.area.as_size(), 0);
        draw_to_scroll_view(&mut scroll_view);

        scroll_view.render(main_buf.area, &mut main_buf);

        #[rustfmt::skip]
        assert_eq!(
            main_buf,
            Buffer::with_lines(vec![
                "0:",
                "AB",
                "DE",
                " 1",
                " A",
                " D",
            ])
        );
    }

    #[test]
    fn offset_scroll_view_render() {
        let mut main_buf = Buffer::empty(Rect::new(0, 0, 6, 6));

        let mut scroll_view = ScrollView::new(main_buf.area.as_size(), 1);
        draw_to_scroll_view(&mut scroll_view);

        scroll_view.render(Rect::new(2, 2, 3, 3), &mut main_buf);

        #[rustfmt::skip]
        assert_eq!(
            main_buf,
            Buffer::with_lines(vec![
                "      ",
                "      ",
                "  ABC ",
                "  DEF ",
                "   1: ",
                "      ",
            ])
        );
    }

    #[test]
    fn main_buffer_offset() {
        let mut main_buf = Buffer::empty(Rect::new(2, 2, 6, 6));

        let mut scroll_view = ScrollView::new(main_buf.area.as_size(), 1);
        draw_to_scroll_view(&mut scroll_view);

        scroll_view.render(main_buf.area, &mut main_buf);

        #[rustfmt::skip]
        let mut target = Buffer::with_lines(vec![
            "ABC   ",
            "DEF   ",
            " 1:   ",
            " ABC  ",
            " DEF  ",
            "  2:  ",
        ]);
        target.area = Rect::new(2, 2, 6, 6);

        assert_eq!(main_buf, target);
    }
}
