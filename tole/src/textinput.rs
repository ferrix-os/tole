//! `TextInput` and `TextEdit` (`docs/UI-FRAMEWORK.md` §4.5): text a
//! person types into, with a caret, a selection by the pointer and the
//! keys, and the clipboard.
//!
//! A `TextInput` is one line, scrolled across so the caret stays in
//! view; a `TextEdit` is lines, broken at its width when `wrapMode` says.
//! Both keep the text, the caret and the selection's other end
//! (`anchor`) as properties, so what they draw follows them as any
//! binding does. Positions are byte offsets into the text, always on a
//! character's edge.
//!
//! Keys reach one after its `Keys` handlers, which may take them first.
//! A letter is typed only with neither Control nor Alt held, so Ctrl+V
//! pastes and never types `v`. Ctrl+C, Ctrl+X and Ctrl+V go through the
//! `Ui`'s clipboard: a window hands a copy to the desktop's and answers
//! a paste from it ([`Ui::take_copied`], [`Ui::take_paste_request`],
//! [`Ui::paste`]); without one, the `Ui` keeps the copy itself.

use std::rc::Rc;

use compositor_text::Run;

use crate::focus::KeyEvent;
use crate::item::{ItemId, Kind, TextInputProps};
use crate::render::{DrawOp, Rect};
use crate::ui::Ui;
use crate::value::Font;

/// `echoMode`'s values, as Qt numbers them.
pub mod echo {
    /// The text as it is.
    pub const NORMAL: i32 = 0;
    /// A dot for each character: a password.
    pub const PASSWORD: i32 = 2;
}

/// `wrapMode`'s values, as Qt numbers them.
pub mod wrap {
    /// Lines run on past the width.
    pub const NO_WRAP: i32 = 0;
    /// Lines break between words at the width, inside a word when one is
    /// wider.
    pub const WRAP: i32 = 4;
}

/// What a password shows for each character.
const DOT: char = '•';

/// The clipboard as the `Ui` holds it.
#[derive(Debug, Default)]
pub(crate) struct Clipboard {
    /// The last copy, pasted from when there is no window's clipboard.
    held: Option<Rc<str>>,
    /// A copy the window has not taken yet.
    copied: Option<Rc<str>>,
    /// The item a paste was asked for in, until the window answers.
    paste_into: Option<ItemId>,
    /// Whether a window answers pastes from the desktop's clipboard.
    external: bool,
}

/// One line as drawn: the part of the text it shows and its shaped run.
#[derive(Clone, Debug)]
struct Line {
    /// Where it starts in the text.
    start: usize,
    /// Where it ends (before a line break).
    end: usize,
    /// What it shows: the text, or dots.
    shown: Rc<str>,
    /// `shown`, shaped, in buffer pixels.
    run: Rc<Run>,
    /// Its top, in logical pixels from the item's.
    top: f32,
}

/// An item's text, laid out.
#[derive(Clone, Debug)]
struct Laid {
    /// Its lines, one for a `TextInput`.
    lines: Vec<Line>,
    /// A line's height, in logical pixels.
    line_height: f32,
    /// The ascent, in buffer pixels.
    ascent: f32,
    /// Whether it shows dots.
    password: bool,
}

impl Laid {
    /// The line holding `offset`: the last one starting at or before it.
    fn line_of(&self, offset: usize) -> usize {
        self.lines
            .iter()
            .rposition(|line| line.start <= offset)
            .unwrap_or(0)
    }
}

/// The byte offset in what a line shows of `offset` in the text: the same,
/// or for dots, as many dots as characters before it.
fn shown_offset(text: &str, line: &Line, offset: usize, password: bool) -> usize {
    let offset = offset.clamp(line.start, line.end);
    if password {
        text.get(line.start..offset)
            .map_or(0, |part| part.chars().count() * DOT.len_utf8())
    } else {
        offset - line.start
    }
}

/// Where in a run the pen is before byte `at` of its text, in buffer
/// pixels.
fn run_x(run: &Run, at: usize) -> f32 {
    run.glyphs
        .iter()
        .filter(|glyph| glyph.cluster >= at)
        .map(|glyph| glyph.x)
        .fold(None, |best: Option<f32>, x| {
            Some(best.map_or(x, |b| b.min(x)))
        })
        .unwrap_or(run.width)
}

/// The character edge at or before `at`.
fn floor_edge(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// The edge of the character after `at`.
fn next_edge(text: &str, at: usize) -> usize {
    text.get(at..)
        .and_then(|rest| rest.chars().next())
        .map_or(text.len(), |c| at + c.len_utf8())
}

/// The edge of the character before `at`.
fn prev_edge(text: &str, at: usize) -> usize {
    text.get(..at)
        .and_then(|part| part.chars().next_back())
        .map_or(0, |c| at - c.len_utf8())
}

/// Whether `c` is part of a word, for word moves and double clicks.
fn wordy(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The start of the word at or before `at`, skipping spaces first.
fn word_left(text: &str, at: usize) -> usize {
    let mut at = at;
    while at > 0
        && !text
            .get(..at)
            .and_then(|p| p.chars().next_back())
            .is_some_and(wordy)
    {
        at = prev_edge(text, at);
    }
    while at > 0
        && text
            .get(..at)
            .and_then(|p| p.chars().next_back())
            .is_some_and(wordy)
    {
        at = prev_edge(text, at);
    }
    at
}

/// The end of the word at or after `at`, skipping spaces first.
fn word_right(text: &str, at: usize) -> usize {
    let mut at = at;
    while at < text.len()
        && !text
            .get(at..)
            .and_then(|p| p.chars().next())
            .is_some_and(wordy)
    {
        at = next_edge(text, at);
    }
    while at < text.len()
        && text
            .get(at..)
            .and_then(|p| p.chars().next())
            .is_some_and(wordy)
    {
        at = next_edge(text, at);
    }
    at
}

impl Ui {
    /// A `TextInput`'s or `TextEdit`'s own properties.
    #[must_use]
    pub fn text_input(&self, item: ItemId) -> Option<TextInputProps> {
        match self.kind(item)? {
            Kind::TextInput(props) => Some(props),
            _ => None,
        }
    }

    /// Say that a window answers pastes from the desktop's clipboard: a
    /// paste then waits for [`Ui::paste`] rather than taking the `Ui`'s
    /// own last copy.
    pub fn set_clipboard_external(&mut self, external: bool) {
        self.clipboard.external = external;
    }

    /// A copy made since the last call, for the window to offer the
    /// desktop.
    pub fn take_copied(&mut self) -> Option<Rc<str>> {
        self.clipboard.copied.take()
    }

    /// Whether a paste was asked for since the last call: the window
    /// reads the desktop's clipboard and gives it to [`Ui::paste`].
    pub fn take_paste_request(&mut self) -> bool {
        self.clipboard.paste_into.is_some()
    }

    /// Paste `text` where a paste was asked for, if anywhere; an empty
    /// one (the desktop's clipboard held no text) only ends the request.
    /// A one-line field takes the first line that is not blank.
    pub fn paste(&mut self, text: &str) {
        let Some(item) = self.clipboard.paste_into.take() else {
            return;
        };
        // One line takes the first line of what was copied that has
        // anything on it; lines take it whole.
        let one_line = self.text_input(item).is_some_and(|p| !p.multiline);
        let text = if one_line {
            text.lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("")
                .trim_end_matches('\r')
        } else {
            text
        };
        if !text.is_empty() {
            self.insert_typed(item, text);
        }
    }

    /// Select the whole text of a `TextInput` or `TextEdit`.
    pub fn select_all(&mut self, item: ItemId) {
        let Some(p) = self.text_input(item) else {
            return;
        };
        let len = self.get(p.text).len();
        p.anchor.set(self, 0);
        p.cursor_position
            .set(self, i32::try_from(len).unwrap_or(i32::MAX));
    }

    /// The caret and the selection's other end, on character edges.
    fn caret(&mut self, p: TextInputProps) -> (usize, usize) {
        let text = self.get(p.text);
        let at = |v: i32| floor_edge(&text, usize::try_from(v).unwrap_or(0));
        (at(self.get(p.cursor_position)), at(self.get(p.anchor)))
    }

    /// Put the caret at `cursor`, the selection's other end at `anchor`.
    fn set_caret(&mut self, p: TextInputProps, cursor: usize, anchor: usize) {
        p.anchor
            .set(self, i32::try_from(anchor).unwrap_or(i32::MAX));
        p.cursor_position
            .set(self, i32::try_from(cursor).unwrap_or(i32::MAX));
    }

    /// Replace `from..to` with `with`, the caret after it.
    fn replace(&mut self, item: ItemId, p: TextInputProps, (from, to): (usize, usize), with: &str) {
        let text = self.get(p.text);
        let (from, to) = (from.min(to), from.max(to));
        let (Some(head), Some(tail)) = (text.get(..from), text.get(to..)) else {
            return;
        };
        let with: String = if p.multiline {
            with.replace("\r\n", "\n")
        } else {
            with.replace(['\r', '\n'], " ")
        };
        let next = format!("{head}{with}{tail}");
        p.text.set(self, Rc::from(next));
        let at = from + with.len();
        self.set_caret(p, at, at);
        let _ = item;
        self.emit(p.text_edited, Vec::new());
    }

    /// Type `text` at the caret, over the selection.
    fn insert_typed(&mut self, item: ItemId, text: &str) {
        let Some(p) = self.text_input(item) else {
            return;
        };
        if self.get(p.read_only) {
            return;
        }
        let (cursor, anchor) = self.caret(p);
        self.replace(item, p, (cursor, anchor), text);
    }

    /// The selected text.
    fn selection(&mut self, p: TextInputProps) -> Option<Rc<str>> {
        let (cursor, anchor) = self.caret(p);
        if cursor == anchor {
            return None;
        }
        let text = self.get(p.text);
        text.get(cursor.min(anchor)..cursor.max(anchor))
            .map(Rc::from)
    }

    /// Copy the selection: kept, and handed to the window.
    fn copy_selection(&mut self, p: TextInputProps) -> bool {
        if self.get(p.echo_mode) == echo::PASSWORD {
            return false;
        }
        let Some(text) = self.selection(p) else {
            return false;
        };
        self.clipboard.held = Some(Rc::clone(&text));
        self.clipboard.copied = Some(text);
        true
    }

    /// A key for the focused `TextInput` or `TextEdit` `item`: whether it
    /// took it.
    pub(crate) fn text_key(&mut self, item: ItemId, event: &KeyEvent) -> bool {
        let Some(p) = self.text_input(item) else {
            return false;
        };
        let text = self.get(p.text);
        let (cursor, anchor) = self.caret(p);
        let read_only = self.get(p.read_only);
        let (lo, hi) = (cursor.min(anchor), cursor.max(anchor));
        // Where a move goes, the selection kept with Shift.
        let moved = |ui: &mut Ui, to: usize| {
            let anchor = if event.shift { anchor } else { to };
            ui.set_caret(p, to, anchor);
            true
        };
        if event.control && !event.alt {
            match &*event.key {
                "a" | "A" => {
                    self.select_all(item);
                    return true;
                }
                "c" | "C" | "Insert" => {
                    let _ = self.copy_selection(p);
                    return true;
                }
                "x" | "X" => {
                    if !read_only && self.copy_selection(p) {
                        self.replace(item, p, (lo, hi), "");
                    }
                    return true;
                }
                "v" | "V" => {
                    if !read_only {
                        self.ask_paste(item);
                    }
                    return true;
                }
                "Left" => return moved(self, word_left(&text, cursor)),
                "Right" => return moved(self, word_right(&text, cursor)),
                "Home" => return moved(self, 0),
                "End" => return moved(self, text.len()),
                "BackSpace" if !read_only => {
                    let from = if lo == hi {
                        word_left(&text, cursor)
                    } else {
                        lo
                    };
                    self.replace(item, p, (from, hi), "");
                    return true;
                }
                "Delete" if !read_only => {
                    let to = if lo == hi {
                        word_right(&text, cursor)
                    } else {
                        hi
                    };
                    self.replace(item, p, (lo, to), "");
                    return true;
                }
                _ => return false,
            }
        }
        match &*event.key {
            "Left" => {
                let to = if lo != hi && !event.shift {
                    lo
                } else {
                    prev_edge(&text, cursor)
                };
                moved(self, to)
            }
            "Right" => {
                let to = if lo != hi && !event.shift {
                    hi
                } else {
                    next_edge(&text, cursor)
                };
                moved(self, to)
            }
            "Home" | "End" => {
                let laid = self.laid_out(item, p);
                let line = laid.lines.get(laid.line_of(cursor));
                let to = match (line, &*event.key) {
                    (Some(line), "Home") => line.start,
                    (Some(line), _) => line.end,
                    (None, "Home") => 0,
                    (None, _) => text.len(),
                };
                moved(self, to)
            }
            "Up" | "Down" if p.multiline => {
                let laid = self.laid_out(item, p);
                let at = laid.line_of(cursor);
                let next = if &*event.key == "Up" {
                    at.checked_sub(1)
                } else {
                    Some(at + 1).filter(|n| *n < laid.lines.len())
                };
                let to = match next {
                    Some(next) => {
                        let x = self.caret_x(&text, &laid, cursor);
                        laid.lines
                            .get(next)
                            .map_or(cursor, |line| self.offset_in_line(&text, &laid, line, x))
                    }
                    None if &*event.key == "Up" => 0,
                    None => text.len(),
                };
                moved(self, to)
            }
            "BackSpace" if !read_only => {
                let from = if lo == hi {
                    prev_edge(&text, cursor)
                } else {
                    lo
                };
                self.replace(item, p, (from, hi), "");
                true
            }
            "Delete" if !read_only => {
                let to = if lo == hi {
                    next_edge(&text, cursor)
                } else {
                    hi
                };
                self.replace(item, p, (lo, to), "");
                true
            }
            "Return" | "KP_Enter" => {
                if p.multiline {
                    if !read_only {
                        self.replace(item, p, (lo, hi), "\n");
                    }
                } else {
                    self.emit(p.accepted, Vec::new());
                }
                true
            }
            _ => {
                // A letter types only with neither Control nor Alt held.
                let typed: String = event.text.chars().filter(|c| !c.is_control()).collect();
                if typed.is_empty() || event.alt || read_only {
                    return false;
                }
                self.replace(item, p, (lo, hi), &typed);
                true
            }
        }
    }

    /// A paste in `item`: from the window's clipboard when it answers
    /// them, else the `Ui`'s last copy.
    fn ask_paste(&mut self, item: ItemId) {
        if self.clipboard.external {
            self.clipboard.paste_into = Some(item);
        } else if let Some(text) = self.clipboard.held.clone() {
            self.insert_typed(item, &text);
        }
    }

    /// The pointer pressed at (`x`, `y`) in `item`'s own coordinates, the
    /// `clicks`th press in a row: the caret goes there (the selection
    /// stretches to it with Shift); a second click takes the word, a
    /// third the line.
    pub(crate) fn text_press(
        &mut self,
        item: ItemId,
        (x, y): (f32, f32),
        clicks: u32,
        shift: bool,
    ) {
        let Some(p) = self.text_input(item) else {
            return;
        };
        let text = self.get(p.text);
        let at = self.offset_at(item, p, x, y);
        let (_, anchor) = self.caret(p);
        match clicks {
            1 if shift => self.set_caret(p, at, anchor),
            1 => self.set_caret(p, at, at),
            2 => {
                let mut start = at;
                while start > 0
                    && text
                        .get(..start)
                        .and_then(|t| t.chars().next_back())
                        .is_some_and(wordy)
                {
                    start = prev_edge(&text, start);
                }
                let mut end = at;
                while end < text.len()
                    && text
                        .get(end..)
                        .and_then(|t| t.chars().next())
                        .is_some_and(wordy)
                {
                    end = next_edge(&text, end);
                }
                self.set_caret(p, end, start);
            }
            _ => {
                let laid = self.laid_out(item, p);
                match laid.lines.get(laid.line_of(at)) {
                    Some(line) => self.set_caret(p, line.end, line.start),
                    None => self.set_caret(p, text.len(), 0),
                }
            }
        }
    }

    /// The pointer moved to (`x`, `y`) in `item` while a press it took
    /// is held: the selection stretches to there.
    pub(crate) fn text_drag(&mut self, item: ItemId, (x, y): (f32, f32)) {
        let Some(p) = self.text_input(item) else {
            return;
        };
        let at = self.offset_at(item, p, x, y);
        let (_, anchor) = self.caret(p);
        self.set_caret(p, at, anchor);
    }

    /// The text offset nearest (`x`, `y`), in `item`'s coordinates.
    fn offset_at(&mut self, item: ItemId, p: TextInputProps, x: f32, y: f32) -> usize {
        let text = self.get(p.text);
        let laid = self.laid_out(item, p);
        let index = if laid.line_height > 0.0 {
            (y / laid.line_height).floor().max(0.0) as usize
        } else {
            0
        };
        let index = index.min(laid.lines.len().saturating_sub(1));
        let scroll = self.item(item).map_or(0.0, |data| data.text_scroll);
        match laid.lines.get(index) {
            Some(line) => self.offset_in_line(&text, &laid, line, x + scroll),
            None => 0,
        }
    }

    /// The offset in `line` nearest `x` logical pixels from its start.
    fn offset_in_line(&mut self, text: &str, laid: &Laid, line: &Line, x: f32) -> usize {
        let scale = self.scale();
        let mut best = (line.start, f32::INFINITY);
        let mut at = line.start;
        loop {
            let shown = shown_offset(text, line, at, laid.password);
            let gap = (run_x(&line.run, shown) / scale - x).abs();
            if gap < best.1 {
                best = (at, gap);
            }
            if at >= line.end {
                break;
            }
            at = next_edge(text, at);
        }
        best.0
    }

    /// Where the caret at `offset` is across its line, logical pixels.
    fn caret_x(&mut self, text: &str, laid: &Laid, offset: usize) -> f32 {
        let scale = self.scale();
        laid.lines.get(laid.line_of(offset)).map_or(0.0, |line| {
            run_x(&line.run, shown_offset(text, line, offset, laid.password)) / scale
        })
    }

    /// `item`'s text in lines at its width.
    fn laid_out(&mut self, item: ItemId, p: TextInputProps) -> Laid {
        let width = self.common(item).map_or(0.0, |c| self.get(c.width));
        self.lay_out(p, width)
    }

    /// `p`'s text in lines at `width`.
    fn lay_out(&mut self, p: TextInputProps, width: f32) -> Laid {
        let text = self.get(p.text);
        let font = self.get(p.font);
        let scale = self.scale();
        let (ascent, descent) = self.text.extent(&font, scale);
        let line_height = (ascent + descent) / scale;
        let password = !p.multiline && self.get(p.echo_mode) == echo::PASSWORD;
        let wrapping = p.multiline && self.get(p.wrap_mode) != wrap::NO_WRAP && width > 0.0;
        let mut spans: Vec<(usize, usize)> = Vec::new();
        let mut start = 0;
        for hard in text.split('\n') {
            let end = start + hard.len();
            if p.multiline {
                if wrapping {
                    self.wrap(&text, (start, end), &font, width * scale, &mut spans);
                } else {
                    spans.push((start, end));
                }
            }
            start = end + 1;
        }
        if !p.multiline {
            spans = vec![(0, text.len())];
        }
        let mut lines = Vec::new();
        for (index, (start, end)) in spans.into_iter().enumerate() {
            let part = text.get(start..end).unwrap_or("");
            let shown: Rc<str> = if password {
                Rc::from(DOT.to_string().repeat(part.chars().count()))
            } else {
                Rc::from(part)
            };
            let run = self.text.run(&font, scale, &shown);
            lines.push(Line {
                start,
                end,
                shown,
                run,
                top: index as f32 * line_height,
            });
        }
        Laid {
            lines,
            line_height,
            ascent,
            password,
        }
    }

    /// One hard line, `start..end` of `text`, broken into lines no wider
    /// than `max` buffer pixels: between words, inside one only when it
    /// is wider alone.
    fn wrap(
        &mut self,
        text: &str,
        (start, end): (usize, usize),
        font: &Font,
        max: f32,
        out: &mut Vec<(usize, usize)>,
    ) {
        let whole: Rc<str> = Rc::from(text.get(start..end).unwrap_or(""));
        let scale = self.scale();
        let run = self.text.run(font, scale, &whole);
        let x = |at: usize| run_x(&run, at - start);
        let mut from = start;
        while from < end {
            if x(end) - x(from) <= max {
                break;
            }
            // The last break after a space that fits, else the last
            // character that fits, else one character.
            let mut fits_word = None;
            let mut fits_char = None;
            let mut at = next_edge(text, from);
            while at <= end && x(at) - x(from) <= max {
                fits_char = Some(at);
                if text.get(..at).and_then(|t| t.chars().next_back()) == Some(' ') {
                    fits_word = Some(at);
                }
                if at == end {
                    break;
                }
                at = next_edge(text, at);
            }
            let cut = fits_word
                .or(fits_char)
                .unwrap_or_else(|| next_edge(text, from));
            out.push((from, cut));
            from = cut;
        }
        out.push((from, end));
    }

    /// `item`'s implicit size: its text's width and its lines' height.
    pub(crate) fn text_input_implicit(
        &mut self,
        p: TextInputProps,
        width: Option<f32>,
    ) -> (f32, f32) {
        let laid = self.lay_out(p, width.unwrap_or(0.0));
        let scale = self.scale();
        let widest = laid
            .lines
            .iter()
            .map(|line| line.run.width / scale)
            .fold(0.0_f32, f32::max);
        let lines = laid.lines.len().max(1) as f32;
        // Room for the caret after the last character.
        (widest + 1.0, lines * laid.line_height)
    }

    /// A `TextInput`'s or `TextEdit`'s drawing in `place`: the selection,
    /// the text and the caret, cut at its edges.
    pub(crate) fn text_input_ops(
        &mut self,
        item: ItemId,
        p: TextInputProps,
        place: Rect,
        ops: &mut Vec<DrawOp>,
    ) {
        let text = self.get(p.text);
        let laid = self.lay_out(p, place.w);
        let (cursor, anchor) = self.caret(p);
        let focused = self.common(item).is_some_and(|c| self.get(c.active_focus));
        let scale = self.scale();
        let color = self.get(p.color);
        let selection = self.get(p.selection_color);
        // A line's top: one line is centred down the item.
        let top = if p.multiline {
            place.y
        } else {
            place.y + (place.h - laid.line_height) / 2.0
        };
        // Across: scrolled so the caret stays in view, as little as can be.
        let caret_x = self.caret_x(&text, &laid, cursor);
        let mut scroll = self.item(item).map_or(0.0, |data| data.text_scroll);
        if p.multiline {
            scroll = 0.0;
        } else {
            let widest = laid.lines.first().map_or(0.0, |l| l.run.width / scale) + 1.0;
            if caret_x - scroll > place.w - 1.0 {
                scroll = caret_x - place.w + 1.0;
            }
            if caret_x < scroll {
                scroll = caret_x;
            }
            scroll = scroll.min((widest - place.w).max(0.0)).max(0.0);
        }
        if let Some(data) = self.item_mut(item) {
            data.text_scroll = scroll;
        }
        let left = place.x - scroll;
        ops.push(DrawOp::PushClip(place));
        let (lo, hi) = (cursor.min(anchor), cursor.max(anchor));
        for line in &laid.lines {
            let y = top + line.top;
            if lo != hi && lo <= line.end && hi >= line.start {
                let x0 = run_x(&line.run, shown_offset(&text, line, lo, laid.password)) / scale;
                let x1 = run_x(&line.run, shown_offset(&text, line, hi, laid.password)) / scale;
                // A selection going on past the line's end shows a space's
                // width there.
                let past = if hi > line.end && p.multiline {
                    4.0
                } else {
                    0.0
                };
                if x1 + past > x0 {
                    ops.push(DrawOp::Fill {
                        rect: Rect::new(left + x0, y, x1 - x0 + past, laid.line_height),
                        radius: 0.0,
                        color: selection,
                    });
                }
            }
            if !line.shown.is_empty() && !color.is_transparent() {
                ops.push(DrawOp::Text {
                    run: Rc::clone(&line.run),
                    x: (left * scale).round(),
                    baseline: (y * scale + laid.ascent).round(),
                    color,
                });
            }
        }
        if focused && !self.get(p.read_only) {
            let line = laid.lines.get(laid.line_of(cursor));
            let y = top + line.map_or(0.0, |l| l.top);
            let x = ((left + caret_x) * scale).round() / scale;
            ops.push(DrawOp::Fill {
                rect: Rect::new(x, y, 1.0, laid.line_height),
                radius: 0.0,
                color: self.get(p.cursor_color),
            });
        }
        ops.push(DrawOp::PopClip);
    }
}
