use super::*;
use crate::editor::parser::StyledLine;
use crate::editor::renderer::geometry::normalized_selection;
use iced::Point;

pub(crate) fn block_number_from_start(
    lines: &[crate::editor::parser::StyledLine],
    start: usize,
    prefix: &str,
) -> Option<usize> {
    let first_line = lines.get(start)?;
    let first_span = first_line.spans.first()?;
    let num_str = first_span.id.as_ref()?.strip_prefix(prefix)?;
    num_str.parse::<usize>().ok()
}

pub(crate) fn get_equation_number(
    lines: &[crate::editor::parser::StyledLine],
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

pub(crate) fn get_image_number(span: &crate::editor::parser::StyledSpan) -> Option<usize> {
    if let Some(ref id) = span.id {
        if let Some(num_str) = id.strip_prefix("figure-") {
            if let Ok(num) = num_str.parse::<usize>() {
                return Some(num);
            }
        }
    }
    None
}

pub(crate) fn is_heading_line(line: &crate::editor::parser::StyledLine) -> bool {
    line.spans
        .iter()
        .any(|span| span.is_syntax && span.visible_text(true).starts_with('#'))
}

pub(crate) fn is_checkbox_line(line: &crate::editor::parser::StyledLine) -> bool {
    line.spans.iter().any(|span| span.is_checkbox)
}

pub(crate) fn is_pdf_citation_line(line: &crate::editor::parser::StyledLine) -> bool {
    line.spans
        .iter()
        .any(|span| span.is_link && span.visible_text(true).starts_with("pdf://"))
}

pub(crate) fn get_block_context_menu_items(
    lines: &[crate::editor::parser::StyledLine],
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

pub(crate) fn source_col_after_span(
    span: &crate::editor::parser::StyledSpan,
    start_col: usize,
) -> usize {
    start_col + span.text.chars().count()
}

pub(crate) fn span_source_range(line: &StyledLine, span_idx: usize) -> Option<(usize, usize)> {
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

pub(crate) fn col_touches_range(col: usize, start: usize, end: usize) -> bool {
    col >= start && col <= end
}

pub(crate) fn span_is_inline_edit_target(
    line: &StyledLine,
    span_idx: usize,
    active_col: usize,
) -> bool {
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

pub(crate) fn span_visible_text<'a>(
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

pub(crate) fn is_block_editing_line(
    line: &StyledLine,
    active_block_id: Option<usize>,
    focused: bool,
) -> bool {
    focused
        && Some(line.block_id) == active_block_id
        && (line.is_code_block || line.is_math_block || line.is_table_row)
}

impl<'a, Message> Editor<'a, Message> {
    pub(crate) fn position_for_col<R>(
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

    pub(crate) fn col_for_visual_point<R>(
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

    pub(crate) fn selected_text(&self, state: &State) -> Option<String> {
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

    pub(crate) fn cursor_position<R>(&self, line_idx: usize, available_width: f32) -> (f32, f32)
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

    pub(crate) fn block_at_y<R>(
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

    /// Convert a click position (relative to widget bounds) into (line, col).
    pub(crate) fn hit_test<R>(
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
}
