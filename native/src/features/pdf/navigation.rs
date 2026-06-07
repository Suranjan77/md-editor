use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq)]
pub enum NavigationTarget {
    Markdown {
        path: String,
        line: usize,
        column: usize,
    },
    Pdf {
        path: String,
        page: u16,
        scroll_offset: f32,
        zoom: f32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct NavigationPoint {
    pub target: NavigationTarget,
    pub timestamp: u64,
}

impl NavigationPoint {
    pub fn new(target: NavigationTarget) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self { target, timestamp }
    }
}

#[derive(Debug, Clone)]
pub struct NavigationHistory {
    pub entries: Vec<NavigationPoint>,
    pub current_index: usize,
    pub max_entries: usize,
}

impl Default for NavigationHistory {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            current_index: 0,
            max_entries: 50,
        }
    }
}

impl NavigationHistory {
    pub fn push(&mut self, target: NavigationTarget) {
        // Truncate forward history if we are in the middle of the stack
        if !self.entries.is_empty() && self.current_index < self.entries.len() - 1 {
            self.entries.truncate(self.current_index + 1);
        }

        // Avoid duplicate consecutive targets
        if let Some(last) = self.entries.last() {
            if last.target == target {
                return;
            }
        }

        self.entries.push(NavigationPoint::new(target));
        if self.entries.len() > self.max_entries {
            self.entries.remove(0);
        }
        self.current_index = self.entries.len().saturating_sub(1);
    }

    pub fn go_back(&mut self) -> Option<NavigationTarget> {
        if self.current_index > 0 && !self.entries.is_empty() {
            self.current_index -= 1;
            Some(self.entries[self.current_index].target.clone())
        } else {
            None
        }
    }

    pub fn go_forward(&mut self) -> Option<NavigationTarget> {
        if self.current_index + 1 < self.entries.len() {
            self.current_index += 1;
            Some(self.entries[self.current_index].target.clone())
        } else {
            None
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfLinkTarget {
    pub path: String,
    pub page: Option<u16>,
    pub annotation_id: Option<String>,
}

pub fn build_pdf_link(path: &str, page: Option<u16>, annotation_id: Option<&str>) -> String {
    let mut link = format!("pdf://{}", percent_encode(path));
    let mut query = Vec::new();
    if let Some(page) = page {
        query.push(format!("page={page}"));
    }
    if let Some(annotation_id) = annotation_id {
        query.push(format!("annotation={}", percent_encode(annotation_id)));
    }
    if !query.is_empty() {
        link.push('?');
        link.push_str(&query.join("&"));
    }
    link
}

pub fn parse_pdf_link(link: &str) -> Option<PdfLinkTarget> {
    let rest = if link.starts_with("pdf://") {
        &link[6..]
    } else if link.contains(".pdf") {
        link
    } else {
        return None;
    };
    let (raw_path, raw_query) = if let Some(idx) = rest.find('?') {
        let (p, q) = rest.split_at(idx);
        (p, &q[1..])
    } else if let Some(idx) = rest.find('#') {
        let (p, q) = rest.split_at(idx);
        (p, &q[1..])
    } else {
        (rest, "")
    };
    let path = percent_decode(raw_path).unwrap_or_else(|| raw_path.to_string());
    if !path.to_lowercase().ends_with(".pdf") {
        return None;
    }

    let mut page = None;
    let mut annotation_id = None;
    for pair in raw_query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "page" => {
                if let Ok(parsed) = value.parse::<u16>() {
                    page = Some(parsed);
                }
            }
            "annotation" => {
                annotation_id = Some(percent_decode(value).unwrap_or_else(|| value.to_string()));
            }
            _ => {}
        }
    }

    Some(PdfLinkTarget {
        path,
        page,
        annotation_id,
    })
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'%' {
            let hi = *bytes.get(idx + 1)?;
            let lo = *bytes.get(idx + 2)?;
            decoded.push(hex_value(hi)? * 16 + hex_value(lo)?);
            idx += 3;
        } else {
            decoded.push(bytes[idx]);
            idx += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_and_parses_pdf_links_with_markdown_url_special_chars() {
        let link = build_pdf_link("papers/a (b)?c#d&e.pdf", Some(5), Some("ann&id)"));
        assert_eq!(
            link,
            "pdf://papers/a%20%28b%29%3Fc%23d%26e.pdf?page=5&annotation=ann%26id%29"
        );

        assert_eq!(
            parse_pdf_link(&link),
            Some(PdfLinkTarget {
                path: "papers/a (b)?c#d&e.pdf".to_string(),
                page: Some(5),
                annotation_id: Some("ann&id)".to_string()),
            })
        );
    }

    #[test]
    fn parses_legacy_raw_pdf_links() {
        assert_eq!(
            parse_pdf_link("pdf://papers/My PDF File.pdf?page=2&annotation=abc"),
            Some(PdfLinkTarget {
                path: "papers/My PDF File.pdf".to_string(),
                page: Some(2),
                annotation_id: Some("abc".to_string()),
            })
        );
    }

    #[test]
    fn parses_pdf_links_with_hash_delimiter() {
        assert_eq!(
            parse_pdf_link("pdf://papers/My PDF File.pdf#page=5&annotation=xyz"),
            Some(PdfLinkTarget {
                path: "papers/My PDF File.pdf".to_string(),
                page: Some(5),
                annotation_id: Some("xyz".to_string()),
            })
        );
    }

    #[test]
    fn parses_pdf_links_without_prefix() {
        assert_eq!(
            parse_pdf_link("papers/My PDF File.pdf#page=5&annotation=xyz"),
            Some(PdfLinkTarget {
                path: "papers/My PDF File.pdf".to_string(),
                page: Some(5),
                annotation_id: Some("xyz".to_string()),
            })
        );
        assert_eq!(parse_pdf_link("folder.pdf/file.md"), None);
    }
}
