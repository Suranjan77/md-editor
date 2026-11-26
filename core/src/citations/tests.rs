//! Unit tests for BibTeX and PDF citation parsing and pairing.

use super::*;

#[test]
fn parses_nested_braces_quotes_and_formats_authors_and_year() {
    let bib = r#"
@comment{ignore me}
@string{neurips = "NeurIPS"}

@inproceedings{vaswani2017,
  title = {{Attention} Is All You Need},
  author = {Vaswani, Ashish and Shazeer, Noam and Parmar, Niki},
  year = {2017},
}

@article{kingma2014,
  title = "Adam: {A} Method for Stochastic Optimization",
  author = {Diederik P. Kingma and Jimmy Ba},
  date = {2014-12-22},
}

@book{knuth1984,
  title = {The {\TeX}book},
  author = {Knuth, Donald E.},
  year = 1984
}
"#;

    let parsed = parse_bibtex(bib);
    assert_eq!(parsed.len(), 3);

    assert_eq!(parsed[0].key, "vaswani2017");
    assert_eq!(parsed[0].title, "Attention Is All You Need");
    assert_eq!(parsed[0].author, "Vaswani et al.");
    assert_eq!(parsed[0].year.as_deref(), Some("2017"));
    assert_eq!(parsed[0].display, "Vaswani et al. (2017)");

    assert_eq!(parsed[1].key, "kingma2014");
    assert_eq!(parsed[1].author, "Kingma & Ba");
    assert_eq!(parsed[1].year.as_deref(), Some("2014"));
    assert_eq!(parsed[1].display, "Kingma & Ba (2014)");

    assert_eq!(parsed[2].key, "knuth1984");
    assert_eq!(parsed[2].author, "Knuth");
    assert_eq!(parsed[2].year.as_deref(), Some("1984"));
    assert_eq!(parsed[2].display, "Knuth (1984)");
}

#[test]
fn pairs_bib_entries_with_pdfs_and_synthesizes_standalone_pdfs() {
    let bib = r#"
@article{vaswani2017,
  title = {Attention Is All You Need},
  author = {Vaswani, Ashish and Shazeer, Noam},
  year = {2017},
}
@article{devlin2018,
  title = {BERT},
  author = {Devlin, Jacob and Chang, Ming-Wei and Lee, Kenton},
  year = {2018},
}
@misc{webOnly,
  title = {No PDF Entry},
  author = {Doe, Jane},
  year = {2021},
}
"#;

    let pdfs = vec![
        "papers/vaswani2017.pdf".to_string(),
        "papers/Devlin - 2018 - BERT.pdf".to_string(),
        "papers/Sutton - 2018 - Reinforcement Learning.pdf".to_string(),
        "papers/lecun1998.pdf".to_string(),
    ];

    let entries = collect_citations_from_vault(&[("refs.bib".into(), bib.into())], &pdfs);
    let find = |key: &str| entries.iter().find(|e| e.key == key).unwrap();

    assert_eq!(
        find("vaswani2017").pdf_path.as_deref(),
        Some("papers/vaswani2017.pdf")
    );
    assert_eq!(
        find("devlin2018").pdf_path.as_deref(),
        Some("papers/Devlin - 2018 - BERT.pdf")
    );
    assert_eq!(find("webOnly").pdf_path, None);

    let sutton = find("Sutton - 2018 - Reinforcement Learning");
    assert_eq!(sutton.display, "Sutton (2018)");
    assert_eq!(sutton.title, "Reinforcement Learning");
    assert_eq!(
        sutton.pdf_path.as_deref(),
        Some("papers/Sutton - 2018 - Reinforcement Learning.pdf")
    );

    let lecun = find("lecun1998");
    assert_eq!(lecun.display, "Lecun (1998)");
}

#[test]
fn handles_multibyte_utf8_authors_without_panic() {
    let bib = "@article{utf8,\n  title = {Multi Byte},\n  author = {Max ABücher and 張三 and Søren Kierkegaard},\n  year = {2023}\n}\n";
    let parsed = parse_bibtex(bib);
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].author, "ABücher et al.");
    assert_eq!(parsed[0].display, "ABücher et al. (2023)");
}
