//! Headless recursive-descent mathematical expression evaluator (`Evaluate Math`).

#[cfg(test)]
mod tests;

/// Evaluate a mathematical expression (supporting standard math and common LaTeX
/// constructs like `\frac{a}{b}`, `\sqrt{x}`, `\pi`, `\cdot`, `\times`).
pub fn evaluate(expr: &str) -> Result<f64, String> {
    let preprocessed = preprocess_latex(expr)?;
    let tokens = tokenize(&preprocessed)?;
    if tokens.is_empty() {
        return Err("Empty math expression".to_string());
    }
    let mut parser = Parser {
        tokens: &tokens,
        pos: 0,
    };
    let value = parser.parse_expr()?;
    if parser.pos < parser.tokens.len() {
        return Err(format!("Unexpected token {:?}", parser.tokens[parser.pos]));
    }
    if !value.is_finite() {
        return Err("Math result is not finite".to_string());
    }
    Ok(value)
}

/// Evaluate `expr` and format the result cleanly for insertion into Markdown.
pub fn evaluate_formatted(expr: &str) -> Result<String, String> {
    evaluate(expr).map(format_number)
}

/// Format a floating-point number without noisy IEEE-754 trailing artifacts.
pub fn format_number(value: f64) -> String {
    let value = if value.abs() < 1e-12 { 0.0 } else { value };
    if value.abs() <= 1e15 && (value - value.round()).abs() < 1e-11 {
        let rounded = value.round() as i64;
        return rounded.to_string();
    }
    let formatted = format!("{value:.10}");
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    if trimmed == "-0" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn preprocess_latex(raw: &str) -> Result<String, String> {
    let mut s = raw
        .replace("\\cdot", " * ")
        .replace("\\times", " * ")
        .replace("\\div", " / ")
        .replace("\\left(", "(")
        .replace("\\right)", ")")
        .replace("\\left[", "(")
        .replace("\\right]", ")")
        .replace("\\left\\{", "(")
        .replace("\\right\\}", ")")
        .replace("\\,", " ")
        .replace("\\;", " ")
        .replace("\\ ", " ")
        .replace("\\pi", "pi")
        .replace("\\tau", "tau");

    for fn_name in [
        "sin", "cos", "tan", "asin", "acos", "atan", "ln", "log10", "log2", "log", "exp", "abs",
        "floor", "ceil", "round", "min", "max",
    ] {
        s = s.replace(&format!("\\{fn_name}"), fn_name);
    }

    // Repeatedly rewrite `\frac{a}{b}` -> `((a)/(b))` and `\sqrt{x}` -> `sqrt(x)`.
    for _ in 0..64 {
        if let Some(pos) = s.find("\\frac") {
            let after = skip_ws_idx(&s, pos + 5);
            let Some((num, after_num)) = read_brace_group(&s, after) else {
                return Err("Malformed \\frac numerator".to_string());
            };
            let after_num_ws = skip_ws_idx(&s, after_num);
            let Some((den, after_den)) = read_brace_group(&s, after_num_ws) else {
                return Err("Malformed \\frac denominator".to_string());
            };
            let mut next = String::with_capacity(s.len() + 6);
            next.push_str(&s[..pos]);
            next.push_str("((");
            next.push_str(num);
            next.push_str(")/(");
            next.push_str(den);
            next.push_str("))");
            next.push_str(&s[after_den..]);
            s = next;
            continue;
        }
        if let Some(pos) = s.find("\\sqrt") {
            let after = skip_ws_idx(&s, pos + 5);
            if let Some((inner, after_inner)) = read_brace_group(&s, after) {
                let mut next = String::with_capacity(s.len() + 4);
                next.push_str(&s[..pos]);
                next.push_str("sqrt(");
                next.push_str(inner);
                next.push(')');
                next.push_str(&s[after_inner..]);
                s = next;
                continue;
            }
            s = format!("{}sqrt{}", &s[..pos], &s[pos + 5..]);
            continue;
        }
        break;
    }

    // Map remaining `{...}` grouping braces to `(...)`.
    Ok(s.replace('{', "(").replace('}', ")"))
}

fn skip_ws_idx(s: &str, mut i: usize) -> usize {
    let bytes = s.as_bytes();
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn read_brace_group(s: &str, start: usize) -> Option<(&str, usize)> {
    let bytes = s.as_bytes();
    if bytes.get(start).copied() != Some(b'{') {
        return None;
    }
    let mut depth = 1usize;
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[start + 1..i], i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    LParen,
    RParen,
    Comma,
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        let ch = input[i..].chars().next().unwrap_or_default();
        if ch.is_ascii_whitespace() {
            i += ch.len_utf8();
            continue;
        }
        if ch.is_ascii_digit() || (ch == '.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            let start = i;
            let mut seen_dot = false;
            while i < bytes.len() {
                let b = bytes[i];
                if b.is_ascii_digit() {
                    i += 1;
                } else if b == b'.' && !seen_dot {
                    seen_dot = true;
                    i += 1;
                } else if (b == b'e' || b == b'E')
                    && bytes
                        .get(i + 1)
                        .is_some_and(|n| n.is_ascii_digit() || *n == b'+' || *n == b'-')
                {
                    i += 1;
                    if matches!(bytes.get(i).copied(), Some(b'+' | b'-')) {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            let num: f64 = input[start..i]
                .parse()
                .map_err(|_| format!("Invalid number '{}'", &input[start..i]))?;
            tokens.push(Token::Number(num));
            continue;
        }
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            tokens.push(Token::Ident(input[start..i].to_ascii_lowercase()));
            continue;
        }
        let tok = match ch {
            '+' => Token::Plus,
            '-' => Token::Minus,
            '*' => Token::Star,
            '/' => Token::Slash,
            '%' => Token::Percent,
            '^' => Token::Caret,
            '(' => Token::LParen,
            ')' => Token::RParen,
            ',' => Token::Comma,
            'π' => Token::Ident("pi".to_string()),
            'τ' => Token::Ident("tau".to_string()),
            other => {
                return Err(format!(
                    "Unsupported character '{other}' in math expression"
                ));
            }
        };
        tokens.push(tok);
        i += ch.len_utf8();
    }
    Ok(tokens)
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<&Token> {
        let tok = self.tokens.get(self.pos);
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    fn parse_expr(&mut self) -> Result<f64, String> {
        self.parse_add_sub()
    }

    fn parse_add_sub(&mut self) -> Result<f64, String> {
        let mut lhs = self.parse_mul_div()?;
        while let Some(tok) = self.peek() {
            match tok {
                Token::Plus => {
                    self.next();
                    lhs += self.parse_mul_div()?;
                }
                Token::Minus => {
                    self.next();
                    lhs -= self.parse_mul_div()?;
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_mul_div(&mut self) -> Result<f64, String> {
        let mut lhs = self.parse_unary()?;
        while let Some(tok) = self.peek() {
            match tok {
                Token::Star => {
                    self.next();
                    lhs *= self.parse_unary()?;
                }
                Token::Slash => {
                    self.next();
                    let rhs = self.parse_unary()?;
                    if rhs == 0.0 {
                        return Err("Division by zero".to_string());
                    }
                    lhs /= rhs;
                }
                Token::Percent => {
                    self.next();
                    let rhs = self.parse_unary()?;
                    if rhs == 0.0 {
                        return Err("Division by zero".to_string());
                    }
                    lhs %= rhs;
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some(Token::Plus) => {
                self.next();
                self.parse_unary()
            }
            Some(Token::Minus) => {
                self.next();
                Ok(-self.parse_unary()?)
            }
            _ => self.parse_power(),
        }
    }

    fn parse_power(&mut self) -> Result<f64, String> {
        let base = self.parse_primary()?;
        if matches!(self.peek(), Some(Token::Caret)) {
            self.next();
            let exp = self.parse_unary()?;
            Ok(base.powf(exp))
        } else {
            Ok(base)
        }
    }

    fn parse_primary(&mut self) -> Result<f64, String> {
        match self.next().cloned() {
            Some(Token::Number(n)) => Ok(n),
            Some(Token::LParen) => {
                let val = self.parse_expr()?;
                if !matches!(self.next(), Some(Token::RParen)) {
                    return Err("Missing closing ')'".to_string());
                }
                Ok(val)
            }
            Some(Token::Ident(name)) => {
                if matches!(self.peek(), Some(Token::LParen)) {
                    self.next();
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Token::RParen)) {
                        loop {
                            args.push(self.parse_expr()?);
                            match self.peek() {
                                Some(Token::Comma) => {
                                    self.next();
                                }
                                Some(Token::RParen) => break,
                                _ => return Err(format!("Expected ')' after {name}(...)")),
                            }
                        }
                    }
                    self.next();
                    eval_function(&name, &args)
                } else {
                    match name.as_str() {
                        "pi" => Ok(std::f64::consts::PI),
                        "e" => Ok(std::f64::consts::E),
                        "tau" => Ok(std::f64::consts::TAU),
                        _ => Err(format!("Unknown identifier '{name}'")),
                    }
                }
            }
            other => Err(format!("Unexpected token {other:?}")),
        }
    }
}

fn eval_function(name: &str, args: &[f64]) -> Result<f64, String> {
    let require_one = || -> Result<f64, String> {
        if args.len() != 1 {
            return Err(format!("{name} expects 1 argument"));
        }
        Ok(args[0])
    };
    let require_two = || -> Result<(f64, f64), String> {
        if args.len() != 2 {
            return Err(format!("{name} expects 2 arguments"));
        }
        Ok((args[0], args[1]))
    };

    let result = match name {
        "sin" => require_one()?.sin(),
        "cos" => require_one()?.cos(),
        "tan" => require_one()?.tan(),
        "asin" => require_one()?.asin(),
        "acos" => require_one()?.acos(),
        "atan" => require_one()?.atan(),
        "ln" => {
            let x = require_one()?;
            if x <= 0.0 {
                return Err("ln domain error (must be > 0)".to_string());
            }
            x.ln()
        }
        // `log` is base-10; use `ln` for natural logarithm.
        "log" | "log10" => {
            let x = require_one()?;
            if x <= 0.0 {
                return Err(format!("{name} domain error (must be > 0)"));
            }
            x.log10()
        }
        "log2" => {
            let x = require_one()?;
            if x <= 0.0 {
                return Err("log2 domain error (must be > 0)".to_string());
            }
            x.log2()
        }
        "exp" => require_one()?.exp(),
        "sqrt" => {
            let x = require_one()?;
            if x < 0.0 {
                return Err("sqrt domain error (must be >= 0)".to_string());
            }
            x.sqrt()
        }
        "abs" => require_one()?.abs(),
        "floor" => require_one()?.floor(),
        "ceil" => require_one()?.ceil(),
        "round" => require_one()?.round(),
        "min" => {
            let (a, b) = require_two()?;
            a.min(b)
        }
        "max" => {
            let (a, b) = require_two()?;
            a.max(b)
        }
        _ => return Err(format!("Unknown function '{name}'")),
    };
    if !result.is_finite() {
        return Err(format!("{name} produced non-finite result"));
    }
    Ok(result)
}
