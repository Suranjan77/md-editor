//! Opt-in literate synthesis helpers: inline math evaluation (`EvaluateMath`) and fenced code execution (`RunCodeBlock`).

use super::*;
use crate::code_runner;
use crate::messages::EditorMessage;
use crate::ui::ToastKind;

impl MdEditor {
    /// Evaluate the selected math or enclosing `$...$` / `$$...$$` expression in one undo step.
    pub(super) fn evaluate_math_at_cursor(&mut self) -> Task<Message> {
        if !self.note_is_showing() {
            return Task::none();
        }
        let text = self.editor.buffer.text();
        let cursor = self.editor.buffer.cursor_offset();
        let selection = self
            .editor
            .buffer
            .selection_offsets()
            .map(|s| s.range())
            .filter(|(a, b)| a < b);

        let Some(plan) = plan_math_eval(&text, cursor, selection) else {
            self.ui.notify(
                ToastKind::Warning,
                "Select a math expression or place the cursor inside $...$",
                false,
            );
            return Task::none();
        };

        let result = match md_editor_core::math_eval::evaluate_formatted(&plan.expr) {
            Ok(res) => res,
            Err(err) => {
                self.ui.notify(ToastKind::Warning, err, false);
                return Task::none();
            }
        };

        let replacement = format!(
            "{}{} = {result}{}",
            plan.leading_ws, plan.expr, plan.trailing_ws
        );
        let cursor_after = plan.replace_start + replacement.chars().count();
        self.run_editor_command(EditorCommand::ReplaceRange {
            start_char: plan.replace_start,
            end_char: plan.replace_end,
            text: replacement,
            cursor_char: Some(cursor_after),
        })
    }

    /// Run the enclosing fenced code block (`sh`, `bash`, `python`) off the UI thread.
    pub(super) fn run_code_block_at_cursor(&mut self) -> Task<Message> {
        if !self.note_is_showing() {
            return Task::none();
        }
        let Some(active_path) = self.active_path.clone() else {
            return Task::none();
        };
        let text = self.editor.buffer.text();
        let cursor_line = self.editor.buffer.cursor_line;
        let Some(fence) = code_runner::enclosing_fence(&text, cursor_line) else {
            self.ui.notify(
                ToastKind::Warning,
                "Place cursor inside a fenced code block (```sh or ```python)",
                false,
            );
            return Task::none();
        };
        if !code_runner::is_supported_language(&fence.lang) {
            self.ui.notify(
                ToastKind::Warning,
                format!(
                    "Unsupported code block language '{}' (supported: sh, bash, python)",
                    fence.lang
                ),
                false,
            );
            return Task::none();
        }

        let pane = self.workspace.focused;
        let cwd = self.vault.root.clone().map(std::path::PathBuf::from);
        let lang = fence.lang;
        let body = fence.body;
        let expected_close_line = fence.close_line;
        let expected_body = body.clone();
        self.ui
            .notify(ToastKind::Info, "Running code block…", false);

        Task::perform(
            super::blocking(move || {
                let out = code_runner::run_fence_command(&lang, &body, cwd.as_deref())?;
                Ok(code_runner::format_output_callout(&out))
            }),
            move |result| {
                Message::ToPane(
                    pane,
                    Box::new(Message::Editor(EditorMessage::CodeBlockFinished {
                        expected_path: active_path,
                        expected_close_line,
                        expected_body,
                        result,
                    })),
                )
            },
        )
    }

    pub(super) fn on_code_block_finished(
        &mut self,
        expected_path: String,
        expected_close_line: usize,
        expected_body: String,
        result: Result<String, String>,
    ) -> Task<Message> {
        if self.active_path.as_deref() != Some(expected_path.as_str()) {
            return Task::none();
        }
        let callout = match result {
            Ok(text) => text,
            Err(err) => {
                self.ui.notify(ToastKind::Warning, err, false);
                return Task::none();
            }
        };
        let buffer_text = self.editor.buffer.text();
        let Some(fence) = code_runner::enclosing_fence(&buffer_text, expected_close_line)
            .filter(|f| f.close_line == expected_close_line && f.body == expected_body)
        else {
            self.ui.notify(
                ToastKind::Warning,
                "Code block changed while running; output skipped",
                false,
            );
            return Task::none();
        };
        let eol = if buffer_text.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let (start_char, end_char, text) =
            code_runner::compute_output_edit(&buffer_text, eol, &fence, &callout);
        let cursor_char = self.editor.buffer.cursor_offset();
        self.run_editor_command(EditorCommand::ReplaceRange {
            start_char,
            end_char,
            text,
            cursor_char: Some(cursor_char),
        })
    }
}

struct MathEvalPlan {
    replace_start: usize,
    replace_end: usize,
    leading_ws: String,
    expr: String,
    trailing_ws: String,
}

fn plan_math_eval(
    text: &str,
    cursor: usize,
    selection: Option<(usize, usize)>,
) -> Option<MathEvalPlan> {
    let chars: Vec<char> = text.chars().collect();
    let (inner_start, inner_end) = if let Some((s, e)) = selection {
        unwrap_selected_math(&chars, s, e)
    } else {
        find_enclosing_math(&chars, cursor)?
    };
    let inner: String = chars[inner_start..inner_end].iter().collect();
    let lead_chars = inner.chars().take_while(|c| c.is_whitespace()).count();
    let trail_chars = inner
        .chars()
        .rev()
        .take_while(|c| c.is_whitespace())
        .count();
    let core_end = inner.chars().count().saturating_sub(trail_chars);
    if lead_chars >= core_end {
        return None;
    }
    let leading_ws: String = inner.chars().take(lead_chars).collect();
    let trailing_ws: String = inner.chars().skip(core_end).collect();
    let core: String = inner
        .chars()
        .skip(lead_chars)
        .take(core_end - lead_chars)
        .collect();
    let expr = strip_trailing_eval(&core).trim().to_string();
    if expr.is_empty() {
        return None;
    }
    Some(MathEvalPlan {
        replace_start: inner_start,
        replace_end: inner_end,
        leading_ws,
        expr,
        trailing_ws,
    })
}

fn unwrap_selected_math(chars: &[char], s: usize, e: usize) -> (usize, usize) {
    let len = e.saturating_sub(s);
    if len >= 4 && chars[s..s + 2] == ['$', '$'] && chars[e - 2..e] == ['$', '$'] {
        (s + 2, e - 2)
    } else if len >= 2 && chars[s] == '$' && chars[e - 1] == '$' {
        (s + 1, e - 1)
    } else {
        (s, e)
    }
}

fn find_enclosing_math(chars: &[char], cursor: usize) -> Option<(usize, usize)> {
    // Check `$$...$$` display math first, then single-line `$...$`.
    if let Some(pair) = find_delimited(chars, cursor, &['$', '$'], true) {
        return Some(pair);
    }
    find_delimited(chars, cursor, &['$'], false)
}

fn find_delimited(
    chars: &[char],
    cursor: usize,
    delim: &[char],
    allow_newlines: bool,
) -> Option<(usize, usize)> {
    let mut i = 0usize;
    let mut open: Option<usize> = None;
    while i + delim.len() <= chars.len() {
        if i > 0 && chars[i - 1] == '\\' {
            i += 1;
            continue;
        }
        let matches_delim = if delim.len() == 1 {
            chars[i] == '$'
                && chars.get(i + 1).copied() != Some('$')
                && (i == 0 || chars[i - 1] != '$')
        } else {
            chars[i..i + 2] == ['$', '$']
        };
        if matches_delim {
            if let Some(start) = open {
                if cursor >= start.saturating_sub(delim.len()) && cursor <= i + delim.len() {
                    let body: String = chars[start..i].iter().collect();
                    if allow_newlines || !body.contains('\n') {
                        return Some((start, i));
                    }
                }
                open = None;
            } else {
                open = Some(i + delim.len());
            }
            i += delim.len();
            continue;
        }
        if !allow_newlines && chars[i] == '\n' {
            open = None;
        }
        i += 1;
    }
    None
}

fn strip_trailing_eval(expr: &str) -> &str {
    let Some(eq) = expr.rfind('=') else {
        return expr;
    };
    let rhs = expr[eq + 1..].trim();
    let looks_numeric = !rhs.is_empty()
        && rhs
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E'));
    if looks_numeric { &expr[..eq] } else { expr }
}
