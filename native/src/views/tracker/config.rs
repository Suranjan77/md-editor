//! The tracker's configuration model: phases, projects, gates and reading lists, parsed from JSON, plus completion keys.

use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerConfig {
    #[serde(rename = "PHASES")]
    pub phases: Vec<PhaseConfig>,
    #[serde(rename = "PROJECTS")]
    pub projects: Vec<ProjectConfig>,
    #[serde(rename = "GATES")]
    pub gates: Vec<GateConfig>,
    #[serde(rename = "READING")]
    pub reading: Vec<ReadingSectionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseConfig {
    pub id: String,
    pub title: String,
    pub year: String,
    pub months: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub id: String,
    pub phase: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateConfig {
    pub id: String,
    pub title: String,
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingSectionConfig {
    pub section: String,
    pub items: Vec<ReadingItemConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingItemConfig {
    pub priority: String,
    pub title: String,
}

pub(super) fn content_hash(text: &str) -> u64 {
    text.as_bytes()
        .iter()
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
}

pub(crate) fn gate_completion_keys(gate: &str, item: &str, index: usize) -> (String, String) {
    (
        format!("gate_{gate}_{:016x}", content_hash(item)),
        format!("gate_{gate}_{index}"),
    )
}

pub(crate) fn reading_completion_keys(
    section: &str,
    title: &str,
    index: usize,
) -> (String, String) {
    (
        format!("read_{section}_{:016x}", content_hash(title)),
        format!("read_{section}_{index}"),
    )
}

pub(super) fn completion_checked(
    kv: &std::collections::HashMap<String, String>,
    keys: &(String, String),
) -> bool {
    kv.get(&keys.0)
        .or_else(|| kv.get(&keys.1))
        .is_some_and(|value| value == "true")
}

pub(super) const PHASES: &[(&str, &str, &str, &str)] = &[
    ("1A", "Mathematics", "Year 1", "Months 1-4"),
    ("1B", "Systems: C/C++ & Hardware", "Year 1", "Months 4-8"),
    ("1C", "Deep Learning Foundations", "Year 1", "Months 8-12"),
    ("2A", "Quantization", "Year 2", "Months 13-18"),
    ("2B", "Pruning", "Year 2", "Months 16-20"),
    ("2C", "Knowledge Distillation", "Year 2", "Months 19-22"),
    ("2D", "Efficient Training", "Year 2", "Months 21-23"),
    ("2E", "Deployment", "Year 2", "Months 23-24"),
    ("3A", "Low-Rank & PEFT", "Year 3", "Months 25-29"),
    ("3B", "Mixture of Experts", "Year 3", "Months 28-32"),
    ("3C", "State Space Models", "Year 3", "Months 31-34"),
    ("3D", "Attention & Spec. Decoding", "Year 3", "Months 33-37"),
    ("3E", "Training at Scale", "Year 3", "Months 35-38"),
    ("3F", "TinyML & Edge", "Year 3", "Months 37-40"),
    ("4A", "Research Methodology", "Year 4", "Months 41-44"),
    ("4B", "Advanced Topics", "Year 4", "Months 43-48"),
    ("4C", "Original Research", "Year 4", "Months 46-48+"),
];

pub(super) const PROJECTS: &[(&str, &str, &str)] = &[
    ("1.1", "1A", "SVD from scratch (NumPy eig() only)"),
    ("1.2", "1A", "KL divergence explorer"),
    ("1.3", "1A", "Scalar autograd engine"),
    ("1.5", "1B", "Cache-aware matrix multiplication"),
    ("1.7", "1B", "First HIP kernel"),
    ("1.10", "1C", "Transformer from scratch"),
    ("2.2", "2A", "PTQ from scratch"),
    ("2.3", "2A", "QAT implementation"),
    ("2.6", "2B", "Unstructured magnitude pruning"),
    ("2.10", "2C", "Classic temperature distillation"),
    ("2.13", "2D", "Gradient checkpointing"),
    ("2.16", "2E", "Full export pipeline"),
    ("3.1", "3A", "LoRA from scratch"),
    ("3.5", "3B", "MoE FFN layer"),
    ("3.8", "3C", "Mamba block implementation"),
    ("3.11", "3D", "FlashAttention tiling"),
    ("3.15", "3E", "FSDP training"),
    ("3.18", "3F", "MobileNetV2 + TFLite"),
    ("4.1", "4A", "Reproducibility study"),
    ("4.8", "4C", "Research proposal"),
];

pub(super) const GATES: &[(&str, &str, &[&str])] = &[
    (
        "1A",
        "Gate 1A - Mathematics",
        &[
            "Derive SVD from eigendecomposition",
            "Explain KL divergence asymmetry",
            "Derive Adam update rule",
            "Derive softmax cross-entropy gradients",
        ],
    ),
    (
        "1B",
        "Gate 1B - Systems",
        &[
            "Explain cache hierarchy and tiled matmul",
            "Write a HIP kernel from memory",
            "Explain GPU occupancy and coalescing",
        ],
    ),
    (
        "1C",
        "Gate 1C - Deep Learning",
        &[
            "Explain transformer forward/backward pass",
            "Read profiler FLOP and bandwidth data",
            "Explain roofline model",
        ],
    ),
    (
        "2A",
        "Gate 2A - Quantization",
        &[
            "Symmetric vs asymmetric quantization",
            "Explain per-channel quantization",
            "Derive STE gradient",
            "Explain Hessian role in GPTQ",
        ],
    ),
    (
        "3D",
        "Gate 3D - Attention",
        &[
            "Explain FlashAttention IO bottleneck",
            "Implement KV cache + GQA",
            "Explain speculative decoding guarantee",
        ],
    ),
    (
        "4C",
        "Gate 4C - Original Research",
        &[
            "Write complete paper draft",
            "Release reproducible code",
            "Present and defend work",
        ],
    ),
];

pub(super) const READING_ITEMS: &[(&str, &[(&str, &str)])] = &[
    (
        "Textbooks - Mathematics",
        &[
            ("critical", "Linear Algebra Done Right - Axler"),
            (
                "critical",
                "Information Theory, Inference, and Learning Algorithms - MacKay",
            ),
            ("important", "Convex Optimization - Boyd & Vandenberghe"),
        ],
    ),
    (
        "Textbooks - Systems",
        &[
            ("critical", "Computer Systems: A Programmer's Perspective"),
            ("critical", "C Programming Language - K&R"),
            ("critical", "AMD ROCm & HIP Programming Guide"),
        ],
    ),
    (
        "Deep Learning",
        &[
            ("important", "Deep Learning - Goodfellow et al."),
            ("important", "Dive into Deep Learning"),
        ],
    ),
    (
        "Efficient AI Papers",
        &[
            ("critical", "Quantization and GPTQ"),
            ("critical", "Pruning and SparseGPT"),
            ("critical", "FlashAttention and PagedAttention"),
        ],
    ),
];

pub fn default_config_json() -> String {
    serde_json::to_string_pretty(&default_config()).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod completion_key_tests {
    use super::*;

    #[test]
    fn content_keys_survive_reordering() {
        let first = gate_completion_keys("g", "alpha", 0).0;
        let reordered = gate_completion_keys("g", "alpha", 4).0;
        assert_eq!(first, reordered);
        assert_ne!(first, gate_completion_keys("g", "beta", 0).0);
    }

    #[test]
    fn manual_dates_can_be_normalized() {
        let date = chrono::NaiveDate::parse_from_str("2026-7-16", "%Y-%m-%d").unwrap();
        assert_eq!(date.format("%Y-%m-%d").to_string(), "2026-07-16");
    }
}

pub fn parse_config(json: &str) -> Result<TrackerConfig, String> {
    let config: TrackerConfig = serde_json::from_str(json).map_err(|err| err.to_string())?;
    if config.phases.is_empty() {
        return Err("PHASES must contain at least one phase".to_string());
    }
    if config.projects.is_empty() {
        return Err("PROJECTS must contain at least one project".to_string());
    }
    Ok(config)
}

pub fn config_or_default(json: &str) -> TrackerConfig {
    parse_config(json).unwrap_or_else(|_| default_config())
}

pub(super) fn default_config() -> TrackerConfig {
    TrackerConfig {
        phases: PHASES
            .iter()
            .map(|(id, title, year, months)| PhaseConfig {
                id: (*id).to_string(),
                title: (*title).to_string(),
                year: (*year).to_string(),
                months: (*months).to_string(),
            })
            .collect(),
        projects: PROJECTS
            .iter()
            .map(|(id, phase, name)| ProjectConfig {
                id: (*id).to_string(),
                phase: (*phase).to_string(),
                name: (*name).to_string(),
            })
            .collect(),
        gates: GATES
            .iter()
            .map(|(id, title, items)| GateConfig {
                id: (*id).to_string(),
                title: (*title).to_string(),
                items: items.iter().map(|item| (*item).to_string()).collect(),
            })
            .collect(),
        reading: READING_ITEMS
            .iter()
            .map(|(section, items)| ReadingSectionConfig {
                section: (*section).to_string(),
                items: items
                    .iter()
                    .map(|(priority, title)| ReadingItemConfig {
                        priority: (*priority).to_string(),
                        title: (*title).to_string(),
                    })
                    .collect(),
            })
            .collect(),
    }
}
