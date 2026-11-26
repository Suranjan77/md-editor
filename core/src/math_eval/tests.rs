//! Unit tests for the headless safe math evaluator.

use super::*;

#[test]
fn evaluates_arithmetic_precedence_and_formats_cleanly() {
    assert_eq!(evaluate_formatted("0.1 + 0.2").unwrap(), "0.3");
    assert_eq!(evaluate_formatted("2 ^ 10").unwrap(), "1024");
    assert_eq!(evaluate_formatted("2 ^ 3 ^ 2").unwrap(), "512");
    assert_eq!(evaluate_formatted("-3^2 + 10").unwrap(), "1");
    assert_eq!(evaluate_formatted("17 % 5").unwrap(), "2");
}

#[test]
fn evaluates_latex_constructs_and_functions() {
    assert_eq!(evaluate_formatted(r"\frac{1 + \sqrt{9}}{2}").unwrap(), "2");
    assert_eq!(evaluate_formatted(r"\sin(\pi / 2)").unwrap(), "1");
    assert_eq!(evaluate_formatted(r"2 \cdot 3 \times 4").unwrap(), "24");
    assert_eq!(
        evaluate_formatted(r"\left(10 + \frac{20}{4}\right)").unwrap(),
        "15"
    );
    assert_eq!(evaluate_formatted("log(100) + log2(8)").unwrap(), "5");
    assert_eq!(evaluate_formatted("ln(e^3)").unwrap(), "3");
    assert_eq!(evaluate_formatted("min(max(3, 9), 7)").unwrap(), "7");
}

#[test]
fn rejects_division_by_zero_domain_errors_and_malformed_input() {
    assert!(evaluate("1 / 0").is_err());
    assert!(evaluate("1 % 0").is_err());
    assert!(evaluate("sqrt(-1)").is_err());
    assert!(evaluate("ln(0)").is_err());
    assert!(evaluate("").is_err());
    assert!(evaluate(r"\frac{1}").is_err());
    assert!(evaluate("unknown_var + 1").is_err());
    assert!(evaluate("1 + * 2").is_err());
}
