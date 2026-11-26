//! Opt-in sandboxed/bounded fenced code block execution (`Run Code Block`).

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests;

const OUTPUT_MAX_BYTES: usize = 64 * 1024;
const EXEC_TIMEOUT: Duration = Duration::from_secs(5);

/// A closed fenced code block enclosing the editor cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FenceBlock {
    pub lang: String,
    pub body: String,
    pub open_line: usize,
    pub close_line: usize,
    /// Line range `[start, end)` of an immediately following `> [!output]` callout.
    pub existing_output: Option<(usize, usize)>,
}

/// Captured bounded output of one opt-in code block execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutput {
    pub stdout: String,
    pub stderr: String,
    pub status_code: Option<i32>,
    pub truncated: bool,
}

/// Whether `lang` is supported by the local fenced block runner.
pub fn is_supported_language(lang: &str) -> bool {
    matches!(
        lang.trim().to_ascii_lowercase().as_str(),
        "sh" | "bash" | "python" | "py" | "python3"
    )
}

/// Find the closed fenced code block containing `cursor_line`, plus any
/// immediate `> [!output]` callout beneath it.
pub fn enclosing_fence(text: &str, cursor_line: usize) -> Option<FenceBlock> {
    let lines: Vec<&str> = text.split('\n').map(|l| l.trim_end_matches('\r')).collect();
    let mut active: Option<(char, usize, usize, String, Vec<&str>)> = None;

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if let Some((marker, marker_len, open_line, ref lang, ref mut body)) = active {
            if is_closing_fence(trimmed, marker, marker_len) {
                if (open_line..=idx).contains(&cursor_line) {
                    let existing_output = find_existing_output(&lines, idx + 1);
                    return Some(FenceBlock {
                        lang: lang.clone(),
                        body: body.join("\n"),
                        open_line,
                        close_line: idx,
                        existing_output,
                    });
                }
                active = None;
            } else {
                body.push(line);
            }
        } else if let Some((marker, marker_len, info)) = parse_opening_fence(trimmed) {
            let lang = info.split_whitespace().next().unwrap_or("").to_string();
            active = Some((marker, marker_len, idx, lang, Vec::new()));
        }
    }
    None
}

fn parse_opening_fence(trimmed: &str) -> Option<(char, usize, &str)> {
    let marker = trimmed.chars().next().filter(|&c| c == '`' || c == '~')?;
    let marker_len = trimmed.chars().take_while(|&c| c == marker).count();
    if marker_len < 3 {
        return None;
    }
    let info = trimmed[marker_len..].trim();
    if marker == '`' && info.contains('`') {
        return None;
    }
    Some((marker, marker_len, info))
}

fn is_closing_fence(trimmed: &str, marker: char, open_len: usize) -> bool {
    let count = trimmed.chars().take_while(|&c| c == marker).count();
    count >= open_len && trimmed[count..].trim().is_empty()
}

fn find_existing_output(lines: &[&str], after_close: usize) -> Option<(usize, usize)> {
    let mut start = after_close;
    if lines.get(start).is_some_and(|l| l.trim().is_empty()) {
        start += 1;
    }
    let header = lines.get(start)?.trim_start();
    if !header
        .get(..11)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("> [!output]"))
    {
        return None;
    }
    let mut end = start + 1;
    while lines
        .get(end)
        .is_some_and(|l| l.trim_start().starts_with('>'))
    {
        end += 1;
    }
    Some((start, end))
}

/// Execute `code` for `lang` inside `cwd` with a strict 5s timeout and 64 KB
/// per-stream capture limit.
pub fn run_fence_command(lang: &str, code: &str, cwd: Option<&Path>) -> Result<RunOutput, String> {
    let program = match lang.trim().to_ascii_lowercase().as_str() {
        "sh" | "bash" => "sh",
        "python" | "py" | "python3" => "python3",
        other => {
            return Err(format!(
                "Unsupported code block language '{other}' (supported: sh, bash, python)"
            ));
        }
    };

    let mut cmd = Command::new(program);
    cmd.arg("-c")
        .arg(code)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd
        && dir.is_dir()
    {
        cmd.current_dir(dir);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start {program}: {e}"))?;

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let out_handle = std::thread::spawn(move || read_bounded(stdout_pipe));
    let err_handle = std::thread::spawn(move || read_bounded(stderr_pipe));

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {
                if started.elapsed() >= EXEC_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err("Timed out after 5s".to_string());
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            Err(e) => {
                let _ = child.kill();
                break Err(format!("Failed waiting on {program}: {e}"));
            }
        }
    };

    // Return timeout/wait errors immediately without blocking if a sub-process inherited pipes.
    let status = status?;
    let (stdout, out_trunc) = out_handle.join().unwrap_or_default();
    let (stderr, err_trunc) = err_handle.join().unwrap_or_default();

    Ok(RunOutput {
        stdout,
        stderr,
        status_code: status.code(),
        truncated: out_trunc || err_trunc,
    })
}

fn read_bounded(pipe: Option<impl Read>) -> (String, bool) {
    let Some(mut reader) = pipe else {
        return (String::new(), false);
    };
    let mut captured = Vec::new();
    let mut truncated = false;
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if captured.len() < OUTPUT_MAX_BYTES {
                    let room = OUTPUT_MAX_BYTES - captured.len();
                    let take = n.min(room);
                    captured.extend_from_slice(&buf[..take]);
                    if take < n {
                        truncated = true;
                    }
                } else {
                    truncated = true;
                }
            }
        }
    }
    (String::from_utf8_lossy(&captured).into_owned(), truncated)
}

/// Format execution output as a git-diffable `> [!output]` Markdown callout.
pub fn format_output_callout(out: &RunOutput) -> String {
    let header = match out.status_code {
        Some(0) => "> [!output]".to_string(),
        Some(code) => format!("> [!output] Exit {code}"),
        None => "> [!output] Terminated".to_string(),
    };
    let mut lines = vec![header];
    let stdout = out.stdout.trim_end_matches(['\r', '\n']);
    let stderr = out.stderr.trim_end_matches(['\r', '\n']);

    if stdout.is_empty() && stderr.is_empty() {
        lines.push("> (no output)".to_string());
    } else {
        for line in stdout.lines() {
            push_quoted_line(&mut lines, line);
        }
        if !stderr.is_empty() {
            if !stdout.is_empty() {
                lines.push(">".to_string());
            }
            lines.push("> [stderr]".to_string());
            for line in stderr.lines() {
                push_quoted_line(&mut lines, line);
            }
        }
    }
    if out.truncated {
        lines.push("> ... (output truncated at 64 KB)".to_string());
    }
    lines.join("\n")
}

fn push_quoted_line(lines: &mut Vec<String>, line: &str) {
    if line.is_empty() {
        lines.push(">".to_string());
    } else {
        lines.push(format!("> {line}"));
    }
}

/// Compute `(start_char, end_char, replacement)` to insert or idempotently replace
/// the `> [!output]` callout immediately following `fence`.
pub fn compute_output_edit(
    text: &str,
    eol: &str,
    fence: &FenceBlock,
    callout: &str,
) -> (usize, usize, String) {
    let callout_eol = callout.replace('\n', eol);
    let mut line_starts = vec![0usize];
    for (idx, ch) in text.chars().enumerate() {
        if ch == '\n' {
            line_starts.push(idx + 1);
        }
    }
    let total_chars = text.chars().count();
    let line_end_without_eol = |line: usize| -> usize {
        let next_start = line_starts.get(line + 1).copied().unwrap_or(total_chars);
        if next_start == 0 {
            return 0;
        }
        let mut end = next_start;
        if line + 1 < line_starts.len() {
            // Strip `\n` and optional `\r` before it.
            end = end.saturating_sub(1);
            if end > 0 && text.chars().nth(end - 1) == Some('\r') {
                end -= 1;
            }
        }
        end
    };

    if let Some((out_start, out_end_excl)) = fence.existing_output {
        let start_char = line_starts[out_start];
        let end_char = line_end_without_eol(out_end_excl - 1);
        (start_char, end_char, callout_eol)
    } else {
        let insert_at = line_end_without_eol(fence.close_line);
        (insert_at, insert_at, format!("{eol}{callout_eol}"))
    }
}
