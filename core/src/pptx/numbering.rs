//! Automatic bullet numbering schemes.

// ── Numbering ────────────────────────────────────────────────────────

pub(super) fn roman(mut n: u32) -> String {
    const NUMERALS: [(u32, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (value, numeral) in NUMERALS {
        while n >= value {
            out.push_str(numeral);
            n -= value;
        }
    }
    out
}

/// PowerPoint's alphabetic numbering repeats the letter: …, y, z, aa, bb.
pub(super) fn alphabetic(n: u32) -> String {
    let n = n.max(1) - 1;
    let letter = (b'a' + (n % 26) as u8) as char;
    std::iter::repeat_n(letter, (n / 26) as usize + 1).collect()
}

/// The label for item `n` of an auto-numbered list in `scheme`
/// (`arabicPeriod`, `romanUcParenR`, …).
pub(super) fn autonumber_label(scheme: &str, n: u32) -> String {
    let (number, punctuation) = if let Some(rest) = scheme.strip_prefix("romanUc") {
        (roman(n).to_uppercase(), rest)
    } else if let Some(rest) = scheme.strip_prefix("romanLc") {
        (roman(n), rest)
    } else if let Some(rest) = scheme.strip_prefix("alphaUc") {
        (alphabetic(n).to_uppercase(), rest)
    } else if let Some(rest) = scheme.strip_prefix("alphaLc") {
        (alphabetic(n), rest)
    } else {
        (
            n.to_string(),
            scheme.strip_prefix("arabic").unwrap_or("Period"),
        )
    };
    if punctuation.ends_with("ParenBoth") {
        format!("({number})")
    } else if punctuation.ends_with("ParenR") {
        format!("{number})")
    } else if punctuation.ends_with("Plain") {
        number
    } else if punctuation.ends_with("Minus") {
        format!("- {number} -")
    } else {
        format!("{number}.")
    }
}
