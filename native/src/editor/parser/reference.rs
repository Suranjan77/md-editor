use super::metadata::styled_line_source;
use super::model::StyledLine;

pub(crate) fn collect_reference_definitions(
    lines: &[StyledLine],
) -> std::collections::HashMap<String, String> {
    let mut defs = std::collections::HashMap::new();
    for line in lines {
        let source = styled_line_source(line);
        if let Some((label, target, ..)) = parse_reference_definition(&source) {
            defs.insert(label, target);
        }
    }
    defs
}

pub(crate) fn get_ref_id_from_span_text(text: &str) -> Option<String> {
    if !text.starts_with('[') || !text.ends_with(']') {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    if let Some(pos) = text.find("][") {
        Some(chars[pos + 2..chars.len() - 1].iter().collect())
    } else {
        if text.starts_with("[^") {
            None
        } else {
            Some(chars[1..chars.len() - 1].iter().collect())
        }
    }
}

pub(crate) fn parse_reference_definition(
    line: &str,
) -> Option<(String, String, usize, usize, usize)> {
    let trimmed = line.trim_start();
    let leading_spaces_len = line.len() - trimmed.len();
    if leading_spaces_len > 3 {
        return None;
    }
    if !trimmed.starts_with('[') {
        return None;
    }
    let chars: Vec<char> = trimmed.chars().collect();
    let mut end_label = None;
    for i in 1..chars.len() {
        if chars[i] == ']' {
            if i + 1 < chars.len() && chars[i + 1] == ':' {
                end_label = Some(i);
                break;
            }
        }
    }
    let idx = end_label?;
    let label: String = chars[1..idx].iter().collect();
    let label = label.trim().to_lowercase();
    if label.is_empty() {
        return None;
    }

    let rest_start = idx + 2;
    let rest = &trimmed[rest_start..];
    let rest_trimmed = rest.trim_start();
    let spaces_len = rest.len() - rest_trimmed.len();
    let target_start = rest_start + spaces_len;

    if rest_trimmed.is_empty() {
        return None;
    }

    let target_len = if rest_trimmed.starts_with('<') {
        if let Some(end_bracket) = rest_trimmed.find('>') {
            end_bracket + 1
        } else {
            return None;
        }
    } else {
        rest_trimmed.split_whitespace().next()?.len()
    };

    let target = if rest_trimmed.starts_with('<') {
        &rest_trimmed[1..target_len - 1]
    } else {
        &rest_trimmed[..target_len]
    };

    Some((label, target.to_string(), idx, target_start, target_len))
}
