//! End-to-end scenarios. Each boots the real app on a fresh vault, drives it,
//! checks the outcome on disk and on screen, and leaves screenshots and
//! telemetry in `target/e2e/`.

use super::{Vault, boot};

/// A small but varied vault: linked notes, every block type, a PDF, a deck.
pub(crate) fn research_vault() -> Vault {
    Vault::new("research")
        .file(
            "Index.md",
            "# Research Index\n\nStart at [[Attention]] and [[Optimizers]].\n\n- [ ] read the Adam paper\n- [x] set up the vault\n\n| Topic | Status |\n|---|---|\n| Attention | drafting |\n| Optimizers | reading |\n",
        )
        .file(
            "notes/Attention.md",
            "# Attention\n\nScaled dot-product attention:\n\n$$\n\\mathrm{softmax}\\left(\\frac{QK^T}{\\sqrt{d_k}}\\right)V\n$$\n\n```python\ndef attention(q, k, v):\n    return softmax(q @ k.T / sqrt(d)) @ v\n```\n\n> Attention is all you need.\n\nBack to [[Index]].\n",
        )
        .file(
            "notes/Optimizers.md",
            "# Optimizers\n\nAdam keeps moving averages of the gradient ($m_t$) and its square ($v_t$).\n\nSee [[Attention]] and the paper in `papers/`.\n",
        )
        .fixture("papers/dummy.pdf", "pdf/dummy.pdf")
        .fixture("slides/deck.pptx", "pptx/fixture.pptx")
}

#[test]
fn smoke_boots_and_renders_a_note() {
    let vault = research_vault();
    let mut app = boot("smoke", &vault, Some("Index.md"));
    app.screenshot("index");
    assert!(
        app.shows("Index.md"),
        "the open note should be named somewhere in the UI"
    );
    let state = app.finish();
    assert_eq!(state.active_path.as_deref(), Some("Index.md"));
}
