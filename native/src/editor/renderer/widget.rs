use super::*;
use crate::editor::buffer::{DocBuffer, EditorCommand, Movement};
use crate::editor::highlight::{StyledLine, StyledSpan};
use crate::editor::layout_cache::{LineHeightCache, line_hash, resource_hash};
use crate::editor::renderer::geometry::{clip_viewport, normalized_selection};
use crate::{search, theme};
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

#[allow(clippy::type_complexity)]
pub struct Editor<'a, Message> {
    pub(crate) buffer: &'a DocBuffer,
    pub(crate) lines: &'a [StyledLine],
    pub(crate) image_cache: &'a HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    pub(crate) math_cache: &'a HashMap<String, (iced::widget::image::Handle, f32, f32)>,
    pub(crate) search_query: &'a str,
    pub(crate) search_regex: bool,
    pub(crate) search_match_case: bool,
    pub(crate) active_search_match: Option<(usize, usize)>,
    pub(crate) modifiers: keyboard::Modifiers,
    pub(crate) on_command: Box<dyn Fn(EditorCommand) -> Message + 'a>,
    pub(crate) on_pointer_command: Box<dyn Fn(EditorCommand) -> Message + 'a>,
    pub(crate) on_link_click: Box<dyn Fn(String) -> Message + 'a>,
    pub(crate) on_checkbox_toggle: Box<dyn Fn(usize) -> Message + 'a>,
    pub(crate) on_block_context_menu: Option<Box<dyn Fn(usize, Point) -> Message + 'a>>,
    pub(crate) on_context_menu: Option<Box<dyn Fn(usize, usize, Point) -> Message + 'a>>,
    pub(crate) vault_root: Option<&'a str>,
    pub(crate) active_path: Option<&'a str>,
    pub(crate) existing_files: Option<HashSet<String>>,
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
                            let mut c = theme::accent();
                            c.a = 0.50;
                            c
                        } else {
                            let mut c = theme::warning();
                            c.a = 0.30;
                            c
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
                                y: start_y + (BASE_LINE_HEIGHT + fs) / 2.0 + 2.0,
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
