//! What a running study session worked on: time per note, PDF and sketch, pages read, words written, highlights made.
//!
//! The shell reports an [`Observation`] after every update while the timer
//! runs; nothing else needs to know a session is being timed. Time goes to
//! the document in front at the previous observation, so switching documents
//! splits the session between them.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Documents in front for less than this, and not changed, are left out of
/// the summary (a glance while switching is not work).
const MIN_SHOWN: Duration = Duration::from_secs(30);
/// Documents named in the summary; the rest are counted.
const MAX_SHOWN: usize = 3;

/// The app's state as far as a study session cares, after one update.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Observation {
    /// Vault-relative path of the document in front (note, PDF or sketch).
    pub focused: Option<String>,
    /// The open note and its word count.
    pub note: Option<(String, usize)>,
    /// The open PDF, its current page (0-based) and how many highlights it
    /// has, once they have loaded (`None` before: an opening PDF's saved
    /// highlights arriving are not new ones).
    pub pdf: Option<(String, u16, Option<usize>)>,
}

#[derive(Debug, Default)]
struct DocWork {
    time: Duration,
    /// First and latest word count seen.
    words: Option<(usize, usize)>,
    /// Lowest and highest page seen, 0-based.
    pages: Option<(u16, u16)>,
    /// First and latest highlight count seen.
    highlights: Option<(usize, usize)>,
    /// First time seen, for a stable order among equals.
    order: usize,
}

impl DocWork {
    fn words_added(&self) -> i64 {
        self.words
            .map_or(0, |(first, last)| last as i64 - first as i64)
    }

    fn highlights_added(&self) -> usize {
        self.highlights
            .map_or(0, |(first, last)| last.saturating_sub(first))
    }

    fn changed(&self) -> bool {
        self.words_added() != 0 || self.highlights_added() > 0
    }
}

#[derive(Debug)]
pub struct SessionContext {
    docs: HashMap<String, DocWork>,
    /// The document in front at the last observation, and when that was.
    current: Option<String>,
    last: Instant,
}

impl SessionContext {
    pub fn start(now: Instant, first: &Observation) -> Self {
        let mut context = Self {
            docs: HashMap::new(),
            current: None,
            last: now,
        };
        context.observe(now, first);
        context
    }

    pub fn observe(&mut self, now: Instant, seen: &Observation) {
        let elapsed = now.saturating_duration_since(self.last);
        if let Some(current) = self.current.clone() {
            self.doc(&current).time += elapsed;
        }
        self.last = now;
        self.current = seen.focused.clone();
        if let Some(focused) = &seen.focused {
            self.doc(focused);
        }
        if let Some((path, words)) = &seen.note {
            let work = self.doc(path);
            work.words = Some(
                work.words
                    .map_or((*words, *words), |(first, _)| (first, *words)),
            );
        }
        if let Some((path, page, highlights)) = &seen.pdf {
            let work = self.doc(path);
            if let Some(highlights) = *highlights {
                work.highlights = Some(
                    work.highlights
                        .map_or((highlights, highlights), |(first, _)| (first, highlights)),
                );
            }
            if seen.focused.as_deref() == Some(path.as_str()) {
                work.pages = Some(
                    work.pages
                        .map_or((*page, *page), |(lo, hi)| (lo.min(*page), hi.max(*page))),
                );
            }
        }
    }

    /// Close the session at `now`: the summary stored with it, e.g.
    /// `Vaswani2017.pdf p. 3–8, +3 highlights · attention.md, +410 words`.
    /// `None` when nothing was worked on long enough to name.
    pub fn finish(mut self, now: Instant) -> Option<String> {
        self.observe(
            now,
            &Observation {
                focused: None,
                ..Default::default()
            },
        );
        let mut shown: Vec<(&String, &DocWork)> = self
            .docs
            .iter()
            .filter(|(_, work)| work.time >= MIN_SHOWN || work.changed())
            .collect();
        shown.sort_by(|a, b| b.1.time.cmp(&a.1.time).then(a.1.order.cmp(&b.1.order)));
        if shown.is_empty() {
            return None;
        }
        let more = shown.len().saturating_sub(MAX_SHOWN);
        let mut parts: Vec<String> = shown
            .into_iter()
            .take(MAX_SHOWN)
            .map(|(path, work)| describe(path, work))
            .collect();
        if more > 0 {
            parts.push(format!("+{more} more"));
        }
        Some(parts.join(" · "))
    }

    fn doc(&mut self, path: &str) -> &mut DocWork {
        let order = self.docs.len();
        self.docs
            .entry(path.to_string())
            .or_insert_with(|| DocWork {
                order,
                ..Default::default()
            })
    }
}

fn describe(path: &str, work: &DocWork) -> String {
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);
    let mut text = name.to_string();
    if let Some((lo, hi)) = work.pages {
        if lo == hi {
            text.push_str(&format!(" p. {}", lo + 1));
        } else {
            text.push_str(&format!(" p. {}–{}", lo + 1, hi + 1));
        }
    }
    let highlights = work.highlights_added();
    if highlights > 0 {
        let s = if highlights == 1 { "" } else { "s" };
        text.push_str(&format!(", +{highlights} highlight{s}"));
    }
    match work.words_added() {
        0 => {}
        added if added > 0 => text.push_str(&format!(", +{added} words")),
        removed => text.push_str(&format!(", {removed} words")),
    }
    text
}

#[cfg(test)]
mod tests;
