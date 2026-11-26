//! Tests for `study_context`.

use super::*;

fn note(path: &str, words: usize) -> Observation {
    Observation {
        focused: Some(path.to_string()),
        note: Some((path.to_string(), words)),
        pdf: None,
    }
}

fn reading(
    pdf: &str,
    page: u16,
    highlights: usize,
    open_note: Option<(&str, usize)>,
) -> Observation {
    Observation {
        focused: Some(pdf.to_string()),
        note: open_note.map(|(p, w)| (p.to_string(), w)),
        pdf: Some((pdf.to_string(), page, Some(highlights))),
    }
}

#[test]
fn a_session_names_what_it_worked_on_most_first() {
    let t0 = Instant::now();
    let at = |mins: u64| t0 + Duration::from_secs(mins * 60);
    let mut session = SessionContext::start(t0, &reading("papers/Vaswani2017.pdf", 2, 5, None));
    session.observe(at(4), &reading("papers/Vaswani2017.pdf", 5, 6, None));
    session.observe(
        at(10),
        &reading(
            "papers/Vaswani2017.pdf",
            7,
            8,
            Some(("notes/attention.md", 100)),
        ),
    );
    // Writing beside it in split view.
    session.observe(at(15), &note("notes/attention.md", 300));
    session.observe(at(20), &note("notes/attention.md", 510));
    // A glance at another note: too short to name.
    session.observe(at(20) + Duration::from_secs(10), &note("Index.md", 40));
    session.observe(
        at(20) + Duration::from_secs(20),
        &reading("papers/Vaswani2017.pdf", 7, 8, None),
    );

    assert_eq!(
        session.finish(at(25)).as_deref(),
        Some("Vaswani2017.pdf p. 3–8, +3 highlights · attention.md, +410 words")
    );
}

#[test]
fn time_goes_to_the_document_in_front_and_short_untouched_glances_are_dropped() {
    let t0 = Instant::now();
    let secs = |s: u64| t0 + Duration::from_secs(s);
    let mut session = SessionContext::start(t0, &note("a.md", 10));
    session.observe(secs(100), &note("b.md", 10));
    session.observe(secs(110), &note("c.md", 10));
    session.observe(secs(400), &Observation::default());
    // a: 100 s, b: 10 s (untouched: dropped), c: 290 s.
    assert_eq!(session.finish(secs(900)).as_deref(), Some("c.md · a.md"));

    let mut idle = SessionContext::start(t0, &Observation::default());
    idle.observe(secs(600), &Observation::default());
    assert_eq!(idle.finish(secs(700)), None, "nothing open: nothing to say");
}

#[test]
fn a_short_visit_that_changed_something_is_kept_and_extra_documents_are_counted() {
    let t0 = Instant::now();
    let secs = |s: u64| t0 + Duration::from_secs(s);
    let mut session = SessionContext::start(t0, &note("quick.md", 50));
    session.observe(secs(5), &note("quick.md", 40));
    for (i, name) in ["a.md", "b.md", "c.md", "d.md"].iter().enumerate() {
        session.observe(secs(10 + i as u64 * 60), &note(name, 1));
    }
    assert_eq!(
        session.finish(secs(250)).as_deref(),
        Some("a.md · b.md · c.md · +2 more"),
        "quick.md lost 10 words in 10 s: kept, but last by time"
    );
}

#[test]
fn highlights_loading_with_a_pdf_are_not_counted_as_made() {
    let t0 = Instant::now();
    let secs = |s: u64| t0 + Duration::from_secs(s);
    let opening = Observation {
        focused: Some("p.pdf".into()),
        note: None,
        pdf: Some(("p.pdf".into(), 0, None)),
    };
    let mut session = SessionContext::start(t0, &opening);
    session.observe(secs(1), &reading("p.pdf", 0, 12, None));
    session.observe(secs(60), &reading("p.pdf", 1, 13, None));
    assert_eq!(
        session.finish(secs(120)).as_deref(),
        Some("p.pdf p. 1–2, +1 highlight")
    );
}
