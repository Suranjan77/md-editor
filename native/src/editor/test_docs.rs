//! Seeded generation of markdown documents and edits for tests.
//!
//! Deterministic from a seed, so failures replay exactly. Documents mix every
//! construct the highlighter and renderer understand, including malformed and
//! pathological nesting.

/// xorshift64* — small, fast, and plenty for test generation.
pub(crate) struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }

    pub fn chance(&mut self, p: f64) -> bool {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64 <= p
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    pub fn f32_in(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * ((self.next() >> 40) as f32 / (1u64 << 24) as f32)
    }
}

const WORDS: &[&str] = &[
    "the",
    "quick",
    "brown",
    "fox",
    "jumps",
    "over",
    "a",
    "lazy",
    "dog",
    "I",
    "editor",
    "markdown",
    "renderer",
    "well-known",
    "state-of-the-art",
    "don't",
    "it's",
    "typography",
    "kerning",
    "AVAVA",
    "Wolf",
    "office",
    "fi",
    "—",
    "“quoted”",
    "(aside)",
    "e.g.",
    "3.14",
    "2026",
    "x",
    "supercalifragilisticexpialidocious",
    "https://example.com/a/very/long/path/that/keeps/going/without/any/spaces/at/all",
];

pub(crate) fn words(rng: &mut Rng, max: usize) -> String {
    let n = 1 + rng.below(max);
    (0..n)
        .map(|_| *rng.pick(WORDS))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn inline(rng: &mut Rng) -> String {
    let parts = 1 + rng.below(5);
    let mut out = Vec::new();
    for _ in 0..parts {
        let chunk = match rng.below(9) {
            0 => format!("**{}**", words(rng, 3)),
            1 => format!("*{}*", words(rng, 3)),
            2 => format!("`{}`", words(rng, 2)),
            3 => format!("[{}](http://x.y/{})", words(rng, 2), rng.below(99)),
            4 => format!("[[Note {}|{}]]", rng.below(9), words(rng, 2)),
            5 => rng.pick(&["$a^2$", "$x$", "$\\uncached$"]).to_string(),
            _ => words(rng, 12),
        };
        out.push(chunk);
    }
    out.join(" ")
}

pub(crate) fn block(rng: &mut Rng, out: &mut Vec<String>) {
    match rng.below(18) {
        // Constructs that stress block state: unusual nesting and endings.
        14 => {
            out.push("$$".into());
            out.push(
                rng.pick(&["```", "# heading ends math", "| a | b |", "- item"])
                    .to_string(),
            );
            out.push(words(rng, 3));
            if rng.chance(0.5) {
                out.push("$$".into());
            }
        }
        15 => {
            out.push("\\begin{align}".into());
            out.push("a &= b \\\\".into());
            out.push(rng.pick(&["\\end{align}", "$$", "\\end{note}"]).to_string());
        }
        16 => {
            out.push(format!("```{}", rng.pick(&["rust", ""])));
            out.push("// a comment".into());
            out.push("/* open".into());
            if rng.chance(0.5) {
                out.push("```".into());
            }
        }
        17 => out.push(format!("{}\r", inline(rng))),
        0 => out.push(format!("{} {}", "#".repeat(1 + rng.below(6)), inline(rng))),
        1 => out.push(format!("- {}", inline(rng))),
        2 => out.push(format!("{}. {}", 1 + rng.below(20), inline(rng))),
        3 => out.push(format!(
            "- [{}] {}",
            if rng.chance(0.5) { " " } else { "x" },
            inline(rng)
        )),
        4 => out.push(format!("> {}", inline(rng))),
        5 => out.push(String::new()),
        6 => out.push("---".into()),
        7 => {
            out.push(format!("```{}", rng.pick(&["", "rust", "js"])));
            for _ in 0..1 + rng.below(3) {
                let long = if rng.chance(0.3) {
                    "x".repeat(40 + rng.below(120))
                } else {
                    String::new()
                };
                out.push(format!(
                    "let {} = {};{long}",
                    rng.pick(WORDS),
                    rng.below(1000)
                ));
            }
            out.push("```".into());
        }
        8 => {
            out.push("$$".into());
            out.push(
                rng.pick(&["E = mc^2", "\\wide", "\\uncached + \\alpha^2"])
                    .to_string(),
            );
            out.push("$$".into());
        }
        9 => out.push(rng.pick(&["$$x^2$$", "$$E = mc^2$$"]).to_string()),
        10 => {
            let cols = 1 + rng.below(6);
            let cell = |rng: &mut Rng| {
                if rng.chance(0.4) {
                    ((b'a' + rng.below(26) as u8) as char).to_string()
                } else {
                    words(rng, 3)
                }
            };
            let row = |rng: &mut Rng| {
                let cells: Vec<String> = (0..cols).map(|_| cell(rng)).collect();
                format!("| {} |", cells.join(" | "))
            };
            out.push(row(rng));
            out.push(format!("|{}", "---|".repeat(cols)));
            for _ in 0..rng.below(4) {
                out.push(row(rng));
            }
        }
        11 => out.push(format!("![{}](img/i.png)", words(rng, 2))),
        _ => out.push(inline(rng)),
    }
}

pub(crate) fn document(rng: &mut Rng) -> String {
    let mut lines = Vec::new();
    for _ in 0..1 + rng.below(9) {
        block(rng, &mut lines);
    }
    lines.join("\n")
}
