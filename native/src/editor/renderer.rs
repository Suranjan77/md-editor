use crate::app::resolve_relative_link_path;
use crate::pdf_links::parse_pdf_link;
use iced::advanced::graphics::core::event::Event;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::keyboard;
use iced::mouse;
use iced::{Color, Element, Length, Point, Rectangle, Size};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use crate::editor::buffer::{DocBuffer, EditorCommand, Movement};
use crate::editor::highlight::StyledLine;
use crate::editor::layout_cache::{LineHeightCache, line_hash, resource_hash};
use crate::{search, theme};

const MARGIN_LEFT: f32 = 64.0;
const MARGIN_RIGHT: f32 = 56.0;
const TEXT_X_OFFSET: f32 = MARGIN_LEFT;
const TOP_PAD: f32 = 24.0;
const BASE_LINE_HEIGHT: f32 = 36.0;
const IMAGE_HEIGHT: f32 = 280.0;
const HORIZONTAL_SCROLLBAR_GUTTER: f32 = 16.0;
const HOT_PATH_BLOCK_SCAN_LIMIT: usize = 256;

// ── Widget ───────────────────────────────────────────────────────────

#[allow(clippy::type_complexity)]
pub struct Editor<'a, Message> {
    buffer: &'a DocBuffer,
    lines: &'a [StyledLine],
    image_cache: &'a HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    math_cache: &'a HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    search_query: &'a str,
    search_regex: bool,
    search_match_case: bool,
    active_search_match: Option<(usize, usize)>,
    modifiers: keyboard::Modifiers,
    on_command: Box<dyn Fn(EditorCommand) -> Message + 'a>,
    on_pointer_command: Box<dyn Fn(EditorCommand) -> Message + 'a>,
    on_link_click: Box<dyn Fn(String) -> Message + 'a>,
    on_checkbox_toggle: Box<dyn Fn(usize) -> Message + 'a>,
    on_block_context_menu: Option<Box<dyn Fn(usize, Point) -> Message + 'a>>,
    on_context_menu: Option<Box<dyn Fn(usize, usize, Point) -> Message + 'a>>,
    vault_root: Option<&'a str>,
    active_path: Option<&'a str>,
    existing_files: Option<HashSet<String>>,
}

#[derive(Hash, PartialEq, Eq, Clone, Copy, Debug)]
pub struct CharCacheKey {
    pub ch: char,
    pub font: iced::Font,
    pub size_bits: u32,
}

#[derive(Default)]
pub struct State {
    is_dragging: bool,
    is_focused: bool,
    modifiers: keyboard::Modifiers,
    selection_anchor: Option<(usize, usize)>,
    selection_focus: Option<(usize, usize)>,
    block_scroll_x: HashMap<usize, f32>,
    horizontal_scroll_drag: Option<HorizontalScrollDrag>,
    desired_visual_x: Option<f32>,
    layout_tree: crate::editor::layout_tree::HeightTree,
    line_height_cache: Vec<LineHeightCache>,
    last_layout_width: f32,
    block_ranges: HashMap<usize, (usize, usize)>,
}

#[derive(Debug, Clone, Copy)]
struct HorizontalScrollDrag {
    block_id: usize,
    viewport_x: f32,
    viewport_w: f32,
    content_w: f32,
    grab_offset: f32,
}

impl<'a, Message> Editor<'a, Message> {
    pub fn new(
        buffer: &'a DocBuffer,
        lines: &'a [StyledLine],
        image_cache: &'a HashMap<String, (iced::widget::image::Handle, f32, f32)>,
        math_cache: &'a HashMap<String, (iced::widget::image::Handle, f32, f32)>,
        on_command: impl Fn(EditorCommand) -> Message + 'a,
        on_pointer_command: impl Fn(EditorCommand) -> Message + 'a,
        on_link_click: impl Fn(String) -> Message + 'a,
        on_checkbox_toggle: impl Fn(usize) -> Message + 'a,
    ) -> Self {
        Self {
            buffer,
            lines,
            image_cache,
            math_cache,
            search_query: "",
            search_regex: false,
            search_match_case: false,
            active_search_match: None,
            modifiers: keyboard::Modifiers::default(),
            on_command: Box::new(on_command),
            on_pointer_command: Box::new(on_pointer_command),
            on_link_click: Box::new(on_link_click),
            on_checkbox_toggle: Box::new(on_checkbox_toggle),
            on_block_context_menu: None,
            on_context_menu: None,
            vault_root: None,
            active_path: None,
            existing_files: None,
        }
    }

    pub fn on_block_context_menu(
        mut self,
        on_block_context_menu: impl Fn(usize, Point) -> Message + 'a,
    ) -> Self {
        self.on_block_context_menu = Some(Box::new(on_block_context_menu));
        self
    }

    pub fn on_context_menu(
        mut self,
        on_context_menu: impl Fn(usize, usize, Point) -> Message + 'a,
    ) -> Self {
        self.on_context_menu = Some(Box::new(on_context_menu));
        self
    }

    pub fn vault_context(
        mut self,
        vault_root: Option<&'a str>,
        active_path: Option<&'a str>,
        existing_files: &HashSet<String>,
    ) -> Self {
        self.vault_root = vault_root;
        self.active_path = active_path;
        self.existing_files = Some(existing_files.clone());
        self
    }

    pub fn search(
        mut self,
        query: &'a str,
        regex: bool,
        match_case: bool,
        active_match: Option<(usize, usize)>,
    ) -> Self {
        self.search_query = query;
        self.search_regex = regex;
        self.search_match_case = match_case;
        self.active_search_match = active_match;
        self
    }

    pub fn modifiers(mut self, modifiers: keyboard::Modifiers) -> Self {
        self.modifiers = modifiers;
        self
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

fn line_height_for<R>(
    line: &StyledLine,
    image_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    math_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    available_width: f32,
    is_editing: bool,
    active_col: Option<usize>,
    seen_math_blocks: &mut std::collections::HashSet<usize>,
) -> f32
where
    R: iced::advanced::text::Renderer<Font = iced::Font>,
{
    if let Some(span) = line.spans.iter().find(|s| s.is_image) {
        if let Some(path) = &span.image_path {
            if let Some((_, w, h)) = image_cache.get(path) {
                let max_w = available_width - TEXT_X_OFFSET - MARGIN_RIGHT;
                let scale = if *w > max_w { max_w / w } else { 1.0 };
                return (h * scale) + 40.0; // Extra padding for caption
            }
        }
        return IMAGE_HEIGHT;
    }
    if line.is_math_block {
        if is_editing {
            return BASE_LINE_HEIGHT;
        } else {
            let has_visible_math = line
                .spans
                .iter()
                .any(|span| !span.visible_text(false).trim_matches('$').trim().is_empty());
            if !has_visible_math {
                return 0.0;
            }

            if seen_math_blocks.insert(line.block_id) {
                let mut max_h: f32 = 72.0;
                for span in &line.spans {
                    let tex = span.visible_text(false).trim_matches('$').trim();
                    if let Some((_, _, h)) = math_cache.get(tex) {
                        max_h = max_h.max(*h * 1.2 + 48.0);
                    } else if !tex.is_empty() {
                        let visual_lines = tex
                            .lines()
                            .map(|line| (line.chars().count() as f32 / 72.0).ceil().max(1.0))
                            .sum::<f32>()
                            .max(1.0);
                        max_h = max_h.max(visual_lines * BASE_LINE_HEIGHT + 48.0);
                    }
                }
                return max_h;
            } else {
                return 0.0;
            }
        }
    }
    if line.is_code_block {
        return 34.0;
    }
    if line.is_table_row {
        if is_editing {
            return measured_inline_height::<R>(
                line,
                math_cache,
                available_width,
                is_editing,
                active_col,
            );
        } else {
            if line.table_cells.is_empty() {
                return 0.0;
            }
            return 34.0;
        }
    }
    if !line.is_math_block && line.spans.iter().any(|s| s.is_math) {
        return measured_inline_height::<R>(
            line,
            math_cache,
            available_width,
            is_editing,
            active_col,
        ) + 10.0;
    }
    measured_inline_height::<R>(line, math_cache, available_width, is_editing, active_col)
}

fn block_number_from_start(
    lines: &[crate::editor::highlight::StyledLine],
    start: usize,
    prefix: &str,
) -> Option<usize> {
    let first_line = lines.get(start)?;
    let first_span = first_line.spans.first()?;
    let num_str = first_span.id.as_ref()?.strip_prefix(prefix)?;
    num_str.parse::<usize>().ok()
}

fn bounded_block_scan_range(
    start: usize,
    end: usize,
    preferred_start: usize,
    preferred_end: usize,
) -> Option<(usize, usize)> {
    if start > end {
        return None;
    }
    let preferred_start = preferred_start.clamp(start, end);
    let preferred_end = preferred_end.clamp(start, end).max(preferred_start);
    let preferred_len = preferred_end
        .saturating_sub(preferred_start)
        .saturating_add(1);

    if preferred_len >= HOT_PATH_BLOCK_SCAN_LIMIT {
        let scan_end = preferred_start
            .saturating_add(HOT_PATH_BLOCK_SCAN_LIMIT - 1)
            .min(end);
        return Some((preferred_start, scan_end));
    }

    let remaining = HOT_PATH_BLOCK_SCAN_LIMIT - preferred_len;
    let before = remaining / 2;
    let after = remaining - before;
    let scan_start = preferred_start.saturating_sub(before).max(start);
    let scan_end = preferred_end.saturating_add(after).min(end);
    Some((scan_start, scan_end))
}

fn get_equation_number(
    lines: &[crate::editor::highlight::StyledLine],
    block_id: usize,
) -> Option<usize> {
    if let Some(first_line) = lines.iter().find(|l| l.block_id == block_id) {
        if let Some(first_span) = first_line.spans.first() {
            if let Some(ref id) = first_span.id {
                if let Some(num_str) = id.strip_prefix("equation-") {
                    if let Ok(num) = num_str.parse::<usize>() {
                        return Some(num);
                    }
                }
            }
        }
    }
    None
}

fn get_image_number(span: &crate::editor::highlight::StyledSpan) -> Option<usize> {
    if let Some(ref id) = span.id {
        if let Some(num_str) = id.strip_prefix("figure-") {
            if let Ok(num) = num_str.parse::<usize>() {
                return Some(num);
            }
        }
    }
    None
}

/// Pick the iced font for a span.
fn span_font(span: &crate::editor::highlight::StyledSpan, line: &StyledLine) -> iced::Font {
    if span.is_code || line.is_code_block || line.is_math_block {
        iced::Font::MONOSPACE
    } else if span.bold {
        iced::Font {
            weight: iced::font::Weight::Bold,
            ..iced::Font::DEFAULT
        }
    } else if span.italic {
        iced::Font {
            style: iced::font::Style::Italic,
            ..iced::Font::DEFAULT
        }
    } else {
        iced::Font::DEFAULT
    }
}

/// Measure the width of a string at a given font size + font.
fn measure_width<R>(content: &str, size: f32, font: iced::Font) -> f32
where
    R: iced::advanced::text::Renderer<Font = iced::Font>,
{
    use iced::advanced::text::Paragraph;
    if content.is_empty() {
        return 0.0;
    }
    let paragraph = R::Paragraph::with_text(iced::advanced::text::Text {
        content,
        bounds: Size::new(f32::INFINITY, f32::INFINITY),
        size: size.into(),
        line_height: iced::advanced::text::LineHeight::default(),
        font,
        align_x: iced::alignment::Horizontal::Left.into(),
        align_y: iced::alignment::Vertical::Top.into(),
        shaping: iced::advanced::text::Shaping::Basic,
        wrapping: iced::advanced::text::Wrapping::None,
    });
    paragraph.min_bounds().width
}

fn measure_char_width<R>(ch: char, size: f32, font: iced::Font) -> f32
where
    R: iced::advanced::text::Renderer<Font = iced::Font>,
{
    static CACHE: OnceLock<Mutex<HashMap<CharCacheKey, f32>>> = OnceLock::new();
    let key = CharCacheKey {
        ch,
        font,
        size_bits: size.to_bits(),
    };
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(cache) = cache.lock() {
        if let Some(width) = cache.get(&key) {
            return *width;
        }
    }

    let width = measure_width::<R>(&ch.to_string(), size, font);
    if let Ok(mut cache) = cache.lock() {
        cache.insert(key, width);
    }
    width
}

fn measured_inline_height<R>(
    line: &StyledLine,
    math_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    available_width: f32,
    is_editing: bool,
    active_col: Option<usize>,
) -> f32
where
    R: iced::advanced::text::Renderer<Font = iced::Font>,
{
    let line_start_x = 0.0_f32;
    let line_right_x = (available_width - TEXT_X_OFFSET - MARGIN_RIGHT).max(80.0);
    let mut x = line_start_x;
    let mut y = 0.0_f32;
    let mut row_step = BASE_LINE_HEIGHT;

    for (span_idx, span) in line.spans.iter().enumerate() {
        let fs = span.font_size;
        let step = visual_line_step(fs);
        row_step = row_step.max(step);
        let span_editing = is_editing
            || active_col.is_some_and(|col| span_is_inline_edit_target(line, span_idx, col));

        if span.is_checkbox && !span_editing {
            let width = 26.0;
            if x > line_start_x && x + width > line_right_x {
                y += row_step;
                x = line_start_x;
                row_step = step;
            }
            x += width;
            continue;
        }

        if span.is_math && !span_editing {
            let tex = span.visible_text(false).trim_matches('$').trim();
            if tex.is_empty() || span.is_syntax {
                continue;
            }
            let (width, height) = math_cache
                .get(tex)
                .map(|(_, w, h)| (*w, *h))
                .unwrap_or_else(|| {
                    (
                        measure_width::<R>(tex, fs, span_font(span, line)),
                        BASE_LINE_HEIGHT,
                    )
                });
            let extra_h = (height - BASE_LINE_HEIGHT).max(0.0);
            row_step = row_step.max(BASE_LINE_HEIGHT + extra_h);
            if x > line_start_x && x + width > line_right_x {
                y += row_step;
                x = line_start_x;
                row_step = step;
            }
            x += width + 4.0;
            continue;
        }

        let display = span_visible_text(line, span_idx, is_editing, active_col);
        if display.is_empty() {
            continue;
        }

        let font = span_font(span, line);
        let mut token = String::new();
        let flush_token = |token: &mut String, x: &mut f32, y: &mut f32, row_step: &mut f32| {
            if token.is_empty() {
                return;
            }

            let width = measure_width::<R>(token, fs, font);
            if *x > line_start_x && *x + width > line_right_x {
                *y += *row_step;
                *x = line_start_x;
                *row_step = step;
            }

            if width <= (line_right_x - line_start_x).max(1.0) {
                *x += width;
            } else {
                for ch in token.chars() {
                    let ch_w = measure_char_width::<R>(ch, fs, font);
                    if *x > line_start_x && *x + ch_w > line_right_x {
                        *y += *row_step;
                        *x = line_start_x;
                        *row_step = step;
                    }
                    *x += ch_w;
                }
            }
            *row_step = (*row_step).max(step);
            token.clear();
        };

        for ch in display.chars() {
            token.push(ch);
            if ch.is_whitespace() {
                flush_token(&mut token, &mut x, &mut y, &mut row_step);
            }
        }
        flush_token(&mut token, &mut x, &mut y, &mut row_step);
    }

    (y + row_step).max(BASE_LINE_HEIGHT)
}

fn is_heading_line(line: &crate::editor::highlight::StyledLine) -> bool {
    line.spans
        .iter()
        .any(|span| span.is_syntax && span.visible_text(true).starts_with('#'))
}

fn is_checkbox_line(line: &crate::editor::highlight::StyledLine) -> bool {
    line.spans.iter().any(|span| span.is_checkbox)
}

fn is_pdf_citation_line(line: &crate::editor::highlight::StyledLine) -> bool {
    line.spans
        .iter()
        .any(|span| span.is_link && span.visible_text(true).starts_with("pdf://"))
}

pub fn get_block_context_menu_items(
    lines: &[crate::editor::highlight::StyledLine],
    line_idx: usize,
) -> Option<Vec<crate::views::modals::EditorBlockContextMenuItem>> {
    use crate::views::modals::EditorBlockContextMenuItem;
    let line = lines.get(line_idx)?;
    if line.is_code_block {
        Some(vec![
            EditorBlockContextMenuItem::CopyCode,
            EditorBlockContextMenuItem::SetCodeLanguage,
        ])
    } else if line.is_table_row {
        Some(vec![
            EditorBlockContextMenuItem::InsertRowAbove,
            EditorBlockContextMenuItem::InsertRowBelow,
            EditorBlockContextMenuItem::DeleteRow,
            EditorBlockContextMenuItem::InsertColumnLeft,
            EditorBlockContextMenuItem::InsertColumnRight,
            EditorBlockContextMenuItem::DeleteColumn,
        ])
    } else if line.is_blockquote {
        Some(vec![EditorBlockContextMenuItem::ConvertQuoteToParagraph])
    } else if is_heading_line(line) {
        Some(vec![
            EditorBlockContextMenuItem::ConvertToH1,
            EditorBlockContextMenuItem::ConvertToH2,
            EditorBlockContextMenuItem::ConvertToH3,
            EditorBlockContextMenuItem::ConvertToParagraph,
        ])
    } else if is_checkbox_line(line) {
        Some(vec![
            EditorBlockContextMenuItem::ToggleCheckbox,
            EditorBlockContextMenuItem::RemoveCheckbox,
        ])
    } else if is_pdf_citation_line(line) {
        Some(vec![EditorBlockContextMenuItem::OpenPdfCitation])
    } else {
        None
    }
}

fn visual_line_step(font_size: f32) -> f32 {
    (font_size * 1.45).max(BASE_LINE_HEIGHT)
}

fn source_col_after_span(span: &crate::editor::highlight::StyledSpan, start_col: usize) -> usize {
    start_col + span.text.chars().count()
}

fn span_source_range(line: &StyledLine, span_idx: usize) -> Option<(usize, usize)> {
    let mut start = 0usize;
    for (idx, span) in line.spans.iter().enumerate() {
        let end = source_col_after_span(span, start);
        if idx == span_idx {
            return Some((start, end));
        }
        start = end;
    }
    None
}

fn col_touches_range(col: usize, start: usize, end: usize) -> bool {
    col >= start && col <= end
}

fn span_is_inline_edit_target(line: &StyledLine, span_idx: usize, active_col: usize) -> bool {
    let Some(span) = line.spans.get(span_idx) else {
        return false;
    };

    let touches = |idx: usize| -> bool {
        span_source_range(line, idx)
            .is_some_and(|(start, end)| col_touches_range(active_col, start, end))
    };

    if touches(span_idx) {
        return true;
    }

    let is_content = |idx: usize| {
        line.spans.get(idx).is_some_and(|s| {
            !s.is_syntax
                && (s.bold
                    || s.italic
                    || s.is_code
                    || s.is_link
                    || s.is_math
                    || s.is_heading
                    || line.is_blockquote)
        })
    };

    let is_syntax = |idx: usize| line.spans.get(idx).is_some_and(|s| s.is_syntax);

    if span.is_syntax {
        let check_side = |content_idx: usize| -> bool {
            if is_content(content_idx) {
                if touches(content_idx) {
                    return true;
                }
                let other_syntax_idx = if content_idx > span_idx {
                    content_idx + 1
                } else {
                    content_idx.saturating_sub(1)
                };
                if is_syntax(other_syntax_idx) && touches(other_syntax_idx) {
                    return true;
                }
            }
            false
        };

        if span_idx > 0 && check_side(span_idx - 1) {
            return true;
        }
        if check_side(span_idx + 1) {
            return true;
        }
    } else if is_content(span_idx) {
        if span_idx > 0 && is_syntax(span_idx - 1) && touches(span_idx - 1) {
            return true;
        }
        if is_syntax(span_idx + 1) && touches(span_idx + 1) {
            return true;
        }
    }

    false
}

fn span_visible_text<'a>(
    line: &'a StyledLine,
    span_idx: usize,
    block_editing: bool,
    active_col: Option<usize>,
) -> &'a str {
    let Some(span) = line.spans.get(span_idx) else {
        return "";
    };
    let span_editing = block_editing
        || active_col.is_some_and(|col| span_is_inline_edit_target(line, span_idx, col));
    span.visible_text(span_editing)
}

fn is_block_editing_line(line: &StyledLine, active_block_id: Option<usize>, focused: bool) -> bool {
    focused
        && Some(line.block_id) == active_block_id
        && (line.is_code_block || line.is_math_block || line.is_table_row)
}

fn table_block_gutter_after(lines: &[StyledLine], line_idx: usize, is_editing: bool) -> f32 {
    let Some(line) = lines.get(line_idx) else {
        return 0.0;
    };
    if is_editing || !line.is_table_row {
        return 0.0;
    }
    let next_same_table_block = lines
        .get(line_idx + 1)
        .is_some_and(|next| next.is_table_row && next.block_id == line.block_id);
    if next_same_table_block {
        0.0
    } else {
        HORIZONTAL_SCROLLBAR_GUTTER
    }
}

fn is_first_block_line(lines: &[StyledLine], line_idx: usize) -> bool {
    let Some(line) = lines.get(line_idx) else {
        return false;
    };
    !lines
        .get(line_idx.saturating_sub(1))
        .is_some_and(|prev| prev.block_id == line.block_id)
}

fn draw_text_chunk<R>(
    renderer: &mut R,
    content: &str,
    x: f32,
    y: f32,
    max_width: f32,
    font_size: f32,
    font: iced::Font,
    color: Color,
    viewport: &Rectangle,
) where
    R: renderer::Renderer + iced::advanced::text::Renderer<Font = iced::Font>,
{
    renderer.fill_text(
        iced::advanced::text::Text {
            content: content.to_string(),
            bounds: Size::new(max_width.max(1.0), visual_line_step(font_size)),
            size: font_size.into(),
            line_height: iced::advanced::text::LineHeight::default(),
            font,
            align_x: iced::alignment::Horizontal::Left.into(),
            align_y: iced::alignment::Vertical::Top.into(),
            shaping: iced::advanced::text::Shaping::Basic,
            wrapping: iced::advanced::text::Wrapping::None,
        },
        Point::new(x, y + (BASE_LINE_HEIGHT - font_size) / 2.0),
        color,
        *viewport,
    );
}

fn draw_wrapped_text<R>(
    renderer: &mut R,
    text: &str,
    x: &mut f32,
    y: &mut f32,
    line_start_x: f32,
    line_right_x: f32,
    font_size: f32,
    font: iced::Font,
    color: Color,
    viewport: &Rectangle,
) where
    R: renderer::Renderer + iced::advanced::text::Renderer<Font = iced::Font>,
{
    let step = visual_line_step(font_size);
    let mut token = String::new();
    let mut flush = |token: &mut String, x: &mut f32, y: &mut f32| {
        if token.is_empty() {
            return;
        }
        let width = measure_width::<R>(token, font_size, font);
        if *x > line_start_x && *x + width > line_right_x {
            *y += step;
            *x = line_start_x;
        }
        if width <= (line_right_x - line_start_x).max(1.0) {
            draw_text_chunk(
                renderer,
                token,
                *x,
                *y,
                line_right_x - *x,
                font_size,
                font,
                color,
                viewport,
            );
            *x += width;
        } else {
            for ch in token.chars() {
                let ch_text = ch.to_string();
                let ch_w = measure_char_width::<R>(ch, font_size, font);
                if *x > line_start_x && *x + ch_w > line_right_x {
                    *y += step;
                    *x = line_start_x;
                }
                draw_text_chunk(
                    renderer,
                    &ch_text,
                    *x,
                    *y,
                    line_right_x - *x,
                    font_size,
                    font,
                    color,
                    viewport,
                );
                *x += ch_w;
            }
        }
        token.clear();
    };

    for ch in text.chars() {
        token.push(ch);
        if ch.is_whitespace() {
            flush(&mut token, x, y);
        }
    }
    flush(&mut token, x, y);
}

fn draw_nowrap_text<R>(
    renderer: &mut R,
    content: &str,
    x: f32,
    y: f32,
    max_width: f32,
    font_size: f32,
    font: iced::Font,
    color: Color,
    viewport: &Rectangle,
) where
    R: renderer::Renderer + iced::advanced::text::Renderer<Font = iced::Font>,
{
    if content.is_empty() {
        return;
    }
    renderer.fill_text(
        iced::advanced::text::Text {
            content: content.to_string(),
            bounds: Size::new(max_width.max(1.0), visual_line_step(font_size)),
            size: font_size.into(),
            line_height: iced::advanced::text::LineHeight::default(),
            font,
            align_x: iced::alignment::Horizontal::Left.into(),
            align_y: iced::alignment::Vertical::Top.into(),
            shaping: iced::advanced::text::Shaping::Basic,
            wrapping: iced::advanced::text::Wrapping::None,
        },
        Point::new(x, y),
        color,
        *viewport,
    );
}

fn clip_viewport(viewport: Rectangle, clip: Rectangle) -> Rectangle {
    let x1 = viewport.x.max(clip.x);
    let y1 = viewport.y.max(clip.y);
    let x2 = (viewport.x + viewport.width).min(clip.x + clip.width);
    let y2 = (viewport.y + viewport.height).min(clip.y + clip.height);
    Rectangle {
        x: x1,
        y: y1,
        width: (x2 - x1).max(0.0),
        height: (y2 - y1).max(0.0),
    }
}

fn draw_horizontal_scrollbar<R>(
    renderer: &mut R,
    block_id: usize,
    state: &State,
    viewport_x: f32,
    viewport_w: f32,
    y: f32,
    content_w: f32,
) where
    R: renderer::Renderer,
{
    if content_w <= viewport_w + 1.0 {
        return;
    }

    let scroll = state
        .block_scroll_x
        .get(&block_id)
        .copied()
        .unwrap_or(0.0)
        .clamp(0.0, (content_w - viewport_w).max(0.0));
    let track_w = viewport_w.max(1.0);
    let thumb_w = (track_w * (viewport_w / content_w)).clamp(32.0, track_w);
    let thumb_x = viewport_x + ((track_w - thumb_w) * (scroll / (content_w - viewport_w)));

    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle {
                x: viewport_x,
                y,
                width: track_w,
                height: 4.0,
            },
            border: iced::Border {
                radius: 2.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        theme::border_subtle(),
    );
    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle {
                x: thumb_x,
                y,
                width: thumb_w,
                height: 4.0,
            },
            border: iced::Border {
                radius: 2.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        theme::accent_dim(),
    );
}

fn normalized_selection(
    anchor: Option<(usize, usize)>,
    focus: Option<(usize, usize)>,
) -> Option<((usize, usize), (usize, usize))> {
    let (a_line, a_col) = anchor?;
    let (f_line, f_col) = focus?;
    if (a_line, a_col) == (f_line, f_col) {
        return None;
    }
    if (a_line, a_col) <= (f_line, f_col) {
        Some(((a_line, a_col), (f_line, f_col)))
    } else {
        Some(((f_line, f_col), (a_line, a_col)))
    }
}

/// Total document height in pixels.
fn total_height<R>(
    lines: &[StyledLine],
    image_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    math_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    width: f32,
    active_block_id: Option<usize>,
    active_cursor: Option<(usize, usize)>,
    focused: bool,
) -> f32
where
    R: iced::advanced::text::Renderer<Font = iced::Font>,
{
    let mut h = TOP_PAD;
    let mut seen_math_blocks = std::collections::HashSet::new();
    let mut seen_code_blocks = std::collections::HashSet::new();
    let mut seen_table_blocks = std::collections::HashSet::new();
    for (idx, line) in lines.iter().enumerate() {
        let is_editing = is_block_editing_line(line, active_block_id, focused);
        let active_col = active_cursor
            .filter(|(line_idx, _)| *line_idx == idx)
            .map(|(_, col)| col);

        let is_new_block = (line.is_code_block && seen_code_blocks.insert(line.block_id))
            || (line.is_table_row && seen_table_blocks.insert(line.block_id));

        if is_new_block && !is_editing {
            h += 24.0;
        }

        h += line_height_for::<R>(
            line,
            image_cache,
            math_cache,
            width,
            is_editing,
            active_col,
            &mut seen_math_blocks,
        );
        h += table_block_gutter_after(lines, idx, is_editing);
    }
    h + 80.0 // bottom padding
}

pub fn line_visual_y<R>(
    lines: &[StyledLine],
    image_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    math_cache: &HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    available_width: f32,
    active_line: usize,
    active_col: usize,
    target_line: usize,
    focused: bool,
) -> f32
where
    R: iced::advanced::text::Renderer<Font = iced::Font>,
{
    let active_block_id = lines.get(active_line).map(|line| line.block_id);
    let mut y = TOP_PAD;
    let mut seen_math_blocks = std::collections::HashSet::new();
    let mut seen_code_blocks = std::collections::HashSet::new();
    let mut seen_table_blocks = std::collections::HashSet::new();

    for (idx, line) in lines.iter().enumerate() {
        if idx >= target_line {
            break;
        }
        let is_editing = is_block_editing_line(line, active_block_id, focused);
        let line_active_col = (focused && idx == active_line).then_some(active_col);

        let is_new_block = (line.is_code_block && seen_code_blocks.insert(line.block_id))
            || (line.is_table_row && seen_table_blocks.insert(line.block_id));

        if is_new_block && !is_editing {
            y += 24.0;
        }

        y += line_height_for::<R>(
            line,
            image_cache,
            math_cache,
            available_width,
            is_editing,
            line_active_col,
            &mut seen_math_blocks,
        );
        y += table_block_gutter_after(lines, idx, is_editing);
    }

    y
}

// ── Widget impl ──────────────────────────────────────────────────────

impl<'a, Message, Theme, R> Widget<Message, Theme, R> for Editor<'a, Message>
where
    R: renderer::Renderer
        + iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::image::Renderer<Handle = iced::widget::image::Handle>,
{
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fixed(total_height::<R>(
                self.lines,
                self.image_cache,
                self.math_cache,
                800.0,
                None,
                None,
                false,
            )),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &R,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = _tree.state.downcast_mut::<State>();
        let focused = state.is_focused;
        let active_block_id = self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
        let max_width = limits.max().width;

        // ── Populate height tree ─────────────────────────────────────────
        let n = self.lines.len();
        if state.layout_tree.len() != n {
            state.layout_tree = crate::editor::layout_tree::HeightTree::new(n);
        }
        if state.line_height_cache.len() != n {
            state.line_height_cache = vec![LineHeightCache::default(); n];
        }
        if (state.last_layout_width - max_width).abs() > 0.5 {
            for cache in &mut state.line_height_cache {
                cache.valid = false;
            }
            state.last_layout_width = max_width;
        }

        let mut seen_math_blocks = std::collections::HashSet::new();
        let mut seen_code_blocks = std::collections::HashSet::new();
        let mut seen_table_blocks = std::collections::HashSet::new();
        state.block_ranges.clear();

        for (i, line) in self.lines.iter().enumerate() {
            let is_editing = is_block_editing_line(line, active_block_id, focused);
            let active_col =
                (focused && i == self.buffer.cursor_line).then_some(self.buffer.cursor_col);

            let is_new_block = (line.is_code_block && seen_code_blocks.insert(line.block_id))
                || (line.is_table_row && seen_table_blocks.insert(line.block_id));
            let spacer = if is_new_block && !is_editing {
                24.0
            } else {
                0.0
            };

            let gutter = table_block_gutter_after(self.lines, i, is_editing);
            let hash = line_hash(line) ^ resource_hash(line, self.image_cache, self.math_cache);
            let cached = state.line_height_cache[i];
            let line_total = if cached.valid
                && cached.hash == hash
                && cached.is_editing == is_editing
                && cached.active_col == active_col
            {
                if line.is_math_block {
                    seen_math_blocks.insert(line.block_id);
                }
                cached.height
            } else {
                let lh = line_height_for::<R>(
                    line,
                    self.image_cache,
                    self.math_cache,
                    max_width,
                    is_editing,
                    active_col,
                    &mut seen_math_blocks,
                );
                let height = spacer + lh + gutter;
                state.line_height_cache[i] = LineHeightCache {
                    hash,
                    is_editing,
                    active_col,
                    height,
                    valid: true,
                };
                height
            };

            state.layout_tree.update_height(i, line_total);

            // Track block ranges for O(1) block lookups
            if line.is_code_block || line.is_table_row || line.is_math_block || line.is_blockquote {
                state
                    .block_ranges
                    .entry(line.block_id)
                    .and_modify(|(_, end)| *end = i)
                    .or_insert((i, i));
            }
        }

        let h = TOP_PAD + state.layout_tree.prefix_sum(n) + 80.0;
        layout::Node::new(limits.resolve(Length::Fill, Length::Fixed(h), Size::new(0.0, 0.0)))
    }

    // ── draw ──────────────────────────────────────────────────────────

    fn draw(
        &self,
        _state: &widget::Tree,
        renderer: &mut R,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let state = _state.state.downcast_ref::<State>();
        let focused = state.is_focused;

        let cursor_pos = _cursor.position_in(bounds);
        let hovered_line_idx = cursor_pos.and_then(|pos| {
            let relative_y = pos.y - bounds.y - TOP_PAD;
            if relative_y >= 0.0 {
                Some(state.layout_tree.find_line_at_y(relative_y))
            } else {
                None
            }
        });

        // Background
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: iced::Border::default(),
                ..Default::default()
            },
            theme::bg_primary(),
        );

        let active_block_id = self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
        let mut image_counter = 0;
        let mut equation_counter = 0;

        let visible_start = state
            .layout_tree
            .find_line_at_y((viewport.y - bounds.y - TOP_PAD).max(0.0));
        let visible_end = state
            .layout_tree
            .find_line_at_y((viewport.y + viewport.height - bounds.y - TOP_PAD).max(0.0))
            .saturating_add(1)
            .min(self.lines.len());

        // ── Pre-calculate and draw visible block backgrounds ────────────────
        struct BlockMeta {
            y: f32,
            height: f32,
            is_code: bool,
            is_math: bool,
            is_quote: bool,
            is_table: bool,
            is_editing: bool,
            col_widths: Vec<f32>,
            content_width: f32,
            code_lang: Option<String>,
        }
        let mut blocks: std::collections::HashMap<usize, BlockMeta> =
            std::collections::HashMap::new();
        let mut visible_block_ids = std::collections::HashSet::new();
        for line in &self.lines[visible_start..visible_end] {
            if line.is_code_block || line.is_math_block || line.is_blockquote || line.is_table_row {
                visible_block_ids.insert(line.block_id);
            }
        }

        for block_id in visible_block_ids {
            let Some(&(start, end)) = state.block_ranges.get(&block_id) else {
                continue;
            };
            let Some(first_line) = self.lines.get(start) else {
                continue;
            };
            let is_editing = is_block_editing_line(first_line, active_block_id, focused);
            let block_y = bounds.y + TOP_PAD + state.layout_tree.prefix_sum(start);
            let block_height = state.layout_tree.prefix_sum(end.saturating_add(1))
                - state.layout_tree.prefix_sum(start);
            if block_height <= 0.0 {
                continue;
            }
            let mut meta = BlockMeta {
                y: block_y,
                height: block_height,
                is_code: first_line.is_code_block,
                is_math: first_line.is_math_block,
                is_quote: first_line.is_blockquote,
                is_table: first_line.is_table_row,
                is_editing,
                col_widths: Vec::new(),
                content_width: 0.0,
                code_lang: first_line.code_block_lang.clone(),
            };

            let Some((scan_start, scan_end)) =
                bounded_block_scan_range(start, end, visible_start, visible_end.saturating_sub(1))
            else {
                continue;
            };

            for line in &self.lines[scan_start..=scan_end] {
                if meta.code_lang.is_none() && line.code_block_lang.is_some() {
                    meta.code_lang = line.code_block_lang.clone();
                }
                if line.is_code_block {
                    let width = line
                        .spans
                        .iter()
                        .map(|span| {
                            measure_width::<R>(
                                span.visible_text(is_editing),
                                15.0,
                                iced::Font::MONOSPACE,
                            )
                        })
                        .sum::<f32>();
                    meta.content_width = meta.content_width.max(width + 28.0);
                } else if line.is_math_block {
                    let width = line
                        .spans
                        .iter()
                        .map(|span| {
                            let tex = span.visible_text(false).trim_matches('$').trim();
                            self.math_cache
                                .get(tex)
                                .map(|(_, w, _)| *w * 1.2 + 48.0)
                                .unwrap_or_else(|| {
                                    measure_width::<R>(tex, 16.0, iced::Font::MONOSPACE) + 48.0
                                })
                        })
                        .fold(0.0_f32, f32::max);
                    meta.content_width = meta.content_width.max(width);
                } else if line.is_table_row && !meta.is_editing {
                    for (c_idx, cell) in line.table_cells.iter().enumerate() {
                        let mut w = 0.0;
                        for span in cell {
                            let text = span.visible_text(false);
                            w += measure_width::<R>(text, span.font_size, span_font(span, line));
                        }
                        if c_idx >= meta.col_widths.len() {
                            meta.col_widths.push(w + 20.0);
                        } else if w + 20.0 > meta.col_widths[c_idx] {
                            meta.col_widths[c_idx] = w + 20.0;
                        }
                    }
                    meta.content_width = meta.col_widths.iter().sum::<f32>() + 12.0;
                }
            }
            blocks.insert(block_id, meta);
        }

        for (&block_id, meta) in &blocks {
            if meta.y + meta.height < viewport.y || meta.y > viewport.y + viewport.height {
                continue;
            }

            if meta.is_quote {
                let block_x = bounds.x + TEXT_X_OFFSET - 16.0;
                let block_w = bounds.width - TEXT_X_OFFSET;

                // Draw card background for quote
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: block_x,
                            y: meta.y,
                            width: block_w,
                            height: meta.height,
                        },
                        border: iced::Border {
                            radius: 8.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    theme::bg_secondary(),
                );

                // Draw left accent border with rounded left corners
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: block_x,
                            y: meta.y,
                            width: 4.0,
                            height: meta.height,
                        },
                        border: iced::Border {
                            radius: iced::border::Radius {
                                top_left: 8.0,
                                bottom_left: 8.0,
                                top_right: 0.0,
                                bottom_right: 0.0,
                            },
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    theme::accent(),
                );
            } else if meta.is_table && !meta.is_editing {
                let available_w = bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT;
                let table_width = available_w;
                let table_x = bounds.x + TEXT_X_OFFSET;
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: table_x - 6.0,
                            y: meta.y,
                            width: table_width + 12.0,
                            height: meta.height,
                        },
                        border: iced::Border {
                            color: theme::border(),
                            width: 1.0,
                            radius: 8.0.into(),
                        },
                        ..Default::default()
                    },
                    theme::bg_secondary(),
                );

                // Draw Table X caption
                let table_num = state
                    .block_ranges
                    .get(&block_id)
                    .and_then(|(start, _)| block_number_from_start(self.lines, *start, "table-"))
                    .unwrap_or(1);
                let caption_text = format!("Table {}", table_num);
                let text_size = 11.0;
                let caption_font = iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..iced::Font::DEFAULT
                };
                renderer.fill_text(
                    iced::advanced::text::Text {
                        content: caption_text,
                        bounds: Size::new(table_width, 24.0),
                        size: text_size.into(),
                        line_height: iced::advanced::text::LineHeight::default(),
                        font: caption_font,
                        align_x: iced::alignment::Horizontal::Center.into(),
                        align_y: iced::alignment::Vertical::Center.into(),
                        shaping: iced::advanced::text::Shaping::Basic,
                        wrapping: iced::advanced::text::Wrapping::None,
                    },
                    Point::new(table_x + table_width / 2.0, meta.y + 12.0),
                    theme::text_muted(),
                    *viewport,
                );
            } else {
                let bg = if meta.is_editing && meta.is_code {
                    theme::bg_secondary()
                } else if meta.is_editing && meta.is_math {
                    theme::bg_secondary()
                } else if meta.is_code {
                    theme::bg_secondary()
                } else {
                    Color::TRANSPARENT
                };

                if bg != Color::TRANSPARENT || meta.is_code || meta.is_math {
                    let block_x = bounds.x + TEXT_X_OFFSET - 16.0;
                    let block_w = bounds.width - TEXT_X_OFFSET;

                    // Draw container card background
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle {
                                x: block_x,
                                y: meta.y,
                                width: block_w,
                                height: meta.height,
                            },
                            border: iced::Border {
                                color: theme::border_subtle(),
                                width: 1.0,
                                radius: 8.0.into(),
                            },
                            ..Default::default()
                        },
                        if meta.is_math && !meta.is_editing {
                            theme::bg_secondary()
                        } else {
                            bg
                        },
                    );

                    // If it's a code block and not editing, draw the language badge!
                    if meta.is_code && !meta.is_editing {
                        // Draw Listing X caption
                        let code_num = state
                            .block_ranges
                            .get(&block_id)
                            .and_then(|(start, _)| {
                                block_number_from_start(self.lines, *start, "code-")
                            })
                            .unwrap_or(1);
                        let caption_text = format!("Listing {}", code_num);
                        let text_size = 11.0;
                        let caption_font = iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..iced::Font::DEFAULT
                        };
                        renderer.fill_text(
                            iced::advanced::text::Text {
                                content: caption_text,
                                bounds: Size::new(block_w, 24.0),
                                size: text_size.into(),
                                line_height: iced::advanced::text::LineHeight::default(),
                                font: caption_font,
                                align_x: iced::alignment::Horizontal::Center.into(),
                                align_y: iced::alignment::Vertical::Center.into(),
                                shaping: iced::advanced::text::Shaping::Basic,
                                wrapping: iced::advanced::text::Wrapping::None,
                            },
                            Point::new(block_x + block_w / 2.0, meta.y + 12.0),
                            theme::text_muted(),
                            *viewport,
                        );
                        if let Some(ref lang) = meta.code_lang {
                            let badge_text = lang.to_uppercase();
                            if !badge_text.is_empty() {
                                let text_size = 11.0;
                                let badge_font = iced::Font {
                                    weight: iced::font::Weight::Bold,
                                    ..iced::Font::DEFAULT
                                };
                                let text_w = measure_width::<R>(&badge_text, text_size, badge_font);
                                let badge_w = text_w + 12.0;
                                let badge_h = 18.0;
                                let badge_rect = Rectangle {
                                    x: block_x + block_w - badge_w - 12.0,
                                    y: meta.y + 8.0,
                                    width: badge_w,
                                    height: badge_h,
                                };
                                // Draw badge container
                                renderer.fill_quad(
                                    renderer::Quad {
                                        bounds: badge_rect,
                                        border: iced::Border {
                                            radius: 4.0.into(),
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    },
                                    theme::bg_tertiary(),
                                );
                                // Draw badge text centered inside the badge container
                                renderer.fill_text(
                                    iced::advanced::text::Text {
                                        content: badge_text,
                                        bounds: Size::new(badge_w, badge_h),
                                        size: text_size.into(),
                                        line_height: iced::advanced::text::LineHeight::default(),
                                        font: badge_font,
                                        align_x: iced::alignment::Horizontal::Center.into(),
                                        align_y: iced::alignment::Vertical::Center.into(),
                                        shaping: iced::advanced::text::Shaping::Basic,
                                        wrapping: iced::advanced::text::Wrapping::None,
                                    },
                                    Point::new(
                                        badge_rect.x + badge_w / 2.0,
                                        badge_rect.y + badge_h / 2.0,
                                    ),
                                    theme::accent(),
                                    *viewport,
                                );
                            }
                        }
                    }
                }
            }
        }

        let mut y = bounds.y + TOP_PAD + state.layout_tree.prefix_sum(visible_start);
        let mut last_table_block = None;
        let selection = normalized_selection(state.selection_anchor, state.selection_focus)
            .or_else(|| {
                self.buffer.selection.map(|(sl, sc, el, ec)| {
                    if (sl, sc) <= (el, ec) {
                        ((sl, sc), (el, ec))
                    } else {
                        ((el, ec), (sl, sc))
                    }
                })
            });

        for (i, line) in self.lines.iter().enumerate().skip(visible_start) {
            let line_has_selection = selection.is_some_and(|((sl, _), (el, _))| i >= sl && i <= el);
            let is_editing =
                is_block_editing_line(line, active_block_id, focused) || line_has_selection;
            let is_new_block =
                (line.is_code_block || line.is_table_row) && is_first_block_line(self.lines, i);

            if is_new_block && !is_editing {
                y += 24.0;
            }

            let active_col =
                (focused && i == self.buffer.cursor_line).then_some(self.buffer.cursor_col);

            // Viewport culling
            let gutter = table_block_gutter_after(self.lines, i, is_editing);
            let spacer = if is_new_block && !is_editing {
                24.0
            } else {
                0.0
            };
            let lh = (state.layout_tree.get_height(i) - spacer - gutter).max(0.0);
            if y + lh + gutter < viewport.y {
                y += lh + gutter;
                continue;
            }
            if y > viewport.y + viewport.height {
                break;
            }

            if line.is_math_block && !is_editing && lh == 0.0 {
                continue;
            }

            // Active line highlight
            if focused && i == self.buffer.cursor_line {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: bounds.x,
                            y,
                            width: bounds.width,
                            height: lh,
                        },
                        border: iced::Border::default(),
                        ..Default::default()
                    },
                    theme::active_line_bg(),
                );
            }

            // selection already calculated outside loop

            if let Some(((start_line, start_col), (end_line, end_col))) = selection {
                if i >= start_line && i <= end_line {
                    let line_len = self.buffer.line_text(i).chars().count();
                    let from_col = if i == start_line {
                        start_col.min(line_len)
                    } else {
                        0
                    };
                    let to_col = if i == end_line {
                        end_col.min(line_len)
                    } else {
                        line_len
                    };

                    if from_col < to_col {
                        let (from_x, from_y) = self.position_for_col::<R>(
                            i,
                            from_col,
                            bounds.width,
                            is_editing,
                            active_col,
                        );
                        let (to_x, to_y) = self.position_for_col::<R>(
                            i,
                            to_col,
                            bounds.width,
                            is_editing,
                            active_col,
                        );
                        let select_x = bounds.x + TEXT_X_OFFSET + from_x;
                        let select_w = if (to_y - from_y).abs() < 1.0 {
                            (to_x - from_x).max(3.0)
                        } else {
                            (bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT - from_x).max(3.0)
                        };
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle {
                                    x: select_x,
                                    y: y + from_y + 4.0,
                                    width: select_w,
                                    height: (BASE_LINE_HEIGHT - 8.0).max(16.0),
                                },
                                border: iced::Border {
                                    radius: 3.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                            theme::accent_dim(),
                        );
                    }
                }
            }

            if !self.search_query.is_empty() {
                let line_text = self.buffer.line_text(i);
                for line_match in search::line_matches(
                    &line_text,
                    self.search_query,
                    self.search_regex,
                    self.search_match_case,
                ) {
                    let from_col = line_match.start_col;
                    let to_col = line_match.end_col;
                    let (from_x, from_y) = self.position_for_col::<R>(
                        i,
                        from_col,
                        bounds.width,
                        is_editing,
                        active_col,
                    );
                    let (to_x, to_y) =
                        self.position_for_col::<R>(i, to_col, bounds.width, is_editing, active_col);
                    let same_visual_line = (to_y - from_y).abs() < 1.0;
                    let highlight_w = if same_visual_line {
                        (to_x - from_x).max(4.0)
                    } else {
                        (bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT - from_x).max(4.0)
                    };
                    let active = self.active_search_match == Some((i, from_col));
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle {
                                x: bounds.x + TEXT_X_OFFSET + from_x,
                                y: y + from_y + 5.0,
                                width: highlight_w,
                                height: (BASE_LINE_HEIGHT - 10.0).max(16.0),
                            },
                            border: iced::Border {
                                radius: 3.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                        if active {
                            theme::warning()
                        } else {
                            theme::accent_dim()
                        },
                    );
                }
            }

            // ── horizontal rule ──────────────────────────────────
            if line.spans.iter().any(|s| s.is_rule) {
                let rule_y = y + lh / 2.0;
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: bounds.x + TEXT_X_OFFSET,
                            y: rule_y,
                            width: bounds.width - TEXT_X_OFFSET - 20.0,
                            height: 2.0,
                        },
                        border: iced::Border {
                            radius: 1.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    theme::accent_glow(), // using a visible accent color for HR
                );
                self.draw_standard_cursor::<R>(renderer, focused, i, bounds, y, lh);
                y += lh;
                continue;
            }

            if !is_editing
                && !line.is_code_block
                && !line.is_math_block
                && !line.is_table_row
                && !line.is_blockquote
                && line.spans.iter().all(|span| {
                    !span.bold
                        && !span.italic
                        && !span.is_code
                        && !span.is_link
                        && !span.is_syntax
                        && span.display_text.is_none()
                })
                && !line
                    .spans
                    .iter()
                    .any(|s| s.is_image || s.is_math || s.is_checkbox)
            {
                let content = line
                    .spans
                    .iter()
                    .map(|span| span.visible_text(false))
                    .collect::<String>();
                if !content.trim().is_empty() {
                    let max_font = line
                        .spans
                        .iter()
                        .map(|s| s.font_size)
                        .fold(17.0_f32, f32::max);
                    let color = line
                        .spans
                        .iter()
                        .find(|span| !span.visible_text(false).is_empty())
                        .map(|span| span.color)
                        .unwrap_or(theme::text_primary());
                    let font = if line.spans.iter().any(|span| span.bold) {
                        iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..iced::Font::DEFAULT
                        }
                    } else {
                        iced::Font::DEFAULT
                    };
                    renderer.fill_text(
                        iced::advanced::text::Text {
                            content,
                            bounds: Size::new(bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT, lh),
                            size: max_font.into(),
                            line_height: iced::advanced::text::LineHeight::default(),
                            font,
                            align_x: iced::alignment::Horizontal::Left.into(),
                            align_y: iced::alignment::Vertical::Top.into(),
                            shaping: iced::advanced::text::Shaping::Basic,
                            wrapping: iced::advanced::text::Wrapping::WordOrGlyph,
                        },
                        Point::new(bounds.x + TEXT_X_OFFSET, y + 2.0),
                        color,
                        *viewport,
                    );
                }
                self.draw_standard_cursor::<R>(renderer, focused, i, bounds, y, lh);
                y += lh;
                continue;
            }

            // (Block backgrounds removed from per-line loop)

            if line.is_code_block && !line.is_math_block {
                let viewport_w = (bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT - 24.0).max(80.0);
                let content_w = blocks
                    .get(&line.block_id)
                    .map(|meta| meta.content_width)
                    .unwrap_or(viewport_w);
                let scroll_x = state
                    .block_scroll_x
                    .get(&line.block_id)
                    .copied()
                    .unwrap_or(0.0)
                    .clamp(0.0, (content_w - viewport_w).max(0.0));
                let mut code_x = bounds.x + TEXT_X_OFFSET - scroll_x;
                let code_left = bounds.x + TEXT_X_OFFSET;
                let code_right = code_left + viewport_w;

                for span in &line.spans {
                    let text = span.visible_text(is_editing);
                    if text.is_empty() {
                        continue;
                    }
                    let width = measure_width::<R>(text, 15.0, iced::Font::MONOSPACE);
                    if code_x + width >= code_left && code_x <= code_right {
                        draw_nowrap_text::<R>(
                            renderer,
                            text,
                            code_x,
                            y + 10.0,
                            width,
                            15.0,
                            iced::Font::MONOSPACE,
                            span.color,
                            viewport,
                        );
                    }
                    code_x += width;
                }

                if focused && i == self.buffer.cursor_line {
                    let (cx, _) = self.cursor_position::<R>(i, bounds.width);
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle {
                                x: bounds.x + TEXT_X_OFFSET + cx - scroll_x,
                                y: y + 12.0,
                                width: 2.0,
                                height: 22.0,
                            },
                            border: iced::Border {
                                radius: 1.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                        theme::accent_secondary(),
                    );
                }

                if let Some(meta) = blocks.get(&line.block_id) {
                    draw_horizontal_scrollbar::<R>(
                        renderer,
                        line.block_id,
                        state,
                        code_left,
                        viewport_w,
                        meta.y + meta.height - 7.0,
                        content_w,
                    );
                }

                y += lh;
                continue;
            }

            // ── table rendering ──────────────────────────────────
            if line.is_table_row && !is_editing {
                if last_table_block != Some(line.block_id) {
                    last_table_block = Some(line.block_id);
                }

                if let Some(meta) = blocks.get(&line.block_id) {
                    let available_w = bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT;
                    let raw_table_width: f32 = meta.col_widths.iter().sum();
                    let table_width = available_w;
                    let scroll_content_width = raw_table_width.max(table_width);
                    let scroll_x = state
                        .block_scroll_x
                        .get(&line.block_id)
                        .copied()
                        .unwrap_or(0.0)
                        .clamp(0.0, (scroll_content_width - table_width).max(0.0));
                    let table_x = bounds.x + TEXT_X_OFFSET;
                    let row_y = y;
                    let row_h = lh;
                    let mut cx = table_x - scroll_x;
                    let is_header = meta.y == row_y;

                    // Is this a separator row? We can check if it has spans with only `-` or `|` or just check table_cells
                    if line.table_cells.is_empty() {
                        // Separator row: draw a horizontal line
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle {
                                    x: table_x - 6.0,
                                    y: row_y,
                                    width: table_width + 12.0,
                                    height: 1.0,
                                },
                                ..Default::default()
                            },
                            theme::border(),
                        );
                        y += lh + gutter;
                        continue;
                    }

                    let is_last_row = !self.lines[i + 1..].iter().any(|next_line| {
                        next_line.is_table_row
                            && next_line.block_id == line.block_id
                            && !next_line.table_cells.is_empty()
                    });

                    let row_bg = if is_header {
                        Some(theme::bg_tertiary())
                    } else if ((row_y - meta.y) / row_h).round() as usize % 2 == 1 {
                        Some(theme::bg_secondary())
                    } else {
                        None
                    };
                    if let Some(bg) = row_bg {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle {
                                    x: table_x - 6.0,
                                    y: row_y,
                                    width: table_width + 12.0,
                                    height: row_h,
                                },
                                border: iced::Border {
                                    radius: iced::border::Radius {
                                        top_left: if is_header { 8.0 } else { 0.0 },
                                        top_right: if is_header { 8.0 } else { 0.0 },
                                        bottom_left: if is_last_row { 8.0 } else { 0.0 },
                                        bottom_right: if is_last_row { 8.0 } else { 0.0 },
                                    },
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                            bg,
                        );
                    }

                    for (c_idx, cell) in line.table_cells.iter().enumerate() {
                        if c_idx >= meta.col_widths.len() {
                            break;
                        }
                        let cw = meta.col_widths[c_idx].max(42.0);

                        // Draw Vertical Separator
                        if c_idx > 0 && cx >= table_x && cx <= table_x + table_width {
                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds: Rectangle {
                                        x: cx - 3.0,
                                        y: row_y,
                                        width: 1.0,
                                        height: row_h,
                                    },
                                    ..Default::default()
                                },
                                theme::border_subtle(),
                            );
                        }

                        // Draw Cell Spans
                        let mut px = cx + 7.0;
                        for span in cell {
                            let text = span.visible_text(false);
                            if text.is_empty() {
                                continue;
                            }

                            let font = span_font(span, line);
                            let fs = span.font_size;
                            let ty = row_y + (row_h - fs) / 2.0;
                            let width = measure_width::<R>(text, fs, font);
                            if px + width < table_x || px > table_x + table_width {
                                px += width;
                                continue;
                            }

                            renderer.fill_text(
                                iced::advanced::text::Text {
                                    content: text.to_string(),
                                    bounds: Size::new(
                                        width.min((table_x + table_width - px).max(1.0)).max(1.0),
                                        row_h,
                                    ),
                                    size: fs.into(),
                                    line_height: iced::advanced::text::LineHeight::default(),
                                    font,
                                    align_x: iced::alignment::Horizontal::Left.into(),
                                    align_y: iced::alignment::Vertical::Top.into(),
                                    shaping: iced::advanced::text::Shaping::Basic,
                                    wrapping: iced::advanced::text::Wrapping::None,
                                },
                                Point::new(px, ty),
                                if is_header {
                                    theme::text_primary()
                                } else {
                                    span.color
                                },
                                Rectangle {
                                    x: table_x,
                                    y: row_y,
                                    width: table_width,
                                    height: row_h,
                                },
                            );
                            px += width;
                        }
                        cx += cw;
                    }
                    draw_horizontal_scrollbar::<R>(
                        renderer,
                        line.block_id,
                        state,
                        table_x,
                        table_width,
                        meta.y + meta.height - HORIZONTAL_SCROLLBAR_GUTTER + 5.0,
                        scroll_content_width,
                    );
                }

                y += lh + gutter;
                continue;
            }

            // ── spans ────────────────────────────────────────────
            let mut x = bounds.x + TEXT_X_OFFSET;
            let mut line_draw_y = y;

            for (span_idx, span) in line.spans.iter().enumerate() {
                let _font = span_font(span, line);
                let is_math = span.is_math || line.is_math_block;
                let span_editing = is_editing
                    || active_col
                        .is_some_and(|col| span_is_inline_edit_target(line, span_idx, col));

                // ── image ────────────────────────────────────────
                if span.is_image && !span_editing {
                    image_counter += 1;
                    if let Some(path) = &span.image_path {
                        if let Some((handle, w, h)) = self.image_cache.get(path) {
                            let available_w = bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT;
                            let scale = if *w > available_w {
                                available_w / w
                            } else {
                                1.0
                            };
                            let draw_w = w * scale;
                            let draw_h = h * scale;
                            let draw_x = bounds.x + TEXT_X_OFFSET + (available_w - draw_w) / 2.0;

                            renderer.draw_image(
                                iced::advanced::image::Image::new(handle.clone()),
                                Rectangle {
                                    x: draw_x,
                                    y: y + 5.0,
                                    width: draw_w,
                                    height: draw_h,
                                },
                                *viewport,
                            );

                            // Draw caption
                            let fig_num = get_image_number(span).unwrap_or(image_counter);
                            let caption = format!(
                                "Figure {}: {}",
                                fig_num,
                                span.image_alt.as_deref().unwrap_or("")
                            );
                            renderer.fill_text(
                                iced::advanced::text::Text {
                                    content: caption,
                                    bounds: Size::new(draw_w, 20.0),
                                    size: 13.0.into(),
                                    line_height: iced::advanced::text::LineHeight::default(),
                                    font: iced::Font::DEFAULT,
                                    align_x: iced::alignment::Horizontal::Center.into(),
                                    align_y: iced::alignment::Vertical::Top.into(),
                                    shaping: iced::advanced::text::Shaping::Basic,
                                    wrapping: iced::advanced::text::Wrapping::WordOrGlyph,
                                },
                                Point::new(draw_x + draw_w / 2.0, y + draw_h + 12.0),
                                theme::text_muted(),
                                *viewport,
                            );

                            x += draw_w + 10.0;
                            continue;
                        }
                    }
                }

                // ── math (rendered to image) ─────────────────────
                if is_math {
                    if line.is_block_fence
                        && !span_editing
                        && span.visible_text(false).trim().is_empty()
                    {
                        continue; // Hide fences in preview
                    }
                    if span.is_syntax && !span_editing {
                        continue; // Hide inline $ in preview
                    }

                    let tex = span.visible_text(false).trim_matches('$').trim();
                    let scale: f32 = if line.is_math_block { 1.2 } else { 1.0 };
                    let mut drawn_w = 0.0;
                    let mut image_rendered = false;

                    if !tex.is_empty() {
                        if let Some((handle, w, h)) = self.math_cache.get(tex) {
                            let available_w = bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT;
                            let block_max_w = (available_w - 48.0).max(80.0);
                            let fit_scale = if line.is_math_block { scale } else { scale };
                            let draw_w = w * fit_scale;
                            let draw_h = h * fit_scale;
                            drawn_w = draw_w;

                            // While editing math, show the source text only. Drawing the rendered
                            // image behind/above the source makes the edit target unreadable.
                            if span_editing {
                                // Skip drawing image, will draw text
                            } else {
                                let line_start_x = bounds.x + TEXT_X_OFFSET;
                                let line_right_x = bounds.x + bounds.width - MARGIN_RIGHT;
                                let mut draw_x = x;
                                if line.is_math_block {
                                    equation_counter += 1;
                                    let max_scroll = (draw_w - block_max_w).max(0.0);
                                    let scroll_x = state
                                        .block_scroll_x
                                        .get(&line.block_id)
                                        .copied()
                                        .unwrap_or(0.0)
                                        .clamp(0.0, max_scroll);
                                    draw_x = bounds.x
                                        + TEXT_X_OFFSET
                                        + if draw_w <= block_max_w {
                                            (block_max_w - draw_w) / 2.0
                                        } else {
                                            -scroll_x
                                        };
                                } else if draw_x > line_start_x && draw_x + draw_w > line_right_x {
                                    line_draw_y += BASE_LINE_HEIGHT;
                                    draw_x = line_start_x;
                                    x = line_start_x;
                                }

                                if line.is_math_block {
                                    // Equation number right aligned
                                    let eq_val = get_equation_number(self.lines, line.block_id)
                                        .unwrap_or(equation_counter);
                                    let eq_num = format!("({})", eq_val);
                                    let eq_w =
                                        measure_width::<R>(&eq_num, 14.0, iced::Font::DEFAULT);
                                    let eq_y = line_draw_y + (lh - draw_h) / 2.0; // center with the equation
                                    renderer.fill_text(
                                        iced::advanced::text::Text {
                                            content: eq_num,
                                            bounds: Size::new(eq_w, draw_h),
                                            size: 14.0.into(),
                                            line_height: iced::advanced::text::LineHeight::default(
                                            ),
                                            font: iced::Font::DEFAULT,
                                            align_x: iced::alignment::Horizontal::Left.into(),
                                            align_y: iced::alignment::Vertical::Center.into(),
                                            shaping: iced::advanced::text::Shaping::Basic,
                                            wrapping: iced::advanced::text::Wrapping::None,
                                        },
                                        Point::new(
                                            bounds.x + TEXT_X_OFFSET + available_w - eq_w,
                                            eq_y + draw_h / 2.0,
                                        ),
                                        theme::text_muted(),
                                        *viewport,
                                    );
                                }

                                let math_viewport = if line.is_math_block {
                                    clip_viewport(
                                        *viewport,
                                        Rectangle {
                                            x: bounds.x + TEXT_X_OFFSET,
                                            y: line_draw_y,
                                            width: block_max_w,
                                            height: lh,
                                        },
                                    )
                                } else {
                                    *viewport
                                };

                                renderer.draw_image(
                                    iced::advanced::image::Image::new(handle.clone()),
                                    Rectangle {
                                        x: draw_x,
                                        y: if line.is_math_block {
                                            line_draw_y + (lh - draw_h) / 2.0
                                        } else {
                                            let margin_top =
                                                (BASE_LINE_HEIGHT - draw_h).max(0.0) / 2.0;
                                            line_draw_y + margin_top
                                        },
                                        width: draw_w,
                                        height: draw_h,
                                    },
                                    math_viewport,
                                );
                                if line.is_math_block {
                                    draw_horizontal_scrollbar::<R>(
                                        renderer,
                                        line.block_id,
                                        state,
                                        bounds.x + TEXT_X_OFFSET,
                                        block_max_w,
                                        y + lh - 7.0,
                                        draw_w,
                                    );
                                }
                                image_rendered = true;
                            }
                        }
                    }

                    if image_rendered
                        && (line.is_math_block || (!line.is_math_block && !span_editing))
                    {
                        x += drawn_w + 4.0;
                        continue;
                    }

                    if line.is_math_block && !is_editing && !tex.is_empty() {
                        equation_counter += 1;
                        let available_w = bounds.width - TEXT_X_OFFSET - MARGIN_RIGHT;
                        let viewport_w = (available_w - 48.0).max(80.0);
                        let content_w = tex
                            .lines()
                            .map(|raw_math_line| {
                                measure_width::<R>(raw_math_line, 16.0, iced::Font::MONOSPACE)
                            })
                            .fold(0.0_f32, f32::max);
                        let scroll_x = state
                            .block_scroll_x
                            .get(&line.block_id)
                            .copied()
                            .unwrap_or(0.0)
                            .clamp(0.0, (content_w - viewport_w).max(0.0));

                        let math_viewport = clip_viewport(
                            *viewport,
                            Rectangle {
                                x: bounds.x + TEXT_X_OFFSET,
                                y: line_draw_y,
                                width: viewport_w,
                                height: lh,
                            },
                        );

                        let mut text_y = line_draw_y + 18.0;
                        for raw_math_line in tex.lines() {
                            renderer.fill_text(
                                iced::advanced::text::Text {
                                    content: raw_math_line.to_string(),
                                    bounds: Size::new(content_w.max(1.0), BASE_LINE_HEIGHT),
                                    size: 16.0.into(),
                                    line_height: iced::advanced::text::LineHeight::default(),
                                    font: iced::Font::MONOSPACE,
                                    align_x: iced::alignment::Horizontal::Left.into(),
                                    align_y: iced::alignment::Vertical::Top.into(),
                                    shaping: iced::advanced::text::Shaping::Basic,
                                    wrapping: iced::advanced::text::Wrapping::None,
                                },
                                Point::new(bounds.x + TEXT_X_OFFSET - scroll_x, text_y),
                                theme::text_secondary(),
                                math_viewport,
                            );
                            text_y += BASE_LINE_HEIGHT;
                        }
                        draw_horizontal_scrollbar::<R>(
                            renderer,
                            line.block_id,
                            state,
                            bounds.x + TEXT_X_OFFSET,
                            viewport_w,
                            y + lh - 7.0,
                            content_w,
                        );
                        let eq_val = get_equation_number(self.lines, line.block_id)
                            .unwrap_or(equation_counter);
                        let eq_num = format!("({})", eq_val);
                        let eq_w = measure_width::<R>(&eq_num, 14.0, iced::Font::DEFAULT);
                        renderer.fill_text(
                            iced::advanced::text::Text {
                                content: eq_num,
                                bounds: Size::new(eq_w, BASE_LINE_HEIGHT),
                                size: 14.0.into(),
                                line_height: iced::advanced::text::LineHeight::default(),
                                font: iced::Font::DEFAULT,
                                align_x: iced::alignment::Horizontal::Left.into(),
                                align_y: iced::alignment::Vertical::Center.into(),
                                shaping: iced::advanced::text::Shaping::Basic,
                                wrapping: iced::advanced::text::Wrapping::None,
                            },
                            Point::new(bounds.x + TEXT_X_OFFSET + available_w - eq_w, y + lh / 2.0),
                            theme::text_muted(),
                            *viewport,
                        );
                        continue;
                    }
                }

                // ── text span ────────────────────────────────────
                let fs = span.font_size;
                let display_text = span_visible_text(line, span_idx, is_editing, active_col);
                if display_text.is_empty() {
                    continue;
                }

                let font = span_font(span, line);
                let w = if span.is_checkbox && !span_editing {
                    26.0
                } else {
                    measure_width::<R>(display_text, fs, font)
                };

                let is_hovered = cursor_pos.is_some_and(|pos| {
                    hovered_line_idx == Some(i)
                        && pos.x >= (x - bounds.x)
                        && pos.x < (x - bounds.x) + w
                });

                let is_broken = if span.is_link {
                    if let Some(ref existing) = self.existing_files {
                        if let Some(ref target) = span.link_target {
                            is_link_target_broken(
                                target,
                                self.vault_root,
                                self.active_path,
                                existing,
                            )
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };

                let text_color = if is_broken {
                    theme::danger()
                } else if span.is_link && is_hovered {
                    theme::accent()
                } else {
                    span.color
                };

                if span.is_checkbox && !span_editing {
                    // Draw a premium custom checkbox quad!
                    let box_size = 18.0;
                    let box_y = line_draw_y + (BASE_LINE_HEIGHT - box_size) / 2.0;
                    let box_rect = Rectangle {
                        x,
                        y: box_y,
                        width: box_size,
                        height: box_size,
                    };

                    if span.is_checked {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: box_rect,
                                border: iced::Border {
                                    radius: 4.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                            theme::accent(),
                        );

                        let check_font = iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..iced::Font::DEFAULT
                        };
                        let check_size = 13.0;
                        let check_w = measure_width::<R>("✓", check_size, check_font);
                        renderer.fill_text(
                            iced::advanced::text::Text {
                                content: "✓".to_string(),
                                bounds: Size::new(check_w, box_size),
                                size: check_size.into(),
                                line_height: iced::advanced::text::LineHeight::default(),
                                font: check_font,
                                align_x: iced::alignment::Horizontal::Left.into(),
                                align_y: iced::alignment::Vertical::Top.into(),
                                shaping: iced::advanced::text::Shaping::Basic,
                                wrapping: iced::advanced::text::Wrapping::None,
                            },
                            Point::new(
                                x + (box_size - check_w) / 2.0,
                                box_y + (box_size - check_size) / 2.0 - 0.5,
                            ),
                            theme::bg_primary(),
                            *viewport,
                        );
                    } else {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: box_rect,
                                border: iced::Border {
                                    color: theme::border(),
                                    width: 1.5,
                                    radius: 4.0.into(),
                                },
                                ..Default::default()
                            },
                            Color::TRANSPARENT,
                        );
                    }

                    x += box_size + 8.0;
                    continue;
                }

                let start_x = x;
                let start_y = line_draw_y;

                draw_wrapped_text::<R>(
                    renderer,
                    display_text,
                    &mut x,
                    &mut line_draw_y,
                    bounds.x + TEXT_X_OFFSET,
                    bounds.x + bounds.width - MARGIN_RIGHT,
                    fs,
                    font,
                    text_color,
                    viewport,
                );

                if span.is_link && (is_hovered || is_broken) {
                    let underline_color = if is_broken {
                        theme::danger()
                    } else {
                        theme::accent()
                    };
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle {
                                x: start_x,
                                y: start_y + fs + 2.0,
                                width: w,
                                height: 1.0,
                            },
                            ..Default::default()
                        },
                        underline_color,
                    );
                }
            }

            // ── cursor ───────────────────────────────────────────
            self.draw_standard_cursor::<R>(renderer, focused, i, bounds, y, lh);

            y += lh;
        }

        // Draw lightweight block control handle on hover
        if let Some(pos) = _cursor.position_in(bounds) {
            let relative_y = pos.y - bounds.y - TOP_PAD;
            if relative_y >= 0.0 {
                let hovered_line_idx = state.layout_tree.find_line_at_y(relative_y);
                if hovered_line_idx < self.lines.len() {
                    if let Some(line) = self.lines.get(hovered_line_idx) {
                        let block_id = line.block_id;
                        let block_start_idx = state
                            .block_ranges
                            .get(&block_id)
                            .map(|(s, _)| *s)
                            .unwrap_or(hovered_line_idx);

                        if get_block_context_menu_items(self.lines, block_start_idx).is_some() {
                            let block_y =
                                bounds.y + TOP_PAD + state.layout_tree.prefix_sum(block_start_idx);
                            let spacer = if (line.is_code_block || line.is_table_row)
                                && is_first_block_line(self.lines, block_start_idx)
                                && !is_block_editing_line(line, active_block_id, focused)
                            {
                                24.0
                            } else {
                                0.0
                            };
                            let grip_x = bounds.x + 24.0;
                            let grip_y = block_y + spacer + 4.0;
                            let grip_rect = Rectangle {
                                x: grip_x - 8.0,
                                y: grip_y,
                                width: 16.0,
                                height: 16.0,
                            };

                            let is_grip_hovered = pos.x >= 12.0
                                && pos.x <= 36.0
                                && (pos.y - (grip_y - bounds.y)).abs() <= 12.0;
                            let is_grip_hovered = is_grip_hovered
                                || grip_rect
                                    .contains(Point::new(pos.x + bounds.x, pos.y + bounds.y));

                            // Draw a subtle background for the grip button
                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds: grip_rect,
                                    border: iced::Border {
                                        radius: 4.0.into(),
                                        color: if is_grip_hovered {
                                            theme::accent()
                                        } else {
                                            theme::border_subtle()
                                        },
                                        width: if is_grip_hovered { 1.0 } else { 0.0 },
                                    },
                                    ..Default::default()
                                },
                                if is_grip_hovered {
                                    theme::bg_secondary()
                                } else {
                                    Color::TRANSPARENT
                                },
                            );

                            // Draw grip text symbol (⋮)
                            let text_size = 12.0;
                            let grip_font = iced::Font::default();
                            renderer.fill_text(
                                iced::advanced::text::Text {
                                    content: "⋮".to_string(),
                                    bounds: Size::new(16.0, 16.0),
                                    size: text_size.into(),
                                    line_height: iced::advanced::text::LineHeight::default(),
                                    font: grip_font,
                                    align_x: iced::alignment::Horizontal::Center.into(),
                                    align_y: iced::alignment::Vertical::Center.into(),
                                    shaping: iced::advanced::text::Shaping::Basic,
                                    wrapping: iced::advanced::text::Wrapping::None,
                                },
                                Point::new(grip_rect.x + 8.0, grip_rect.y + 8.0),
                                if is_grip_hovered {
                                    theme::accent()
                                } else {
                                    theme::text_muted()
                                },
                                *viewport,
                            );
                        }
                    }
                }
            }
        }
    }

    // ── update (event handling) ───────────────────────────────────────

    fn update(
        &mut self,
        _tree: &mut widget::Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &R,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = _tree.state.downcast_mut::<State>();

        match event {
            // ── mouse click ──────────────────────────────────────
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                if let Some(pos) = _cursor.position_in(_layout.bounds()) {
                    let active_block_id =
                        self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
                    let _active_col = (state.is_focused
                        && self.buffer.cursor_line < self.lines.len())
                    .then_some(self.buffer.cursor_col);
                    let (line_idx, col) = self.hit_test::<R>(
                        pos,
                        _layout.bounds().width,
                        active_block_id,
                        state.is_focused,
                        state,
                    );

                    let absolute_pos = _cursor.position().unwrap_or_default();

                    shell.publish((self.on_pointer_command)(EditorCommand::SetCursor {
                        line: line_idx,
                        col,
                    }));

                    if let Some(ref cb) = self.on_context_menu {
                        shell.publish(cb(line_idx, col, absolute_pos));
                    }
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = _cursor.position_in(_layout.bounds()) {
                    // Check for block handle click
                    let relative_y = pos.y - _layout.bounds().y - TOP_PAD;
                    if relative_y >= 0.0 {
                        let hovered_line_idx = state.layout_tree.find_line_at_y(relative_y);
                        if hovered_line_idx < self.lines.len() {
                            if let Some(line) = self.lines.get(hovered_line_idx) {
                                let block_id = line.block_id;
                                let block_start_idx = state
                                    .block_ranges
                                    .get(&block_id)
                                    .map(|(s, _)| *s)
                                    .unwrap_or(hovered_line_idx);

                                if get_block_context_menu_items(self.lines, block_start_idx)
                                    .is_some()
                                {
                                    let active_block_id =
                                        self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
                                    let block_y = _layout.bounds().y
                                        + TOP_PAD
                                        + state.layout_tree.prefix_sum(block_start_idx);
                                    let spacer = if (line.is_code_block || line.is_table_row)
                                        && is_first_block_line(self.lines, block_start_idx)
                                        && !is_block_editing_line(
                                            line,
                                            active_block_id,
                                            state.is_focused,
                                        ) {
                                        24.0
                                    } else {
                                        0.0
                                    };
                                    let grip_x = _layout.bounds().x + 24.0;
                                    let grip_y = block_y + spacer + 4.0;
                                    let grip_rect = Rectangle {
                                        x: grip_x - 8.0,
                                        y: grip_y,
                                        width: 16.0,
                                        height: 16.0,
                                    };

                                    let click_pt = Point::new(
                                        pos.x + _layout.bounds().x,
                                        pos.y + _layout.bounds().y,
                                    );
                                    if grip_rect.contains(click_pt) {
                                        let absolute_pos = _cursor.position().unwrap_or_default();
                                        if let Some(ref cb) = self.on_block_context_menu {
                                            shell.publish(cb(block_start_idx, absolute_pos));
                                        }
                                        shell.capture_event();
                                        return;
                                    }
                                }
                            }
                        }
                    }

                    if let Some(drag) =
                        self.horizontal_scrollbar_hit::<R>(pos, _layout.bounds().width, state)
                    {
                        state.horizontal_scroll_drag = Some(drag);
                        state.is_dragging = false;
                        shell.capture_event();
                        return;
                    }

                    let active_block_id =
                        self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
                    let (line_idx, col) = self.hit_test::<R>(
                        pos,
                        _layout.bounds().width,
                        active_block_id,
                        state.is_focused,
                        state,
                    );

                    // Check for checkbox / link clicks BEFORE mutating state or setting cursor
                    if let Some(line) = self.lines.get(line_idx) {
                        let is_editing =
                            is_block_editing_line(line, active_block_id, state.is_focused);
                        let mut x_acc = 0.0_f32;
                        let active_col =
                            (line_idx == self.buffer.cursor_line).then_some(self.buffer.cursor_col);
                        for (span_idx, span) in line.spans.iter().enumerate() {
                            let font = span_font(span, line);
                            let w = if span.is_checkbox && !is_editing {
                                26.0
                            } else {
                                measure_width::<R>(
                                    span_visible_text(line, span_idx, is_editing, active_col),
                                    span.font_size,
                                    font,
                                )
                            };
                            let click_x = pos.x - TEXT_X_OFFSET;
                            if click_x >= x_acc && click_x < x_acc + w {
                                if span.is_checkbox {
                                    shell.publish((self.on_checkbox_toggle)(line_idx));
                                    return;
                                }
                                if span.is_link {
                                    if let Some(target) = &span.link_target {
                                        let link_mod_active = state.modifiers.control()
                                            || state.modifiers.command()
                                            || self.modifiers.control()
                                            || self.modifiers.command();
                                        if link_mod_active {
                                            shell.publish((self.on_link_click)(target.clone()));
                                            return;
                                        }
                                    }
                                }
                            }
                            x_acc += w;
                        }
                    }

                    state.is_focused = true;
                    state.selection_anchor = Some((line_idx, col));
                    state.selection_focus = Some((line_idx, col));
                    state.desired_visual_x = None;
                    shell.publish((self.on_pointer_command)(EditorCommand::SetCursor {
                        line: line_idx,
                        col,
                    }));
                    state.is_dragging = true;
                } else {
                    state.is_focused = false;
                    state.selection_anchor = None;
                    state.selection_focus = None;
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.is_dragging => {
                if let Some(pos) = _cursor.position_in(_layout.bounds()) {
                    let active_block_id =
                        self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
                    let (line_idx, col) = self.hit_test::<R>(
                        pos,
                        _layout.bounds().width,
                        active_block_id,
                        state.is_focused,
                        state,
                    );
                    state.selection_focus = Some((line_idx, col));
                    if let Some((anchor_line, anchor_col)) = state.selection_anchor {
                        shell.publish((self.on_pointer_command)(EditorCommand::SetSelection {
                            anchor_line,
                            anchor_col,
                            focus_line: line_idx,
                            focus_col: col,
                        }));
                    }
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. })
                if state.horizontal_scroll_drag.is_some() =>
            {
                if let (Some(pos), Some(drag)) = (
                    _cursor.position_in(_layout.bounds()),
                    state.horizontal_scroll_drag,
                ) {
                    let track_w = drag.viewport_w.max(1.0);
                    let thumb_w =
                        (track_w * (drag.viewport_w / drag.content_w)).clamp(32.0, track_w);
                    let max_scroll = (drag.content_w - drag.viewport_w).max(0.0);
                    let track_range = (track_w - thumb_w).max(1.0);
                    let thumb_x =
                        (pos.x - drag.viewport_x - drag.grab_offset).clamp(0.0, track_range);
                    state
                        .block_scroll_x
                        .insert(drag.block_id, (thumb_x / track_range) * max_scroll);
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.is_dragging = false;
                state.horizontal_scroll_drag = None;
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let Some(pos) = _cursor.position_in(_layout.bounds()) else {
                    return;
                };
                let Some(block_id) = self.block_at_y::<R>(
                    pos.y,
                    _layout.bounds().width,
                    self.lines.get(self.buffer.cursor_line).map(|l| l.block_id),
                    state.is_focused,
                    state,
                ) else {
                    return;
                };
                let mut block_table = false;
                let mut block_math = false;
                if let Some((start, _)) = state.block_ranges.get(&block_id)
                    && let Some(first_line) = self.lines.get(*start)
                {
                    block_table = first_line.is_table_row;
                    block_math = first_line.is_math_block;
                }
                let available_w = _layout.bounds().width - TEXT_X_OFFSET - MARGIN_RIGHT;
                let viewport_w = if block_table {
                    available_w
                } else if block_math {
                    available_w - 48.0
                } else {
                    available_w - 24.0
                }
                .max(80.0);
                let scan_line = self.line_at_widget_y(pos.y, state).unwrap_or(0);
                let content_w = self.block_content_width::<R>(
                    block_id,
                    _layout.bounds().width,
                    state.is_focused,
                    Some((scan_line, scan_line)),
                    state,
                );
                let max_scroll = (content_w - viewport_w).max(0.0);
                if max_scroll <= 0.0 {
                    return;
                }

                let (dx, dy) = match delta {
                    mouse::ScrollDelta::Lines { x, y } => (*x * 48.0, *y * 48.0),
                    mouse::ScrollDelta::Pixels { x, y } => (*x, *y),
                };
                let horizontal_delta = if dx.abs() > 0.0 {
                    dx
                } else if state.modifiers.shift() {
                    -dy
                } else {
                    0.0
                };
                if horizontal_delta.abs() > 0.0 {
                    let entry = state.block_scroll_x.entry(block_id).or_insert(0.0);
                    *entry = (*entry + horizontal_delta).clamp(0.0, max_scroll);
                }
            }

            // ── keyboard ─────────────────────────────────────────
            Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                state.modifiers = *m;
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) if state.is_focused => {
                state.modifiers = *modifiers;

                if !matches!(
                    key.as_ref(),
                    keyboard::Key::Named(keyboard::key::Named::ArrowUp)
                        | keyboard::Key::Named(keyboard::key::Named::ArrowDown)
                ) {
                    state.desired_visual_x = None;
                }

                // Named keys first — they must never fall through to char input
                match key.as_ref() {
                    keyboard::Key::Named(keyboard::key::Named::Backspace) => {
                        shell.publish((self.on_command)(EditorCommand::DeleteBackward));
                        state.selection_anchor = None;
                        state.selection_focus = None;
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::Delete) => {
                        shell.publish((self.on_command)(EditorCommand::DeleteForward));
                        state.selection_anchor = None;
                        state.selection_focus = None;
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::Enter) => {
                        shell.publish((self.on_command)(EditorCommand::InsertText(
                            "\n".to_string(),
                        )));
                        state.selection_anchor = None;
                        state.selection_focus = None;
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
                        shell.publish((self.on_command)(EditorCommand::MoveCursor {
                            movement: Movement::Left,
                            extend: modifiers.shift(),
                        }));
                        state.selection_anchor = None;
                        state.selection_focus = None;
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
                        shell.publish((self.on_command)(EditorCommand::MoveCursor {
                            movement: Movement::Right,
                            extend: modifiers.shift(),
                        }));
                        state.selection_anchor = None;
                        state.selection_focus = None;
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                        let (new_line, new_col) =
                            self.move_visual::<R>(state, -1.0, _layout.bounds().width);
                        if modifiers.shift() {
                            let (a_l, a_c) = state
                                .selection_anchor
                                .or_else(|| self.buffer.selection.map(|(sl, sc, _, _)| (sl, sc)))
                                .unwrap_or((self.buffer.cursor_line, self.buffer.cursor_col));
                            state.selection_anchor = Some((a_l, a_c));
                            state.selection_focus = Some((new_line, new_col));
                            shell.publish((self.on_command)(EditorCommand::SetSelection {
                                anchor_line: a_l,
                                anchor_col: a_c,
                                focus_line: new_line,
                                focus_col: new_col,
                            }));
                        } else {
                            state.selection_anchor = None;
                            state.selection_focus = None;
                            shell.publish((self.on_command)(EditorCommand::SetCursor {
                                line: new_line,
                                col: new_col,
                            }));
                        }
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                        let (new_line, new_col) =
                            self.move_visual::<R>(state, 1.0, _layout.bounds().width);
                        if modifiers.shift() {
                            let (a_l, a_c) = state
                                .selection_anchor
                                .or_else(|| self.buffer.selection.map(|(sl, sc, _, _)| (sl, sc)))
                                .unwrap_or((self.buffer.cursor_line, self.buffer.cursor_col));
                            state.selection_anchor = Some((a_l, a_c));
                            state.selection_focus = Some((new_line, new_col));
                            shell.publish((self.on_command)(EditorCommand::SetSelection {
                                anchor_line: a_l,
                                anchor_col: a_c,
                                focus_line: new_line,
                                focus_col: new_col,
                            }));
                        } else {
                            state.selection_anchor = None;
                            state.selection_focus = None;
                            shell.publish((self.on_command)(EditorCommand::SetCursor {
                                line: new_line,
                                col: new_col,
                            }));
                        }
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::Home) => {
                        shell.publish((self.on_command)(EditorCommand::MoveCursor {
                            movement: Movement::Home,
                            extend: modifiers.shift(),
                        }));
                        state.selection_anchor = None;
                        state.selection_focus = None;
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::End) => {
                        shell.publish((self.on_command)(EditorCommand::MoveCursor {
                            movement: Movement::End,
                            extend: modifiers.shift(),
                        }));
                        state.selection_anchor = None;
                        state.selection_focus = None;
                        return;
                    }
                    keyboard::Key::Named(keyboard::key::Named::Tab) => {
                        shell.publish((self.on_command)(EditorCommand::InsertText(
                            "    ".to_string(),
                        )));
                        state.selection_anchor = None;
                        state.selection_focus = None;
                        return;
                    }
                    _ => {}
                }

                // Ctrl / Cmd shortcuts
                if modifiers.command() || modifiers.control() {
                    match key.as_ref() {
                        keyboard::Key::Character(c) if c == "z" => {
                            shell.publish((self.on_command)(EditorCommand::Undo));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                        keyboard::Key::Character(c) if c == "y" => {
                            shell.publish((self.on_command)(EditorCommand::Redo));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                        keyboard::Key::Character(c) if c == "a" => {
                            shell.publish((self.on_command)(EditorCommand::SelectAll));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                        keyboard::Key::Character(c) if c == "b" => {
                            shell.publish((self.on_command)(EditorCommand::FormatBold));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                        keyboard::Key::Character(c) if c == "i" => {
                            shell.publish((self.on_command)(EditorCommand::FormatItalic));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                        keyboard::Key::Character(c) if c == "e" => {
                            shell.publish((self.on_command)(EditorCommand::FormatInlineCode));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                        keyboard::Key::Character(c) if c == "k" => {
                            shell.publish((self.on_command)(EditorCommand::InsertLink));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                        keyboard::Key::Character(c) if c == "1" => {
                            shell.publish((self.on_command)(EditorCommand::ToggleHeading));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                        keyboard::Key::Character(c) if c == "q" => {
                            shell.publish((self.on_command)(EditorCommand::ToggleBlockquote));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                        keyboard::Key::Character(c) if c == "l" => {
                            shell.publish((self.on_command)(EditorCommand::ToggleUnorderedList));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                        keyboard::Key::Character(c) if c == "7" => {
                            shell.publish((self.on_command)(EditorCommand::ToggleOrderedList));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                        keyboard::Key::Character(c) if c == "d" => {
                            shell.publish((self.on_command)(EditorCommand::DuplicateLine));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                        keyboard::Key::Character(c) if c == "c" => {
                            if let Some(selected) = self
                                .buffer
                                .selected_text()
                                .or_else(|| self.selected_text(state))
                            {
                                _clipboard
                                    .write(iced::advanced::clipboard::Kind::Standard, selected);
                            }
                        }
                        keyboard::Key::Character(c) if c == "x" => {
                            if let Some(selected) = self
                                .buffer
                                .selected_text()
                                .or_else(|| self.selected_text(state))
                            {
                                _clipboard
                                    .write(iced::advanced::clipboard::Kind::Standard, selected);
                                shell.publish((self.on_command)(EditorCommand::DeleteSelection));
                                state.selection_anchor = None;
                                state.selection_focus = None;
                            }
                        }
                        keyboard::Key::Character(c) if c == "v" => {
                            if let Some(text) =
                                _clipboard.read(iced::advanced::clipboard::Kind::Standard)
                            {
                                shell.publish((self.on_command)(EditorCommand::InsertText(text)));
                                state.selection_anchor = None;
                                state.selection_focus = None;
                            }
                        }
                        _ => {}
                    }
                    return;
                }

                // Printable character input
                if let Some(t) = text {
                    if let Some(c) = t.chars().next() {
                        if !c.is_control() {
                            shell.publish((self.on_command)(EditorCommand::InsertText(
                                t.to_string(),
                            )));
                            state.selection_anchor = None;
                            state.selection_focus = None;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        _state: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &R,
    ) -> mouse::Interaction {
        let state = _state.state.downcast_ref::<State>();
        if let Some(pos) = cursor.position_in(layout.bounds()) {
            if self
                .horizontal_scrollbar_hit::<R>(pos, layout.bounds().width, state)
                .is_some()
            {
                return mouse::Interaction::Pointer;
            }

            let active_block_id = self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
            let line_idx = self.line_at_widget_y(pos.y, state);

            if let Some(line_idx) = line_idx {
                if let Some(line) = self.lines.get(line_idx) {
                    let selection =
                        normalized_selection(state.selection_anchor, state.selection_focus);
                    let line_has_selection = selection
                        .is_some_and(|((sl, _), (el, _))| line_idx >= sl && line_idx <= el);
                    let is_editing = is_block_editing_line(line, active_block_id, state.is_focused)
                        || line_has_selection;
                    let mut x_acc = 0.0_f32;
                    let active_col =
                        (line_idx == self.buffer.cursor_line).then_some(self.buffer.cursor_col);
                    for (span_idx, span) in line.spans.iter().enumerate() {
                        let font = span_font(span, line);
                        let w = if span.is_checkbox && !is_editing {
                            26.0
                        } else {
                            measure_width::<R>(
                                span_visible_text(line, span_idx, is_editing, active_col),
                                span.font_size,
                                font,
                            )
                        };
                        let click_x = pos.x - TEXT_X_OFFSET;
                        if click_x >= x_acc && click_x < x_acc + w {
                            if span.is_checkbox {
                                return mouse::Interaction::Pointer;
                            }
                            if span.is_link {
                                return mouse::Interaction::Pointer;
                            }
                        }
                        x_acc += w;
                    }
                }
            }
            return mouse::Interaction::Text;
        }
        mouse::Interaction::Idle
    }
}

pub(crate) fn is_link_target_broken(
    target: &str,
    vault_root: Option<&str>,
    active_path: Option<&str>,
    existing_files: &HashSet<String>,
) -> bool {
    let is_url = target.starts_with("http://")
        || target.starts_with("https://")
        || target.contains("://") && !target.starts_with("pdf://");
    if is_url {
        return false;
    }
    if let Some(pdf_target) = parse_pdf_link(target) {
        let resolved = resolve_relative_link_path(vault_root, active_path, &pdf_target.path);
        !existing_files.contains(&resolved)
    } else {
        let resolved = resolve_relative_link_path(vault_root, active_path, target);
        let mut exists = existing_files.contains(&resolved);
        if !exists {
            exists = existing_files.contains(&format!("{}.md", resolved))
                || existing_files.contains(&format!("{}.markdown", resolved));
        }
        !exists
    }
}

// ── Private helpers on Editor ────────────────────────────────────────

impl<'a, Message> Editor<'a, Message> {
    fn rebuild_layout_tree<R>(&self, state: &mut State, available_width: f32)
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        state.layout_tree.resize(self.lines.len());
        state.block_ranges.clear();
        let active_block_id = self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
        let mut seen_math_blocks = std::collections::HashSet::new();
        let mut seen_code_blocks = std::collections::HashSet::new();
        let mut seen_table_blocks = std::collections::HashSet::new();

        let selection = normalized_selection(state.selection_anchor, state.selection_focus);

        for (i, line) in self.lines.iter().enumerate() {
            let line_has_selection = selection.is_some_and(|((sl, _), (el, _))| i >= sl && i <= el);
            let is_editing = is_block_editing_line(line, active_block_id, state.is_focused)
                || line_has_selection;
            let active_col = (state.is_focused && i == self.buffer.cursor_line)
                .then_some(self.buffer.cursor_col);

            let is_new_block = (line.is_code_block && seen_code_blocks.insert(line.block_id))
                || (line.is_table_row && seen_table_blocks.insert(line.block_id));
            let spacer = if is_new_block && !is_editing {
                24.0
            } else {
                0.0
            };
            let lh = line_height_for::<R>(
                line,
                self.image_cache,
                self.math_cache,
                available_width,
                is_editing,
                active_col,
                &mut seen_math_blocks,
            );
            let gutter = table_block_gutter_after(self.lines, i, is_editing);
            state.layout_tree.update_height(i, spacer + lh + gutter);

            if line.is_code_block || line.is_table_row || line.is_math_block || line.is_blockquote {
                state
                    .block_ranges
                    .entry(line.block_id)
                    .and_modify(|(_, end)| *end = i)
                    .or_insert((i, i));
            }
        }
    }

    fn line_at_widget_y(&self, y: f32, state: &State) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }
        let relative_y = (y - TOP_PAD).max(0.0);
        Some(state.layout_tree.find_line_at_y(relative_y))
    }

    fn widget_y_for_line(&self, line_idx: usize, state: &State) -> f32 {
        TOP_PAD + state.layout_tree.prefix_sum(line_idx.min(self.lines.len()))
    }

    fn draw_standard_cursor<R>(
        &self,
        renderer: &mut R,
        focused: bool,
        line_idx: usize,
        bounds: Rectangle,
        y: f32,
        _line_height: f32,
    ) where
        R: renderer::Renderer + iced::advanced::text::Renderer<Font = iced::Font>,
    {
        if !focused || line_idx != self.buffer.cursor_line {
            return;
        }

        let mut font_size = 17.0;
        if let Some(styled_line) = self.lines.get(line_idx) {
            let mut char_count = 0;
            let mut found = false;
            for span in &styled_line.spans {
                let span_text = span.visible_text(true);
                let next_char_count = char_count + span_text.chars().count();
                if self.buffer.cursor_col >= char_count && self.buffer.cursor_col <= next_char_count
                {
                    font_size = span.font_size;
                    found = true;
                    break;
                }
                char_count = next_char_count;
            }
            if !found {
                if let Some(first_span) = styled_line.spans.first() {
                    font_size = first_span.font_size;
                }
            }
        }

        let cursor_h = font_size + 2.0;
        if cursor_h <= 0.0 {
            return;
        }

        let (cx, cy) = self.cursor_position::<R>(line_idx, bounds.width);
        // Center cursor within the visual line step, matching text centering
        let line_step = visual_line_step(font_size);
        let centering_offset = (line_step - cursor_h) / 2.0;

        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: bounds.x + TEXT_X_OFFSET + cx,
                    y: y + cy + centering_offset,
                    width: 2.0,
                    height: cursor_h,
                },
                border: iced::Border {
                    radius: 1.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            theme::accent(),
        );
    }

    fn position_for_col<R>(
        &self,
        line_idx: usize,
        col: usize,
        available_width: f32,
        is_editing: bool,
        active_col: Option<usize>,
    ) -> (f32, f32)
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let Some(line) = self.lines.get(line_idx) else {
            return (0.0, 0.0);
        };
        if line.is_code_block {
            let mut x = 0.0_f32;
            let mut source_col = 0usize;
            for span in &line.spans {
                let display = span.visible_text(is_editing);
                for ch in display.chars() {
                    if source_col >= col {
                        return (x, 0.0);
                    }
                    x += measure_char_width::<R>(ch, 15.0, iced::Font::MONOSPACE);
                    source_col += 1;
                }
            }
            return (x, 0.0);
        }
        let max_w = (available_width - TEXT_X_OFFSET - MARGIN_RIGHT).max(80.0);
        let mut x = 0.0_f32;
        let mut y = 0.0_f32;
        let mut source_col = 0usize;

        for (span_idx, span) in line.spans.iter().enumerate() {
            let font = span_font(span, line);
            let span_editing = is_editing
                || active_col.is_some_and(|col| span_is_inline_edit_target(line, span_idx, col));
            let display = span_visible_text(line, span_idx, is_editing, active_col);
            let span_start_col = source_col;
            let span_end_col = source_col_after_span(span, span_start_col);
            if display.is_empty() {
                if col <= span_end_col {
                    return (x, y);
                }
                source_col = span_end_col;
                continue;
            }
            let step = visual_line_step(span.font_size);
            let mut token = Vec::new();
            let flush_token =
                |token: &mut Vec<(char, usize)>, x: &mut f32, y: &mut f32| -> Option<(f32, f32)> {
                    if token.is_empty() {
                        return None;
                    }

                    let token_width = token
                        .iter()
                        .map(|(ch, _)| measure_char_width::<R>(*ch, span.font_size, font))
                        .sum::<f32>();

                    if *x > 0.0 && *x + token_width > max_w {
                        *y += step;
                        *x = 0.0;
                    }

                    if token_width <= max_w {
                        for (ch, ch_col) in token.iter() {
                            if *ch_col >= col {
                                return Some((*x, *y));
                            }
                            *x += measure_char_width::<R>(*ch, span.font_size, font);
                        }
                    } else {
                        for (ch, ch_col) in token.iter() {
                            let ch_w = measure_char_width::<R>(*ch, span.font_size, font);
                            if *x > 0.0 && *x + ch_w > max_w {
                                *y += step;
                                *x = 0.0;
                            }
                            if *ch_col >= col {
                                return Some((*x, *y));
                            }
                            *x += ch_w;
                        }
                    }

                    token.clear();
                    None
                };

            for ch in display.chars() {
                if span.is_checkbox && !is_editing {
                    if source_col >= col {
                        return (x, y);
                    }
                    x += 26.0;
                    source_col += 1;
                    continue;
                }

                if span.is_math && !span_editing {
                    let tex = span.visible_text(false).trim_matches('$').trim();
                    if !tex.is_empty() && !span.is_syntax {
                        let (width, _) = self
                            .math_cache
                            .get(tex)
                            .map(|(_, w, h)| (*w, *h))
                            .unwrap_or_else(|| {
                                (
                                    measure_width::<R>(tex, span.font_size, font),
                                    BASE_LINE_HEIGHT,
                                )
                            });

                        if x > 0.0 && x + width > max_w {
                            y += step;
                            x = 0.0;
                        }

                        if source_col >= col {
                            return (x, y);
                        }

                        x += width + 4.0;
                        // Since this is a single block, if col is within it, return x, y
                        if col <= span_end_col {
                            return (x, y);
                        }
                        break; // Move to next span
                    }
                }
                token.push((ch, source_col));
                source_col += 1;
                if ch.is_whitespace() {
                    if let Some(pos) = flush_token(&mut token, &mut x, &mut y) {
                        return pos;
                    }
                }
            }
            if let Some(pos) = flush_token(&mut token, &mut x, &mut y) {
                return pos;
            }
            if col <= span_end_col {
                return (x, y);
            }
            source_col = span_end_col;
        }
        (x, y)
    }

    fn col_for_visual_point<R>(
        &self,
        line: &StyledLine,
        click_x: f32,
        line_y: f32,
        available_width: f32,
        is_editing: bool,
        active_col: Option<usize>,
    ) -> usize
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        if click_x <= 0.0 {
            return 0;
        }

        let max_w = (available_width - TEXT_X_OFFSET - MARGIN_RIGHT).max(80.0);
        let mut x_acc = 0.0_f32;
        let mut row_y = 0.0_f32;
        let mut source_col = 0usize;
        let mut row_start_col = 0usize;
        let mut row_end_col = 0usize;
        let mut row_step = BASE_LINE_HEIGHT;

        for (span_idx, span) in line.spans.iter().enumerate() {
            let font = span_font(span, line);
            let span_editing = is_editing
                || active_col.is_some_and(|col| span_is_inline_edit_target(line, span_idx, col));
            let display = span_visible_text(line, span_idx, is_editing, active_col);
            let span_start_col = source_col;
            let span_end_col = source_col_after_span(span, span_start_col);

            if display.is_empty() {
                source_col = span_end_col;
                row_end_col = source_col;
                continue;
            }

            let step = visual_line_step(span.font_size);
            row_step = row_step.max(step);
            let mut token = Vec::new();
            let flush_token = |token: &mut Vec<(char, usize)>,
                               x_acc: &mut f32,
                               row_y: &mut f32,
                               row_start_col: &mut usize,
                               row_end_col: &mut usize,
                               row_step: &mut f32|
             -> Option<usize> {
                if token.is_empty() {
                    return None;
                }

                let token_width = token
                    .iter()
                    .map(|(ch, _)| measure_char_width::<R>(*ch, span.font_size, font))
                    .sum::<f32>();

                if *x_acc > 0.0 && *x_acc + token_width > max_w {
                    if line_y < *row_y + *row_step {
                        return Some(*row_end_col);
                    }
                    *row_y += *row_step;
                    *x_acc = 0.0;
                    *row_start_col = token.first().map(|(_, col)| *col).unwrap_or(*row_end_col);
                    *row_end_col = *row_start_col;
                    *row_step = step;
                }

                if token_width <= max_w {
                    if line_y < *row_y + *row_step {
                        for (ch, ch_col) in token.iter() {
                            let cw = measure_char_width::<R>(*ch, span.font_size, font);
                            if click_x < *x_acc + cw * 0.6 {
                                return Some(*ch_col);
                            }
                            *row_end_col = *ch_col + 1;
                            *x_acc += cw;
                        }
                    } else {
                        *x_acc += token_width;
                        if let Some((_, last_col)) = token.last() {
                            *row_end_col = *last_col + 1;
                        }
                    }
                } else {
                    for (ch, ch_col) in token.iter() {
                        let cw = measure_char_width::<R>(*ch, span.font_size, font);
                        if *x_acc > 0.0 && *x_acc + cw > max_w {
                            if line_y < *row_y + *row_step {
                                return Some(*row_end_col);
                            }
                            *row_y += *row_step;
                            *x_acc = 0.0;
                            *row_start_col = *ch_col;
                            *row_end_col = *ch_col;
                            *row_step = step;
                        }

                        if line_y < *row_y + *row_step {
                            if click_x < *x_acc + cw * 0.6 {
                                return Some(*ch_col);
                            }
                            *row_end_col = *ch_col + 1;
                        }
                        *x_acc += cw;
                    }
                }

                token.clear();
                None
            };

            for ch in display.chars() {
                if span.is_checkbox && !is_editing {
                    let cw = 26.0;
                    if x_acc > 0.0 && x_acc + cw > max_w {
                        if line_y < row_y + row_step {
                            return row_end_col;
                        }
                        row_y += row_step;
                        x_acc = 0.0;
                        row_start_col = source_col;
                        row_end_col = source_col;
                        row_step = step;
                    }

                    if line_y < row_y + row_step {
                        if click_x < x_acc + cw * 0.6 {
                            return source_col;
                        }
                        row_end_col = source_col + 1;
                    }

                    x_acc += cw;
                    source_col += 1;
                    continue;
                }

                if span.is_math && !span_editing {
                    let tex = span.visible_text(false).trim_matches('$').trim();
                    if !tex.is_empty() && !span.is_syntax {
                        let (width, height) = self
                            .math_cache
                            .get(tex)
                            .map(|(_, w, h)| (*w, *h))
                            .unwrap_or_else(|| {
                                (
                                    measure_width::<R>(tex, span.font_size, font),
                                    BASE_LINE_HEIGHT,
                                )
                            });

                        let extra_h = (height - BASE_LINE_HEIGHT).max(0.0);
                        row_step = row_step.max(BASE_LINE_HEIGHT + extra_h);

                        if x_acc > 0.0 && x_acc + width > max_w {
                            if line_y < row_y + row_step {
                                return row_end_col;
                            }
                            row_y += row_step;
                            x_acc = 0.0;
                            row_start_col = source_col;
                            row_end_col = source_col;
                            row_step = step;
                        }

                        if line_y < row_y + row_step {
                            if click_x < x_acc + width {
                                return source_col;
                            }
                            row_end_col = span_end_col;
                        }

                        x_acc += width + 4.0;
                        break; // Skip token loop entirely for this span
                    }
                }
                token.push((ch, source_col));
                source_col += 1;
                if ch.is_whitespace() {
                    if let Some(col) = flush_token(
                        &mut token,
                        &mut x_acc,
                        &mut row_y,
                        &mut row_start_col,
                        &mut row_end_col,
                        &mut row_step,
                    ) {
                        return col;
                    }
                }
            }
            if let Some(col) = flush_token(
                &mut token,
                &mut x_acc,
                &mut row_y,
                &mut row_start_col,
                &mut row_end_col,
                &mut row_step,
            ) {
                return col;
            }

            source_col = span_end_col;
            if line_y < row_y + row_step {
                row_end_col = source_col;
            }
        }

        if line_y < row_y + row_step {
            row_end_col.max(row_start_col)
        } else {
            source_col
        }
    }

    fn selected_text(&self, state: &State) -> Option<String> {
        let ((start_line, start_col), (end_line, end_col)) =
            normalized_selection(state.selection_anchor, state.selection_focus)?;

        let mut out = String::new();
        for line_idx in start_line..=end_line {
            let line = self.buffer.line_text(line_idx);
            let line_len = line.chars().count();
            let from = if line_idx == start_line {
                start_col.min(line_len)
            } else {
                0
            };
            let to = if line_idx == end_line {
                end_col.min(line_len)
            } else {
                line_len
            };
            if from < to {
                out.push_str(&line.chars().skip(from).take(to - from).collect::<String>());
            }
            if line_idx != end_line {
                out.push('\n');
            }
        }

        if out.is_empty() { None } else { Some(out) }
    }

    fn cursor_position<R>(&self, line_idx: usize, available_width: f32) -> (f32, f32)
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let Some(line) = self.lines.get(line_idx) else {
            return (0.0, 0.0);
        };
        let is_editing = is_block_editing_line(
            line,
            self.lines.get(self.buffer.cursor_line).map(|l| l.block_id),
            true,
        );
        self.position_for_col::<R>(
            line_idx,
            self.buffer.cursor_col,
            available_width,
            is_editing,
            (line_idx == self.buffer.cursor_line).then_some(self.buffer.cursor_col),
        )
    }

    fn block_at_y<R>(
        &self,
        pos_y: f32,
        _available_width: f32,
        _active_block_id: Option<usize>,
        _focused: bool,
        state: &State,
    ) -> Option<usize>
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let line_idx = self.line_at_widget_y(pos_y, state)?;
        let line = self.lines.get(line_idx)?;
        if (line.is_code_block || line.is_table_row || line.is_math_block)
            && state.block_ranges.contains_key(&line.block_id)
        {
            Some(line.block_id)
        } else {
            None
        }
    }

    fn block_content_width<R>(
        &self,
        block_id: usize,
        available_width: f32,
        focused: bool,
        scan_hint: Option<(usize, usize)>,
        state: &State,
    ) -> f32
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let active_block_id = self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
        let mut max_width = 0.0_f32;
        let mut table_widths: Vec<f32> = Vec::new();
        let Some(&(start, end)) = state.block_ranges.get(&block_id) else {
            return (available_width - TEXT_X_OFFSET - MARGIN_RIGHT).max(80.0);
        };
        let (preferred_start, preferred_end) = scan_hint.unwrap_or((start, end));
        let Some((scan_start, scan_end)) =
            bounded_block_scan_range(start, end, preferred_start, preferred_end)
        else {
            return (available_width - TEXT_X_OFFSET - MARGIN_RIGHT).max(80.0);
        };
        for line in &self.lines[scan_start..=scan_end] {
            let is_editing = is_block_editing_line(line, active_block_id, focused);
            if line.is_code_block {
                let width = line
                    .spans
                    .iter()
                    .map(|span| {
                        measure_width::<R>(
                            span.visible_text(is_editing),
                            15.0,
                            iced::Font::MONOSPACE,
                        )
                    })
                    .sum::<f32>();
                max_width = max_width.max(width + 28.0);
            } else if line.is_table_row && !is_editing {
                for (idx, cell) in line.table_cells.iter().enumerate() {
                    let width = cell
                        .iter()
                        .map(|span| {
                            measure_width::<R>(
                                span.visible_text(false),
                                span.font_size,
                                span_font(span, line),
                            )
                        })
                        .sum::<f32>()
                        + 20.0;
                    if idx >= table_widths.len() {
                        table_widths.push(width);
                    } else {
                        table_widths[idx] = table_widths[idx].max(width);
                    }
                }
            } else if line.is_math_block {
                for span in &line.spans {
                    let tex = span.visible_text(false).trim_matches('$').trim();
                    let width = self
                        .math_cache
                        .get(tex)
                        .map(|(_, w, _)| *w * 1.2 + 72.0)
                        .unwrap_or_else(|| measure_width::<R>(tex, 16.0, iced::Font::MONOSPACE));
                    max_width = max_width.max(width);
                }
            }
        }
        max_width
            .max(table_widths.iter().sum::<f32>() + 12.0)
            .max((available_width - TEXT_X_OFFSET - MARGIN_RIGHT).max(80.0))
    }

    fn horizontal_scrollbar_hit<R>(
        &self,
        pos: Point,
        available_width: f32,
        state: &State,
    ) -> Option<HorizontalScrollDrag>
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let line_idx = self.line_at_widget_y(pos.y, state)?;
        let line = self.lines.get(line_idx)?;
        if !(line.is_code_block || line.is_table_row || line.is_math_block) {
            return None;
        }
        let &(start, end) = state.block_ranges.get(&line.block_id)?;
        let block_y = TOP_PAD + state.layout_tree.prefix_sum(start);
        let block_h = state.layout_tree.prefix_sum(end.saturating_add(1))
            - state.layout_tree.prefix_sum(start);
        let available_w = available_width - TEXT_X_OFFSET - MARGIN_RIGHT;
        let viewport_w = if line.is_table_row {
            available_w
        } else if line.is_math_block {
            available_w - 48.0
        } else {
            available_w - 24.0
        }
        .max(80.0);

        self.scrollbar_hit_for_block::<R>(
            pos,
            line.block_id,
            block_y,
            block_h,
            TEXT_X_OFFSET,
            viewport_w,
            available_width,
            state.is_focused,
            Some((line_idx, line_idx)),
            state,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn scrollbar_hit_for_block<R>(
        &self,
        pos: Point,
        block_id: usize,
        block_y: f32,
        block_h: f32,
        viewport_x: f32,
        viewport_w: f32,
        available_width: f32,
        focused: bool,
        scan_hint: Option<(usize, usize)>,
        state: &State,
    ) -> Option<HorizontalScrollDrag>
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let content_w =
            self.block_content_width::<R>(block_id, available_width, focused, scan_hint, state);
        if content_w <= viewport_w + 1.0 {
            return None;
        }

        let scrollbar_y = block_y + block_h - 7.0;
        if pos.x < viewport_x
            || pos.x > viewport_x + viewport_w
            || pos.y < scrollbar_y - 8.0
            || pos.y > scrollbar_y + 10.0
        {
            return None;
        }

        let scroll = state
            .block_scroll_x
            .get(&block_id)
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, (content_w - viewport_w).max(0.0));
        let track_w = viewport_w.max(1.0);
        let thumb_w = (track_w * (viewport_w / content_w)).clamp(32.0, track_w);
        let thumb_x = viewport_x + ((track_w - thumb_w) * (scroll / (content_w - viewport_w)));
        let grab_offset = if pos.x >= thumb_x && pos.x <= thumb_x + thumb_w {
            pos.x - thumb_x
        } else {
            thumb_w / 2.0
        };

        Some(HorizontalScrollDrag {
            block_id,
            viewport_x,
            viewport_w,
            content_w,
            grab_offset,
        })
    }

    /// Convert a click position (relative to widget bounds) into (line, col).
    fn hit_test<R>(
        &self,
        pos: Point,
        available_width: f32,
        active_block_id: Option<usize>,
        focused: bool,
        state: &State,
    ) -> (usize, usize)
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let line_idx = self.line_at_widget_y(pos.y, state).unwrap_or(0);
        let y_acc = self.widget_y_for_line(line_idx, state);

        // Horizontal: walk spans character by character
        let Some(line) = self.lines.get(line_idx) else {
            return (line_idx, 0);
        };
        let click_x = pos.x - TEXT_X_OFFSET;
        if click_x <= 0.0 {
            return (line_idx, 0);
        }

        let selection = normalized_selection(state.selection_anchor, state.selection_focus);
        let line_has_selection =
            selection.is_some_and(|((sl, _), (el, _))| line_idx >= sl && line_idx <= el);
        let is_editing =
            is_block_editing_line(line, active_block_id, focused) || line_has_selection;
        let col = self.col_for_visual_point::<R>(
            line,
            click_x,
            pos.y - y_acc,
            available_width,
            is_editing,
            (focused && line_idx == self.buffer.cursor_line).then_some(self.buffer.cursor_col),
        );
        (line_idx, col)
    }

    fn move_visual<R>(
        &self,
        state: &mut State,
        delta_lines: f32,
        available_width: f32,
    ) -> (usize, usize)
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        if state.layout_tree.len() != self.lines.len() {
            self.rebuild_layout_tree::<R>(state, available_width);
        }
        let (cur_x, cur_y_in_line) =
            self.cursor_position::<R>(self.buffer.cursor_line, available_width);
        let cur_y_base = self.widget_y_for_line(self.buffer.cursor_line, state);

        let visual_x = *state.desired_visual_x.get_or_insert(cur_x);
        let line = &self.lines[self.buffer.cursor_line];
        let max_font = line
            .spans
            .iter()
            .map(|s| s.font_size)
            .fold(17.0_f32, f32::max);
        let step = visual_line_step(max_font);

        let target_y = cur_y_base + cur_y_in_line + delta_lines * step + step / 2.0;

        let mut target = self.hit_test::<R>(
            Point::new(visual_x + TEXT_X_OFFSET, target_y),
            available_width,
            self.lines.get(self.buffer.cursor_line).map(|l| l.block_id),
            state.is_focused,
            state,
        );

        let current = (self.buffer.cursor_line, self.buffer.cursor_col);
        if delta_lines > 0.0 && target <= current {
            target = self.fallback_visual_line_move::<R>(visual_x, 1, available_width);
        } else if delta_lines < 0.0 && target >= current {
            target = self.fallback_visual_line_move::<R>(visual_x, -1, available_width);
        }

        target
    }

    fn fallback_visual_line_move<R>(
        &self,
        visual_x: f32,
        delta_lines: isize,
        available_width: f32,
    ) -> (usize, usize)
    where
        R: iced::advanced::text::Renderer<Font = iced::Font>,
    {
        let current_line = self.buffer.cursor_line;
        let target_line = if delta_lines < 0 {
            current_line.saturating_sub(1)
        } else {
            (current_line + 1).min(self.lines.len().saturating_sub(1))
        };

        if target_line == current_line {
            return (self.buffer.cursor_line, self.buffer.cursor_col);
        }

        let Some(line) = self.lines.get(target_line) else {
            return (self.buffer.cursor_line, self.buffer.cursor_col);
        };
        let active_block_id = self.lines.get(self.buffer.cursor_line).map(|l| l.block_id);
        let is_editing = is_block_editing_line(line, active_block_id, true);
        let col = self.col_for_visual_point::<R>(
            line,
            visual_x,
            BASE_LINE_HEIGHT / 2.0,
            available_width,
            is_editing,
            None,
        );
        (target_line, col)
    }
}

// ── Into<Element> ────────────────────────────────────────────────────

impl<'a, Message, Theme, R> From<Editor<'a, Message>> for Element<'a, Message, Theme, R>
where
    R: renderer::Renderer
        + iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::image::Renderer<Handle = iced::widget::image::Handle>,
    Message: 'a,
{
    fn from(editor: Editor<'a, Message>) -> Self {
        Self::new(editor)
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::buffer::{DocBuffer, EditorCommand};
    use crate::editor::highlight::{StyledLine, StyledSpan, highlight_markdown};
    use std::collections::HashMap;

    fn make_line(block_id: usize, spans: Vec<StyledSpan>) -> StyledLine {
        let mut line = StyledLine::new();
        line.block_id = block_id;
        line.spans = spans;
        line
    }

    fn editor_for<'a>(
        buffer: &'a DocBuffer,
        lines: &'a [StyledLine],
        image_cache: &'a HashMap<String, (iced::widget::image::Handle, f32, f32)>,
        math_cache: &'a HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    ) -> Editor<'a, ()> {
        Editor::new(
            buffer,
            lines,
            image_cache,
            math_cache,
            |_| (),
            |_| (),
            |_| (),
            |_| (),
        )
    }

    fn test_state() -> State {
        State {
            is_dragging: false,
            is_focused: true,
            modifiers: keyboard::Modifiers::default(),
            selection_anchor: None,
            selection_focus: None,
            block_scroll_x: HashMap::new(),
            horizontal_scroll_drag: None,
            desired_visual_x: None,
            ..Default::default()
        }
    }

    #[test]
    fn test_normalized_selection_combinatorics() {
        // Run thousands of combinations of boundary cases for selections
        for anchor_line in 0..15 {
            for anchor_col in 0..10 {
                for focus_line in 0..15 {
                    for focus_col in 0..10 {
                        let norm = normalized_selection(
                            Some((anchor_line, anchor_col)),
                            Some((focus_line, focus_col)),
                        );
                        if (anchor_line, anchor_col) == (focus_line, focus_col) {
                            assert!(norm.is_none());
                        } else {
                            let (start, end) = norm.unwrap();
                            assert!(start <= end);
                            if anchor_line < focus_line {
                                assert_eq!(start, (anchor_line, anchor_col));
                                assert_eq!(end, (focus_line, focus_col));
                            } else if anchor_line > focus_line {
                                assert_eq!(start, (focus_line, focus_col));
                                assert_eq!(end, (anchor_line, anchor_col));
                            } else {
                                assert_eq!(start, (anchor_line, anchor_col.min(focus_col)));
                                assert_eq!(end, (anchor_line, anchor_col.max(focus_col)));
                            }
                        }
                    }
                }
            }
        }

        assert!(normalized_selection(None, None).is_none());
        assert!(normalized_selection(Some((1, 1)), None).is_none());
        assert!(normalized_selection(None, Some((2, 2))).is_none());
    }

    #[test]
    fn test_editor_selected_text_extraction() {
        let buffer = DocBuffer::from_text("line one\nline two\nline three\nline four");
        let lines: Vec<StyledLine> = vec![
            make_line(1, vec![StyledSpan::plain("line one")]),
            make_line(2, vec![StyledSpan::plain("line two")]),
            make_line(3, vec![StyledSpan::plain("line three")]),
            make_line(4, vec![StyledSpan::plain("line four")]),
        ];

        let image_cache = HashMap::new();
        let math_cache = HashMap::new();

        let editor = Editor::new(
            &buffer,
            &lines,
            &image_cache,
            &math_cache,
            |_| (),
            |_| (),
            |_| (),
            |_| (),
        );

        // Perform combinatorial selections over the entire document
        for start_line in 0..4 {
            for start_col in 0..10 {
                for end_line in 0..4 {
                    for end_col in 0..10 {
                        let state = State {
                            is_dragging: false,
                            is_focused: true,
                            modifiers: keyboard::Modifiers::default(),
                            selection_anchor: Some((start_line, start_col)),
                            selection_focus: Some((end_line, end_col)),
                            block_scroll_x: HashMap::new(),
                            horizontal_scroll_drag: None,
                            desired_visual_x: None,
                            ..Default::default()
                        };

                        let sel = editor.selected_text(&state);
                        if let Some(((s_l, s_c), (e_l, e_c))) = normalized_selection(
                            Some((start_line, start_col)),
                            Some((end_line, end_col)),
                        ) {
                            let mut manual = String::new();
                            for l in s_l..=e_l {
                                let content = buffer.line_text(l);
                                let from = if l == s_l {
                                    s_c.min(content.chars().count())
                                } else {
                                    0
                                };
                                let to = if l == e_l {
                                    e_c.min(content.chars().count())
                                } else {
                                    content.chars().count()
                                };
                                if from < to {
                                    manual.push_str(
                                        &content
                                            .chars()
                                            .skip(from)
                                            .take(to - from)
                                            .collect::<String>(),
                                    );
                                }
                                if l != e_l {
                                    manual.push('\n');
                                }
                            }
                            if manual.is_empty() {
                                assert!(sel.is_none());
                            } else {
                                assert_eq!(sel.unwrap(), manual);
                            }
                        } else {
                            assert!(sel.is_none());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn typora_inline_editing_reveals_only_active_span_and_markers() {
        let lines = highlight_markdown("alpha **bold** omega");
        let line = &lines[0];

        let rendered = line
            .spans
            .iter()
            .enumerate()
            .map(|(idx, _)| span_visible_text(line, idx, false, None))
            .collect::<Vec<_>>();
        assert_eq!(rendered, vec!["alpha ", "", "bold", "", " omega"]);

        let active_inside_bold = "alpha **bo".chars().count();
        let editing = line
            .spans
            .iter()
            .enumerate()
            .map(|(idx, _)| span_visible_text(line, idx, false, Some(active_inside_bold)))
            .collect::<Vec<_>>();
        assert_eq!(editing, vec!["alpha ", "**", "bold", "**", " omega"]);

        let active_inside_plain = "al".chars().count();
        let editing_plain = line
            .spans
            .iter()
            .enumerate()
            .map(|(idx, _)| span_visible_text(line, idx, false, Some(active_inside_plain)))
            .collect::<Vec<_>>();
        assert_eq!(editing_plain, vec!["alpha ", "", "bold", "", " omega"]);
    }

    #[test]
    fn table_scrollbar_gutter_is_reserved_only_after_last_table_row() {
        let mut first = make_line(1, vec![]);
        first.is_table_row = true;
        let mut second = make_line(1, vec![]);
        second.is_table_row = true;
        let plain = make_line(2, vec![StyledSpan::plain("after")]);
        let lines = vec![first, second, plain];

        assert_eq!(table_block_gutter_after(&lines, 0, false), 0.0);
        assert_eq!(
            table_block_gutter_after(&lines, 1, false),
            HORIZONTAL_SCROLLBAR_GUTTER
        );
        assert_eq!(table_block_gutter_after(&lines, 1, true), 0.0);
        assert_eq!(table_block_gutter_after(&lines, 2, false), 0.0);
    }

    #[test]
    fn inactive_plain_line_height_does_not_create_cursorless_blank_gap() {
        let line = make_line(1, vec![StyledSpan::plain("short line")]);
        let image_cache = HashMap::new();
        let math_cache = HashMap::new();
        let mut seen_math_blocks = std::collections::HashSet::new();

        let h = line_height_for::<iced::Renderer>(
            &line,
            &image_cache,
            &math_cache,
            900.0,
            false,
            None,
            &mut seen_math_blocks,
        );
        assert_eq!(h, BASE_LINE_HEIGHT);
    }

    #[test]
    fn visual_down_movement_is_monotonic_through_wrapped_markdown_lines() {
        let text = concat!(
            "alpha **bold text with enough words to wrap around the editor width** omega\n",
            "second line with `inline code` and more words to move through\n",
            "third line ends here"
        );
        let mut buffer = DocBuffer::from_text(text);
        buffer.execute(EditorCommand::SetCursor { line: 0, col: 0 });
        let image_cache = HashMap::new();
        let math_cache = HashMap::new();
        let mut previous = (buffer.cursor_line, buffer.cursor_col);

        for _ in 0..12 {
            let lines = highlight_markdown(&buffer.text());
            let editor = editor_for(&buffer, &lines, &image_cache, &math_cache);
            let mut state = test_state();
            let next = editor.move_visual::<iced::Renderer>(&mut state, 1.0, 260.0);

            if next == previous {
                assert_eq!(next.0, lines.len().saturating_sub(1));
                break;
            }
            assert!(
                next > previous,
                "visual down must move forward, previous={previous:?}, next={next:?}"
            );
            drop(editor);
            buffer.execute(EditorCommand::SetCursor {
                line: next.0,
                col: next.1,
            });
            previous = next;
        }

        assert_eq!(previous.0, 2);
    }

    #[test]
    fn visual_down_moves_through_empty_lines_without_vanishing() {
        let text = "first\n\nthird\n\nfifth";
        let mut buffer = DocBuffer::from_text(text);
        buffer.execute(EditorCommand::SetCursor { line: 0, col: 2 });
        let image_cache = HashMap::new();
        let math_cache = HashMap::new();

        let mut visited = Vec::new();
        for _ in 0..8 {
            let lines = highlight_markdown(&buffer.text());
            let editor = editor_for(&buffer, &lines, &image_cache, &math_cache);
            let mut state = test_state();
            let next = editor.move_visual::<iced::Renderer>(&mut state, 1.0, 900.0);
            visited.push(next);
            drop(editor);
            buffer.execute(EditorCommand::SetCursor {
                line: next.0,
                col: next.1,
            });
            if next.0 == lines.len().saturating_sub(1) {
                break;
            }
        }

        assert!(
            visited.iter().any(|(line, col)| *line == 1 && *col == 0),
            "down should visit first empty line, visited={visited:?}"
        );
        assert!(
            visited.iter().any(|(line, col)| *line == 3 && *col == 0),
            "down should visit second empty line, visited={visited:?}"
        );
        assert_eq!(buffer.cursor_line, 4);
    }

    #[test]
    fn trailing_empty_line_after_enter_has_visible_cursor_geometry() {
        let mut buffer = DocBuffer::from_text("first");
        buffer.execute(EditorCommand::SetCursor { line: 0, col: 5 });
        buffer.execute(EditorCommand::InsertText("\n".to_string()));

        let lines = highlight_markdown(&buffer.text());
        let image_cache = HashMap::new();
        let math_cache = HashMap::new();
        let editor = editor_for(&buffer, &lines, &image_cache, &math_cache);
        let mut seen_math_blocks = std::collections::HashSet::new();

        assert_eq!((buffer.cursor_line, buffer.cursor_col), (1, 0));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].spans.len(), 1);
        assert_eq!(lines[1].spans[0].visible_text(false), "");

        let height = line_height_for::<iced::Renderer>(
            &lines[1],
            &image_cache,
            &math_cache,
            900.0,
            false,
            Some(0),
            &mut seen_math_blocks,
        );
        let cursor = editor.cursor_position::<iced::Renderer>(1, 900.0);

        assert_eq!(height, BASE_LINE_HEIGHT);
        assert_eq!(cursor, (0.0, 0.0));
        assert!(height.min(20.0) > 0.0);
    }

    #[test]
    fn line_visual_y_includes_single_table_scrollbar_gutter() {
        let mut header = make_line(1, vec![]);
        header.is_table_row = true;
        header.table_cells = vec![vec![StyledSpan::plain("A")], vec![StyledSpan::plain("B")]];
        let mut body = make_line(1, vec![]);
        body.is_table_row = true;
        body.table_cells = vec![vec![StyledSpan::plain("1")], vec![StyledSpan::plain("2")]];
        let after = make_line(2, vec![StyledSpan::plain("after")]);
        let lines = vec![header, body, after];
        let image_cache = HashMap::new();
        let math_cache = HashMap::new();

        let y_after_table = line_visual_y::<iced::Renderer>(
            &lines,
            &image_cache,
            &math_cache,
            900.0,
            0,
            0,
            2,
            false,
        );

        assert_eq!(
            y_after_table,
            TOP_PAD + 24.0 + 34.0 + 34.0 + HORIZONTAL_SCROLLBAR_GUTTER
        );
    }

    #[test]
    fn test_renderer_line_height_permutations() {
        let mut lines = Vec::new();

        // 1. Plain text line
        lines.push(make_line(1, vec![StyledSpan::plain("Hello world")]));

        // 2. Code block line
        let mut code_line = make_line(
            2,
            vec![StyledSpan {
                text: "let x = 10;".to_string(),
                is_code: true,
                ..StyledSpan::plain("")
            }],
        );
        code_line.is_code_block = true;
        lines.push(code_line);

        // 3. Math block line (not editing)
        let mut math_line = make_line(
            3,
            vec![StyledSpan {
                text: "$$ E = mc^2 $$".to_string(),
                is_math: true,
                ..StyledSpan::plain("")
            }],
        );
        math_line.is_math_block = true;
        lines.push(math_line);

        // 4. Table row
        let mut table_line = make_line(4, vec![]);
        table_line.is_table_row = true;
        table_line.table_cells = vec![
            vec![StyledSpan::plain("Col A")],
            vec![StyledSpan::plain("Col B")],
        ];
        lines.push(table_line);

        // 5. Image line
        let img_line = make_line(
            5,
            vec![StyledSpan {
                text: "![alt](image.png)".to_string(),
                is_image: true,
                image_path: Some("image.png".to_string()),
                ..StyledSpan::plain("")
            }],
        );
        lines.push(img_line);

        // 6. Deep quote line
        let mut quote_line = make_line(6, vec![StyledSpan::plain("A quote")]);
        quote_line.is_blockquote = true;
        lines.push(quote_line);

        let mut image_cache = HashMap::new();
        let mut math_cache = HashMap::new();

        image_cache.insert(
            "image.png".to_string(),
            (
                iced::widget::image::Handle::from_rgba(10, 10, vec![0; 400]),
                400.0,
                300.0,
            ),
        );
        math_cache.insert(
            "E = mc^2".to_string(),
            (
                iced::widget::image::Handle::from_rgba(10, 10, vec![0; 400]),
                200.0,
                50.0,
            ),
        );

        let widths = vec![100.0, 200.0, 400.0, 600.0, 800.0, 1000.0, 1200.0];
        let mut seen_math_blocks = std::collections::HashSet::new();

        for &width in &widths {
            for &is_editing in &[true, false] {
                for line in &lines {
                    seen_math_blocks.clear();
                    let h = line_height_for::<iced::Renderer>(
                        line,
                        &image_cache,
                        &math_cache,
                        width,
                        is_editing,
                        None,
                        &mut seen_math_blocks,
                    );

                    assert!(h >= 0.0);

                    if line.is_table_row {
                        if is_editing {
                            assert!(h >= BASE_LINE_HEIGHT);
                        } else {
                            assert_eq!(h, 34.0);
                        }
                    } else if line.is_math_block && is_editing {
                        assert_eq!(h, BASE_LINE_HEIGHT);
                    } else if line.is_blockquote {
                        assert!(h > 0.0);
                    }
                }
            }
        }
    }

    #[test]
    fn test_renderer_total_height_accumulation() {
        let mut lines = Vec::new();
        for i in 1..=200 {
            lines.push(make_line(
                i,
                vec![StyledSpan::plain("Hello accumulated document")],
            ));
        }

        let image_cache = HashMap::new();
        let math_cache = HashMap::new();

        // 1. Verify adding lines monotonically increases total height
        let h1 = total_height::<iced::Renderer>(
            &lines[0..50],
            &image_cache,
            &math_cache,
            800.0,
            None,
            None,
            false,
        );
        let h2 = total_height::<iced::Renderer>(
            &lines[0..100],
            &image_cache,
            &math_cache,
            800.0,
            None,
            None,
            false,
        );
        let h3 = total_height::<iced::Renderer>(
            &lines[0..200],
            &image_cache,
            &math_cache,
            800.0,
            None,
            None,
            false,
        );

        assert!(h2 > h1);
        assert!(h3 > h2);

        // 2. Verify width decreases wrapping space and monotonically increases total height
        let h_wide = total_height::<iced::Renderer>(
            &lines,
            &image_cache,
            &math_cache,
            1000.0,
            None,
            None,
            false,
        );
        let h_narrow = total_height::<iced::Renderer>(
            &lines,
            &image_cache,
            &math_cache,
            200.0,
            None,
            None,
            false,
        );

        assert!(h_narrow >= h_wide);
    }

    #[test]
    fn test_bug_finder_renderer_extreme_dimensions() {
        let line = make_line(
            1,
            vec![StyledSpan::plain(
                "Wrap this extremely long sentence with extreme layout boundary dimensions to find bugs.",
            )],
        );
        let image_cache = HashMap::new();
        let math_cache = HashMap::new();
        let mut seen_math_blocks = std::collections::HashSet::new();

        // Extreme layout widths (0, negative, infinite, sub-pixel)
        let extreme_widths = vec![
            0.0,
            -100.0,
            -0.0001,
            0.0001,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ];
        for &width in &extreme_widths {
            seen_math_blocks.clear();
            let h = line_height_for::<iced::Renderer>(
                &line,
                &image_cache,
                &math_cache,
                width,
                false,
                None,
                &mut seen_math_blocks,
            );
            assert!(h >= 0.0);
        }
    }

    #[test]
    fn test_clip_viewport() {
        let viewport = Rectangle {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 200.0,
        };
        // Fully inside
        let clip_inside = Rectangle {
            x: 20.0,
            y: 30.0,
            width: 50.0,
            height: 50.0,
        };
        let res = clip_viewport(viewport, clip_inside);
        assert_eq!(res.x, 20.0);
        assert_eq!(res.y, 30.0);
        assert_eq!(res.width, 50.0);
        assert_eq!(res.height, 50.0);

        // No overlap
        let clip_no_overlap = Rectangle {
            x: 200.0,
            y: 300.0,
            width: 50.0,
            height: 50.0,
        };
        let res_none = clip_viewport(viewport, clip_no_overlap);
        assert_eq!(res_none.width, 0.0);
        assert_eq!(res_none.height, 0.0);

        // Partial overlap
        let clip_partial = Rectangle {
            x: 50.0,
            y: 100.0,
            width: 100.0,
            height: 200.0,
        };
        let res_partial = clip_viewport(viewport, clip_partial);
        assert_eq!(res_partial.x, 50.0);
        assert_eq!(res_partial.y, 100.0);
        assert_eq!(res_partial.width, 60.0);
        assert_eq!(res_partial.height, 120.0);
    }

    #[test]
    fn bounded_block_scan_range_caps_large_blocks() {
        let (start, end) = bounded_block_scan_range(0, 49_999, 25_000, 25_000).expect("range");

        assert!(start <= 25_000);
        assert!(end >= 25_000);
        assert!(end.saturating_sub(start).saturating_add(1) <= HOT_PATH_BLOCK_SCAN_LIMIT);
    }

    #[test]
    fn block_content_width_uses_bounded_scan_hint() {
        let mut lines = Vec::new();
        for idx in 0..1_000 {
            let mut line = make_line(
                7,
                vec![StyledSpan::plain(if idx == 500 {
                    "wide code line far inside large block"
                } else {
                    "x"
                })],
            );
            line.is_code_block = true;
            lines.push(line);
        }
        let mut buffer = DocBuffer::from_text(&vec!["x"; 1_000].join("\n"));
        buffer.execute(EditorCommand::SetCursor { line: 500, col: 0 });
        let image_cache = HashMap::new();
        let math_cache = HashMap::new();
        let editor = editor_for(&buffer, &lines, &image_cache, &math_cache);
        let mut state = test_state();
        state.block_ranges.insert(7, (0, 999));

        let width_near =
            editor.block_content_width::<iced::Renderer>(7, 200.0, true, Some((500, 500)), &state);
        let width_far =
            editor.block_content_width::<iced::Renderer>(7, 200.0, true, Some((0, 0)), &state);

        assert!(width_near > width_far);
    }

    #[test]
    fn test_code_block_badge_logic() {
        use crate::editor::highlight::highlight_markdown;
        let md = "```rust\nfn main() {}\n```";
        let lines = highlight_markdown(md);

        // The first line should have the language
        assert_eq!(lines[0].is_code_block, true);
        assert_eq!(lines[0].code_block_lang.as_deref(), Some("rust"));

        // Check badge_w calculation logic
        let _badge_text = "RUST";
        let text_w = 32.0; // Simulated width
        let badge_w = text_w + 12.0;
        let badge_h = 18.0;

        let block_x = 10.0;
        let block_w = 500.0;
        let badge_rect = Rectangle {
            x: block_x + block_w - badge_w - 12.0,
            y: 100.0 + 8.0,
            width: badge_w,
            height: badge_h,
        };

        // Assert bounds placement
        assert!(badge_rect.x > block_x);
        assert!(badge_rect.x + badge_rect.width < block_x + block_w);

        // Centered text anchor point
        let text_x = badge_rect.x + badge_w / 2.0;
        let text_y = badge_rect.y + badge_h / 2.0;

        assert_eq!(text_x, badge_rect.x + badge_w / 2.0);
        assert_eq!(text_y, badge_rect.y + badge_h / 2.0);
    }

    #[test]
    fn test_get_equation_and_image_number() {
        let md = "Here is an image:\n![Alt](image.png)\nAnd a math block:\n$$\nE = mc^2\n$$\nAnother image:\n![Alt2](pic.png)";
        let lines = highlight_markdown(md);

        // Find math block id
        let math_block_id = lines[3].block_id;
        assert_eq!(get_equation_number(&lines, math_block_id), Some(1));

        // Find image span and test get_image_number
        let img1_span = &lines[1].spans[0];
        let img2_span = &lines[7].spans[0];
        assert_eq!(get_image_number(img1_span), Some(1));
        assert_eq!(get_image_number(img2_span), Some(2));
    }

    #[test]
    fn test_get_table_and_code_number() {
        let md = "Here is a code block:\n```rust\nfn main() {}\n```\nAnd a table:\n| a | b |\n|---|---|\n| 1 | 2 |\nAnother code:\n```\nhello\n```";
        let lines = highlight_markdown(md);
        let block_start = |block_id| {
            lines
                .iter()
                .position(|line| line.block_id == block_id)
                .expect("block start")
        };

        assert_eq!(
            block_number_from_start(&lines, block_start(lines[1].block_id), "code-"),
            Some(1)
        );
        assert_eq!(
            block_number_from_start(&lines, block_start(lines[5].block_id), "table-"),
            Some(1)
        );
        assert_eq!(
            block_number_from_start(&lines, block_start(lines[10].block_id), "code-"),
            Some(2)
        );
    }
}
