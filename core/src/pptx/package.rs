//! The zip package and its relationship parts.

use super::*;
use super::{PptxError, PptxResult};

// ── Package and relationships ────────────────────────────────────────

pub(super) struct Package<R> {
    pub(super) archive: zip::ZipArchive<R>,
    pub(super) bytes_read: u64,
}

impl<R: Read + Seek> Package<R> {
    pub(super) fn open(reader: R) -> PptxResult<Self> {
        let archive = zip::ZipArchive::new(reader)
            .map_err(|_| "Not a valid .pptx file: it is not a zip package".to_string())?;
        Ok(Self {
            archive,
            bytes_read: 0,
        })
    }

    /// The bytes of `part`, or `None` when the package has no such part.
    pub(super) fn read(&mut self, part: &str) -> PptxResult<Option<Vec<u8>>> {
        let file = match self.archive.by_name(part) {
            Ok(file) => file,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(e) => return Err(PptxError::from(format!("Cannot read {part}: {e}"))),
        };
        if file.size() > MAX_PART_BYTES {
            return Err(PptxError::from(format!("{part} is too large to open")));
        }
        let mut bytes = Vec::with_capacity(file.size() as usize);
        file.take(MAX_PART_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| PptxError::from(format!("Cannot read {part}: {e}")))?;
        if bytes.len() as u64 > MAX_PART_BYTES {
            return Err(PptxError::from(format!("{part} is too large to open")));
        }
        self.bytes_read += bytes.len() as u64;
        if self.bytes_read > MAX_PACKAGE_BYTES {
            return Err(PptxError::from(
                "The presentation is too large to open".to_string(),
            ));
        }
        Ok(Some(bytes))
    }

    pub(super) fn read_text(&mut self, part: &str) -> PptxResult<Option<String>> {
        match self.read(part)? {
            Some(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| PptxError::from(format!("{part} is not UTF-8 XML"))),
            None => Ok(None),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct Relationship {
    /// Last segment of the relationship type URI: `slide`, `image`, `theme`…
    pub(super) kind: String,
    /// Package part name for internal targets; the raw target otherwise.
    pub(super) target: String,
    pub(super) external: bool,
}

pub(super) type Relationships = HashMap<String, Relationship>;

pub(super) fn rels_part(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// Resolve a relationship target against the part that holds it, yielding a
/// package part name without a leading slash.
pub(super) fn resolve_target(source_part: &str, target: &str) -> String {
    let target = percent_decode(&target.replace('\\', "/"));
    let mut segments: Vec<&str> = Vec::new();
    let relative = match target.strip_prefix('/') {
        Some(absolute) => absolute,
        None => {
            if let Some((dir, _)) = source_part.rsplit_once('/') {
                segments.extend(dir.split('/'));
            }
            target.as_str()
        }
    };
    for segment in relative.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    segments.join("/")
}

pub(super) fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(value) = u8::from_str_radix(&text[i + 1..i + 3], 16)
        {
            out.push(value);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

pub(super) fn read_relationships<R: Read + Seek>(
    package: &mut Package<R>,
    part: &str,
) -> PptxResult<Relationships> {
    let rels_name = rels_part(part);
    let Some(xml) = package.read_text(&rels_name)? else {
        return Ok(HashMap::new());
    };
    let Ok(doc) = parse_xml(&xml, &rels_name) else {
        return Ok(HashMap::new());
    };
    let mut rels = HashMap::new();
    for rel in children_named(doc.root_element(), "Relationship") {
        let (Some(id), Some(kind), Some(target)) = (
            rel.attribute("Id"),
            rel.attribute("Type"),
            rel.attribute("Target"),
        ) else {
            continue;
        };
        let external = rel.attribute("TargetMode") == Some("External");
        rels.insert(
            id.to_string(),
            Relationship {
                kind: kind.rsplit('/').next().unwrap_or(kind).to_string(),
                target: if external {
                    target.to_string()
                } else {
                    resolve_target(part, target)
                },
                external,
            },
        );
    }
    Ok(rels)
}

pub(super) fn target_of(rels: &Relationships, kind: &str) -> Option<String> {
    rels.values()
        .find(|rel| rel.kind == kind && !rel.external)
        .map(|rel| rel.target.clone())
}
