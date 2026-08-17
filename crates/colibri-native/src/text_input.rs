//! Single-line text field adapted from gpui 0.2.2 `examples/input.rs`.
//!
//! Kept lean for the fidelity demo: focus, type, backspace, paste, select-all.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, Element, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable, GlobalElementId,
    InteractiveElement, IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PaintQuad, ParentElement, Pixels, Point, ShapedLine, SharedString, Style, Styled,
    TextRun, UTF16Selection, UnderlineStyle, Window, actions, div, fill, point, px, relative, rgb,
    rgba, size,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::theme::{DOGE_EIGHT, FIELD_MIN_H, FIELD_PAD_X, FIELD_PAD_Y, ThemePalette};

actions!(
    colibri_text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
        SelectLeft,
        SelectRight,
        MoveWordLeft,
        MoveWordRight,
        SelectWordLeft,
        SelectWordRight,
        DeleteWordBack,
        DeleteWordForward,
        SelectHome,
        SelectEnd
    ]
);

/// Primary fill with ~25% alpha for soft selection (`0xRRGGBBAA`). Mint only.
fn selection_rgba(primary: u32) -> u32 {
    ((primary & 0x00ff_ffff) << 8) | 0x40
}

/// Selection fill RGB (opaque). DOGE must stay pure eight, never a soft midtone wash.
///
/// Returns `Some(rgb)` for solid DOGE selection, `None` when mint soft alpha applies.
pub(crate) fn selection_solid_rgb_for_primary(primary: u32) -> Option<u32> {
    if DOGE_EIGHT.contains(&primary) {
        Some(primary)
    } else {
        None
    }
}

pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
    /// Active theme paint tokens (cursor, selection, field chrome).
    palette: ThemePalette,
}

impl TextInput {
    pub fn new(
        cx: &mut Context<Self>,
        content: impl Into<SharedString>,
        placeholder: impl Into<SharedString>,
        palette: ThemePalette,
    ) -> Self {
        let content: SharedString = content.into();
        let len = content.len();
        Self {
            focus_handle: cx.focus_handle(),
            content,
            placeholder: placeholder.into(),
            selected_range: len..len,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
            palette,
        }
    }

    /// Update field chrome when the shell theme changes (Tools / wizard later).
    #[allow(dead_code)]
    pub fn set_palette(&mut self, palette: ThemePalette, cx: &mut Context<Self>) {
        if self.palette != palette {
            self.palette = palette;
            cx.notify();
        }
    }

    pub fn text(&self) -> String {
        self.content.to_string()
    }

    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = text.into();
        let len = self.content.len();
        self.selected_range = len..len;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.set_text("", cx);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn move_word_left(&mut self, _: &MoveWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(
            previous_word_boundary(&self.content, self.cursor_offset()),
            cx,
        );
    }

    fn move_word_right(&mut self, _: &MoveWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(next_word_boundary(&self.content, self.cursor_offset()), cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(
            previous_word_boundary(&self.content, self.cursor_offset()),
            cx,
        );
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(next_word_boundary(&self.content, self.cursor_offset()), cx);
    }

    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(buffer_start_offset(), cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(buffer_end_offset(self.content.len()), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_word_back(
        &mut self,
        _: &DeleteWordBack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            self.select_to(
                previous_word_boundary(&self.content, self.cursor_offset()),
                cx,
            );
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_word_forward(
        &mut self,
        _: &DeleteWordForward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            self.select_to(
                next_word_delete_end(&self.content, self.cursor_offset()),
                cx,
            );
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &flatten_paste_text(&text), window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = true;
        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        line.closest_index_for_x(position.x - bounds.left())
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        previous_grapheme_boundary(&self.content, offset)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        next_grapheme_boundary(&self.content, offset)
    }
}

/// Byte offset of the grapheme before `offset`, or 0.
pub(crate) fn previous_grapheme_boundary(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    text.grapheme_indices(true)
        .rev()
        .find_map(|(idx, _)| (idx < offset).then_some(idx))
        .unwrap_or(0)
}

/// Byte offset of the grapheme after `offset`, or `text.len()`.
pub(crate) fn next_grapheme_boundary(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    text.grapheme_indices(true)
        .find_map(|(idx, _)| (idx > offset).then_some(idx))
        .unwrap_or(text.len())
}

fn word_bound_segments(text: &str) -> Vec<(usize, &str)> {
    UnicodeSegmentation::split_word_bound_indices(text).collect()
}

fn segment_is_whitespace(seg: &str) -> bool {
    !seg.is_empty() && seg.chars().all(char::is_whitespace)
}

/// Start of the word before `offset` (skip trailing whitespace, then the word).
pub(crate) fn previous_word_boundary(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    if offset == 0 {
        return 0;
    }
    let segs = word_bound_segments(text);
    let mut idx = None;
    for (i, (start, s)) in segs.iter().enumerate() {
        let end = start + s.len();
        if *start < offset && offset <= end {
            idx = Some(i);
            break;
        }
        if *start < offset {
            idx = Some(i);
        }
    }
    let Some(mut i) = idx else {
        return 0;
    };
    while i > 0 && segment_is_whitespace(segs[i].1) {
        i -= 1;
    }
    if segment_is_whitespace(segs[i].1) {
        0
    } else {
        segs[i].0
    }
}

/// Start of the next word after `offset` (skip rest of word, then whitespace).
pub(crate) fn next_word_boundary(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    if offset >= text.len() {
        return text.len();
    }
    let segs = word_bound_segments(text);
    let mut i = 0;
    let mut found = false;
    for (idx, (start, s)) in segs.iter().enumerate() {
        let end = start + s.len();
        if offset < end {
            i = idx;
            found = true;
            break;
        }
        i = idx + 1;
    }
    if !found || i >= segs.len() {
        return text.len();
    }
    if !segment_is_whitespace(segs[i].1) && segs[i].0 <= offset {
        i += 1;
    }
    while i < segs.len() && segment_is_whitespace(segs[i].1) {
        i += 1;
    }
    if i >= segs.len() {
        text.len()
    } else {
        segs[i].0
    }
}

/// End offset for Ctrl+Delete: rest of this word, or following spaces plus the next word.
pub(crate) fn next_word_delete_end(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    if offset >= text.len() {
        return text.len();
    }
    let segs = word_bound_segments(text);
    let mut i = 0;
    let mut found = false;
    for (idx, (start, s)) in segs.iter().enumerate() {
        let end = start + s.len();
        if offset < end {
            i = idx;
            found = true;
            break;
        }
        i = idx + 1;
    }
    if !found || i >= segs.len() {
        return text.len();
    }
    if segment_is_whitespace(segs[i].1) {
        while i < segs.len() && segment_is_whitespace(segs[i].1) {
            i += 1;
        }
        if i >= segs.len() {
            return text.len();
        }
        return segs[i].0 + segs[i].1.len();
    }
    segs[i].0 + segs[i].1.len()
}

/// Clipboard paste in this single-line field: newlines become spaces.
pub(crate) fn flatten_paste_text(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\n', '\r'], " ")
}

/// Buffer start for Home / Ctrl+Home (single-line field).
pub(crate) fn buffer_start_offset() -> usize {
    0
}

/// Buffer end for End / Ctrl+End (single-line field).
pub(crate) fn buffer_end_offset(len: usize) -> usize {
    len
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| new_range.start + range.start..new_range.end + range.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(
                bounds.left() + last_layout.x_for_index(range.start),
                bounds.top(),
            ),
            point(
                bounds.left() + last_layout.x_for_index(range.end),
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        let utf8_index = last_layout.index_for_x(point.x - line_point.x)?;
        Some(self.offset_to_utf16(utf8_index))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let style = window.text_style();

        let p = input.palette;
        let (display_text, text_color) = if content.is_empty() {
            (input.placeholder.clone(), rgb(p.muted).into())
        } else {
            (content, style.color)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked_range) = input.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        let cursor_pos = line.x_for_index(cursor);
        let (selection, cursor) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_pos, bounds.top()),
                        size(px(2.), bounds.bottom() - bounds.top()),
                    ),
                    rgb(p.primary),
                )),
            )
        } else {
            let sel_color = if let Some(solid) = selection_solid_rgb_for_primary(p.primary) {
                // DOGE: pure solid primary (no alpha midtone wash).
                rgb(solid)
            } else {
                rgba(selection_rgba(p.primary))
            };
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(selected_range.start),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(selected_range.end),
                            bounds.bottom(),
                        ),
                    ),
                    sel_color,
                )),
                None,
            )
        };
        PrepaintState {
            line: Some(line),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        // Clip glyph paint to field bounds so long placeholders/paths never spill.
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if let Some(selection) = prepaint.selection.take() {
                window.paint_quad(selection);
            }
            let line = prepaint.line.take().unwrap();
            line.paint(bounds.origin, window.line_height(), window, cx)
                .unwrap();

            if focus_handle.is_focused(window)
                && let Some(cursor) = prepaint.cursor.take()
            {
                window.paint_quad(cursor);
            }

            self.input.update(cx, |input, _cx| {
                input.last_layout = Some(line);
                input.last_bounds = Some(bounds);
            });
        });
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl gpui::Render for TextInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        div()
            .flex()
            .key_context("TextInput")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::move_word_left))
            .on_action(cx.listener(Self::move_word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::delete_word_back))
            .on_action(cx.listener(Self::delete_word_forward))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .w_full()
            .min_w_0()
            .line_height(px(22.))
            .text_size(px(14.))
            .text_color(rgb(p.text))
            .child(
                div()
                    .h(px(FIELD_MIN_H))
                    .w_full()
                    .min_w_0()
                    .px(px(FIELD_PAD_X))
                    .py(px(FIELD_PAD_Y))
                    .overflow_hidden()
                    .bg(rgb(p.secondary))
                    .border_1()
                    .border_color(rgb(p.border))
                    .child(TextElement { input: cx.entity() }),
            )
    }
}

/// Bind common text field shortcuts on the app (call once at startup).
pub fn bind_text_input_keys(cx: &mut App) {
    cx.bind_keys(text_input_key_bindings());
}

/// Same table `bind_text_input_keys` installs. Tests inspect this, not a fake Field.
pub(crate) fn text_input_key_bindings() -> Vec<gpui::KeyBinding> {
    use gpui::KeyBinding;
    vec![
        KeyBinding::new("backspace", Backspace, Some("TextInput")),
        KeyBinding::new("delete", Delete, Some("TextInput")),
        KeyBinding::new("left", Left, Some("TextInput")),
        KeyBinding::new("right", Right, Some("TextInput")),
        KeyBinding::new("home", Home, Some("TextInput")),
        KeyBinding::new("end", End, Some("TextInput")),
        KeyBinding::new("cmd-a", SelectAll, Some("TextInput")),
        KeyBinding::new("ctrl-a", SelectAll, Some("TextInput")),
        KeyBinding::new("cmd-c", Copy, Some("TextInput")),
        KeyBinding::new("ctrl-c", Copy, Some("TextInput")),
        KeyBinding::new("cmd-x", Cut, Some("TextInput")),
        KeyBinding::new("ctrl-x", Cut, Some("TextInput")),
        KeyBinding::new("cmd-v", Paste, Some("TextInput")),
        KeyBinding::new("ctrl-v", Paste, Some("TextInput")),
        KeyBinding::new("shift-left", SelectLeft, Some("TextInput")),
        KeyBinding::new("shift-right", SelectRight, Some("TextInput")),
        KeyBinding::new("shift-home", SelectHome, Some("TextInput")),
        KeyBinding::new("shift-end", SelectEnd, Some("TextInput")),
        KeyBinding::new("ctrl-left", MoveWordLeft, Some("TextInput")),
        KeyBinding::new("ctrl-right", MoveWordRight, Some("TextInput")),
        KeyBinding::new("cmd-left", MoveWordLeft, Some("TextInput")),
        KeyBinding::new("cmd-right", MoveWordRight, Some("TextInput")),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, Some("TextInput")),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, Some("TextInput")),
        KeyBinding::new("cmd-shift-left", SelectWordLeft, Some("TextInput")),
        KeyBinding::new("cmd-shift-right", SelectWordRight, Some("TextInput")),
        KeyBinding::new("ctrl-backspace", DeleteWordBack, Some("TextInput")),
        KeyBinding::new("cmd-backspace", DeleteWordBack, Some("TextInput")),
        KeyBinding::new("ctrl-delete", DeleteWordForward, Some("TextInput")),
        KeyBinding::new("cmd-delete", DeleteWordForward, Some("TextInput")),
        KeyBinding::new("ctrl-home", Home, Some("TextInput")),
        KeyBinding::new("ctrl-end", End, Some("TextInput")),
        KeyBinding::new("cmd-home", Home, Some("TextInput")),
        KeyBinding::new("cmd-end", End, Some("TextInput")),
        KeyBinding::new("ctrl-shift-home", SelectHome, Some("TextInput")),
        KeyBinding::new("ctrl-shift-end", SelectEnd, Some("TextInput")),
        KeyBinding::new("cmd-shift-home", SelectHome, Some("TextInput")),
        KeyBinding::new("cmd-shift-end", SelectEnd, Some("TextInput")),
    ]
}

#[cfg(test)]
mod composer_key_tests {
    use super::*;

    struct Field {
        content: String,
        sel: std::ops::Range<usize>,
        reversed: bool,
    }

    impl Field {
        fn at_end(text: &str) -> Self {
            let len = text.len();
            Self {
                content: text.to_string(),
                sel: len..len,
                reversed: false,
            }
        }

        fn at(text: &str, offset: usize) -> Self {
            Self {
                content: text.to_string(),
                sel: offset..offset,
                reversed: false,
            }
        }

        fn cursor(&self) -> usize {
            if self.reversed {
                self.sel.start
            } else {
                self.sel.end
            }
        }

        fn select_to(&mut self, offset: usize) {
            let offset = offset.min(self.content.len());
            if self.reversed {
                self.sel.start = offset;
            } else {
                self.sel.end = offset;
            }
            if self.sel.end < self.sel.start {
                self.reversed = !self.reversed;
                self.sel = self.sel.end..self.sel.start;
            }
        }

        fn delete_sel(&mut self) {
            self.content.replace_range(self.sel.start..self.sel.end, "");
            let at = self.sel.start;
            self.sel = at..at;
            self.reversed = false;
        }

        fn delete_word_back(&mut self) {
            if self.sel.is_empty() {
                self.select_to(previous_word_boundary(&self.content, self.cursor()));
            }
            self.delete_sel();
        }

        fn delete_word_forward(&mut self) {
            if self.sel.is_empty() {
                self.select_to(next_word_delete_end(&self.content, self.cursor()));
            }
            self.delete_sel();
        }
    }

    #[test]
    fn word_left_from_after_space_lands_on_word_start() {
        let text = "hello world";
        let at = text.len();
        let next = previous_word_boundary(text, at);
        assert_eq!(&text[next..], "world", "offset {next}");
        assert_eq!(&text[..next], "hello ");
    }

    #[test]
    fn word_right_from_start_skips_to_next_word() {
        let text = "hello world";
        let next = next_word_boundary(text, 0);
        assert_eq!(next, text.find('w').expect("w"));
    }

    #[test]
    fn word_left_from_mid_word_lands_on_that_word_start() {
        let text = "hello world";
        let at = text.find('r').expect("r");
        let next = previous_word_boundary(text, at);
        assert_eq!(next, text.find('w').expect("w"));
        assert_eq!(&text[next..], "world");
    }

    #[test]
    fn ctrl_backspace_deletes_previous_word_and_leading_spaces() {
        let mut field = Field::at_end("hello world");
        field.delete_word_back();
        assert_eq!(field.content, "hello ");
        let mut with_trail = Field::at_end("hello world  ");
        with_trail.delete_word_back();
        assert_eq!(with_trail.content, "hello ");
    }

    #[test]
    fn ctrl_backspace_with_selection_deletes_selection_only() {
        let mut field = Field::at("hello world", 0);
        field.sel = 6..11;
        field.delete_word_back();
        assert_eq!(field.content, "hello ");
        assert!(field.sel.is_empty());
        assert_eq!(field.sel.start, 6);
    }

    #[test]
    fn ctrl_delete_deletes_next_word() {
        let mut field = Field::at("hello world", 6);
        field.delete_word_forward();
        assert_eq!(field.content, "hello ");
    }

    fn action_name_for_keystroke(key: &str) -> Option<String> {
        let ks = gpui::Keystroke::parse(key).ok()?;
        text_input_key_bindings().into_iter().find_map(|b| {
            (b.match_keystrokes(std::slice::from_ref(&ks)) == Some(false))
                .then(|| b.action().name().to_string())
        })
    }

    fn action_name_ends_with(name: &str, suffix: &str) -> bool {
        name == suffix || name.ends_with(&format!("::{suffix}"))
    }

    #[test]
    fn home_and_end_bindings_are_buffer_start_and_end() {
        let home = action_name_for_keystroke("home").expect("home bound");
        let end = action_name_for_keystroke("end").expect("end bound");
        assert!(
            action_name_ends_with(&home, "Home"),
            "home must bind Home, got {home}"
        );
        assert!(
            action_name_ends_with(&end, "End"),
            "end must bind End, got {end}"
        );
        assert!(
            !action_name_ends_with(&home, "MoveWordLeft"),
            "home must not word-move"
        );
        assert!(
            !action_name_ends_with(&end, "MoveWordRight"),
            "end must not word-move"
        );
        assert_eq!(buffer_start_offset(), 0);
        assert_eq!(buffer_end_offset("hello world".len()), "hello world".len());
    }

    #[test]
    fn ctrl_home_and_ctrl_end_bind_same_actions_as_home_and_end() {
        let home = action_name_for_keystroke("home").expect("home");
        let end = action_name_for_keystroke("end").expect("end");
        for key in ["ctrl-home", "cmd-home"] {
            let got = action_name_for_keystroke(key).unwrap_or_else(|| panic!("{key} unbound"));
            assert_eq!(got, home, "{key} must alias Home, not word-move");
        }
        for key in ["ctrl-end", "cmd-end"] {
            let got = action_name_for_keystroke(key).unwrap_or_else(|| panic!("{key} unbound"));
            assert_eq!(got, end, "{key} must alias End, not word-move");
        }
    }

    #[test]
    fn select_word_left_extends_to_word_start() {
        let text = "hello world";
        let at = text.find('r').expect("r");
        let start = previous_word_boundary(text, at);
        assert_eq!(start, text.find('w').expect("w"));
        let mut field = Field::at(text, at);
        field.select_to(start);
        assert_eq!(field.sel, start..at);
        assert!(field.reversed);
    }

    #[test]
    fn text_input_bindings_do_not_bind_enter() {
        for key in ["enter", "return"] {
            if let Some(name) = action_name_for_keystroke(key) {
                panic!("{key} must not be bound (no Enter-to-send), got {name}");
            }
        }
    }

    #[test]
    fn ctrl_delete_from_spaces_eats_spaces_and_next_word() {
        let text = "hello   world";
        let at = text.find(' ').expect("space");
        let mut field = Field::at(text, at);
        field.delete_word_forward();
        assert_eq!(field.content, "hello");
    }

    #[test]
    fn select_home_keeps_anchor_at_cursor() {
        let mut field = Field::at("hello world", 5);
        field.select_to(0);
        assert_eq!(field.sel, 0..5);
        assert!(field.reversed, "anchor stays at the original cursor");
    }

    #[test]
    fn shift_left_extends_one_grapheme() {
        let text = "hello";
        let end = text.len();
        let prev = previous_grapheme_boundary(text, end);
        assert_eq!(prev, 4);
        let mut field = Field::at_end(text);
        field.select_to(previous_grapheme_boundary(&field.content, field.cursor()));
        assert_eq!(field.sel, 4..5);
        assert!(field.reversed);
    }

    #[test]
    fn paste_flattens_newlines() {
        assert_eq!(flatten_paste_text("hello\nworld"), "hello world");
        assert_eq!(flatten_paste_text("a\r\nb"), "a b");
        assert_eq!(flatten_paste_text("a\rb"), "a b");
        assert_eq!(flatten_paste_text("no-break"), "no-break");
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    use crate::theme::{DOGE_GREEN, doge_palette, mint_palette};

    #[test]
    fn doge_selection_fill_is_pure_eight_or_opaque_primary() {
        let primary = doge_palette().primary;
        assert_eq!(primary, DOGE_GREEN);
        let solid = selection_solid_rgb_for_primary(primary).expect("DOGE solid");
        assert!(
            DOGE_EIGHT.contains(&solid),
            "DOGE selection 0x{solid:06X} not in eight"
        );
        assert_eq!(solid, primary);
    }

    #[test]
    fn mint_selection_uses_soft_alpha_path() {
        let primary = mint_palette().primary;
        assert!(
            selection_solid_rgb_for_primary(primary).is_none(),
            "mint should keep soft selection wash"
        );
        let packed = selection_rgba(primary);
        assert_eq!(packed & 0xff, 0x40, "mint soft alpha");
    }
}
