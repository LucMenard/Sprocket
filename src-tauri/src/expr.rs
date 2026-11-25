//! The small expression language of conditions, loops and `{{templates}}`.
//!
//! - paths: `steps.triage.output.severity`, a loop's item `file` or `file.path`, `.0` for a list's first entry
//! - literals: `"text"`, `'text'`, `12`, `1.5`, `true`, `false`, `null`
//! - comparisons: `==`, `!=`, `<`, `<=`, `>`, `>=`, and `contains` (text in text, or a value in a list)
//! - `and`, `or`, `not`, parentheses
//!
//! Something missing is `null`; `null`, `false`, `0`, `""` and empty lists and objects count as false.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Path(Vec<String>),
    Literal(Value),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Compare(Op, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Contains,
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Str(String),
    Num(f64),
    Op(Op),
    Dot,
    Open,
    Close,
    And,
    Or,
    Not,
}

fn tokens(text: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '(' => (out.push(Token::Open), i += 1).1,
            ')' => (out.push(Token::Close), i += 1).1,
            '.' => (out.push(Token::Dot), i += 1).1,
            '=' if next == Some('=') => (out.push(Token::Op(Op::Eq)), i += 2).1,
            '!' if next == Some('=') => (out.push(Token::Op(Op::Ne)), i += 2).1,
            '<' if next == Some('=') => (out.push(Token::Op(Op::Le)), i += 2).1,
            '>' if next == Some('=') => (out.push(Token::Op(Op::Ge)), i += 2).1,
            '<' => (out.push(Token::Op(Op::Lt)), i += 1).1,
            '>' => (out.push(Token::Op(Op::Gt)), i += 1).1,
            '&' if next == Some('&') => (out.push(Token::And), i += 2).1,
            '|' if next == Some('|') => (out.push(Token::Or), i += 2).1,
            '!' => (out.push(Token::Not), i += 1).1,
            '"' | '\'' => {
                let mut s = String::new();
                i += 1;
                loop {
                    match chars.get(i) {
                        None => return Err("A text in quotes isn't closed".into()),
                        Some(&q) if q == c => break,
                        Some('\\') if chars.get(i + 1).is_some() => {
                            s.push(chars[i + 1]);
                            i += 2;
                        }
                        Some(&ch) => {
                            s.push(ch);
                            i += 1;
                        }
                    }
                }
                i += 1;
                out.push(Token::Str(s));
            }
            // A number, unless it follows a dot: `list.0` is a path
            c if (c.is_ascii_digit() || (c == '-' && next.is_some_and(|n| n.is_ascii_digit())))
                && out.last() != Some(&Token::Dot) =>
            {
                let start = i;
                i += 1;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                out.push(Token::Num(s.parse().map_err(|_| format!("`{s}` isn't a number"))?));
            }
            c if c.is_alphanumeric() || c == '_' || c == '-' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '-') {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                out.push(match word.as_str() {
                    "and" => Token::And,
                    "or" => Token::Or,
                    "not" => Token::Not,
                    "contains" => Token::Op(Op::Contains),
                    _ => Token::Ident(word),
                });
            }
            other => return Err(format!("Unexpected `{other}`")),
        }
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.at).cloned();
        self.at += 1;
        t
    }

    fn or(&mut self) -> Result<Expr, String> {
        let mut left = self.and()?;
        while self.peek() == Some(&Token::Or) {
            self.at += 1;
            left = Expr::Or(Box::new(left), Box::new(self.and()?));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Expr, String> {
        let mut left = self.not()?;
        while self.peek() == Some(&Token::And) {
            self.at += 1;
            left = Expr::And(Box::new(left), Box::new(self.not()?));
        }
        Ok(left)
    }

    fn not(&mut self) -> Result<Expr, String> {
        if self.peek() == Some(&Token::Not) {
            self.at += 1;
            return Ok(Expr::Not(Box::new(self.not()?)));
        }
        self.compare()
    }

    fn compare(&mut self) -> Result<Expr, String> {
        let left = self.atom()?;
        if let Some(Token::Op(op)) = self.peek().cloned() {
            self.at += 1;
            return Ok(Expr::Compare(op, Box::new(left), Box::new(self.atom()?)));
        }
        Ok(left)
    }

    fn atom(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Token::Open) => {
                let e = self.or()?;
                match self.next() {
                    Some(Token::Close) => Ok(e),
                    _ => Err("A `(` isn't closed".into()),
                }
            }
            Some(Token::Str(s)) => Ok(Expr::Literal(Value::String(s))),
            Some(Token::Num(n)) => Ok(Expr::Literal(serde_json::json!(n))),
            Some(Token::Ident(word)) => match word.as_str() {
                "true" => Ok(Expr::Literal(Value::Bool(true))),
                "false" => Ok(Expr::Literal(Value::Bool(false))),
                "null" => Ok(Expr::Literal(Value::Null)),
                _ => {
                    let mut path = vec![word];
                    while self.peek() == Some(&Token::Dot) {
                        self.at += 1;
                        match self.next() {
                            Some(Token::Ident(part)) => path.push(part),
                            Some(Token::Num(n)) if n >= 0.0 && n.fract() == 0.0 => path.push((n as u64).to_string()),
                            _ => return Err(format!("`{}.` needs a name after the dot", path.join("."))),
                        }
                    }
                    Ok(Expr::Path(path))
                }
            },
            Some(t) => Err(format!("Unexpected {}", describe(&t))),
            None => Err("The expression ends too early".into()),
        }
    }
}

fn describe(t: &Token) -> String {
    match t {
        Token::Close => "`)`".into(),
        Token::Op(_) => "comparison".into(),
        Token::Dot => "`.`".into(),
        Token::And => "`and`".into(),
        Token::Or => "`or`".into(),
        _ => "word".into(),
    }
}

pub fn parse(text: &str) -> Result<Expr, String> {
    let mut p = Parser { tokens: tokens(text)?, at: 0 };
    if p.tokens.is_empty() {
        return Err("The expression is empty".into());
    }
    let e = p.or()?;
    match p.peek() {
        None => Ok(e),
        Some(t) => Err(format!("Unexpected {} after `{}`", describe(t), text.trim())),
    }
}

impl Expr {
    /// Every path the expression reads.
    pub fn paths(&self) -> Vec<&[String]> {
        let mut out = Vec::new();
        self.walk(&mut out);
        out
    }

    fn walk<'a>(&'a self, out: &mut Vec<&'a [String]>) {
        match self {
            Expr::Path(p) => out.push(p),
            Expr::Literal(_) => {}
            Expr::Not(e) => e.walk(out),
            Expr::And(a, b) | Expr::Or(a, b) | Expr::Compare(_, a, b) => {
                a.walk(out);
                b.walk(out);
            }
        }
    }

    /// The value, with `lookup` giving each path's (null when missing).
    pub fn eval(&self, lookup: &dyn Fn(&[String]) -> Value) -> Value {
        match self {
            Expr::Path(p) => lookup(p),
            Expr::Literal(v) => v.clone(),
            Expr::Not(e) => Value::Bool(!truthy(&e.eval(lookup))),
            Expr::And(a, b) => Value::Bool(truthy(&a.eval(lookup)) && truthy(&b.eval(lookup))),
            Expr::Or(a, b) => Value::Bool(truthy(&a.eval(lookup)) || truthy(&b.eval(lookup))),
            Expr::Compare(op, a, b) => Value::Bool(compare(*op, &a.eval(lookup), &b.eval(lookup))),
        }
    }
}

pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Numbers compare as numbers, also when one side is text that reads as a number (`"3" < 10`).
fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn equal(a: &Value, b: &Value) -> bool {
    match (number(a), number(b)) {
        (Some(x), Some(y)) if a.is_number() || b.is_number() => x == y,
        _ => match (a, b) {
            // Text compares without case or surrounding spaces: agents don't write `High` and `high` alike
            (Value::String(x), Value::String(y)) => x.trim().eq_ignore_ascii_case(y.trim()),
            _ => a == b,
        },
    }
}

fn compare(op: Op, a: &Value, b: &Value) -> bool {
    match op {
        Op::Eq => equal(a, b),
        Op::Ne => !equal(a, b),
        Op::Contains => match a {
            Value::String(s) => b.as_str().is_some_and(|t| s.to_lowercase().contains(&t.to_lowercase())),
            Value::Array(items) => items.iter().any(|i| equal(i, b)),
            Value::Object(o) => b.as_str().is_some_and(|k| o.contains_key(k)),
            _ => false,
        },
        Op::Lt | Op::Le | Op::Gt | Op::Ge => {
            let ordering = match (number(a), number(b)) {
                (Some(x), Some(y)) => x.partial_cmp(&y),
                _ => match (a.as_str(), b.as_str()) {
                    (Some(x), Some(y)) => Some(x.cmp(y)),
                    _ => None,
                },
            };
            ordering.is_some_and(|o| match op {
                Op::Lt => o.is_lt(),
                Op::Le => o.is_le(),
                Op::Gt => o.is_gt(),
                _ => o.is_ge(),
            })
        }
    }
}

/// Follows `rest` into a value: object fields, and list entries by number.
pub fn dig(mut v: Value, rest: &[String]) -> Value {
    for part in rest {
        v = match v {
            Value::Object(mut o) => o.remove(part).unwrap_or(Value::Null),
            Value::Array(mut a) => match part.parse::<usize>() {
                Ok(i) if i < a.len() => a.swap_remove(i),
                _ => Value::Null,
            },
            _ => Value::Null,
        };
    }
    v
}

/// A value as text in a prompt or a message: text as it is, the rest as JSON.
pub fn to_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

/// The `{{ … }}` parts of a text, parsed, with their place.
pub fn template_parts(text: &str) -> Result<Vec<(std::ops::Range<usize>, Expr)>, String> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(start) = text[from..].find("{{").map(|i| from + i) {
        let end = text[start..].find("}}").map(|i| start + i).ok_or("A `{{` isn't closed by `}}`")?;
        out.push((start..end + 2, parse(&text[start + 2..end])?));
        from = end + 2;
    }
    Ok(out)
}

/// The text with each `{{ expression }}` replaced by its value.
pub fn render(text: &str, lookup: &dyn Fn(&[String]) -> Value) -> Result<String, String> {
    let mut out = String::new();
    let mut last = 0;
    for (range, e) in template_parts(text)? {
        out.push_str(&text[last..range.start]);
        out.push_str(&to_text(&e.eval(lookup)));
        last = range.end;
    }
    out.push_str(&text[last..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn data() -> Value {
        json!({
            "steps": { "triage": { "output": { "severity": "High", "files": ["a.rs", "b.rs"], "score": 7, "risky": true } } },
            "file": { "path": "a.rs" },
        })
    }

    fn eval(text: &str) -> Value {
        let d = data();
        parse(text).unwrap().eval(&|p| dig(d.clone(), p))
    }

    #[test]
    fn compares_and_combines() {
        assert_eq!(eval("steps.triage.output.severity == 'high'"), json!(true));
        assert_eq!(eval("steps.triage.output.score >= 7 and not steps.triage.output.risky"), json!(false));
        assert_eq!(eval("steps.triage.output.score > 10 || steps.triage.output.risky"), json!(true));
        assert_eq!(eval("steps.triage.output.files contains \"b.rs\""), json!(true));
        assert_eq!(eval("steps.triage.output.severity contains 'IG'"), json!(true));
        assert_eq!(eval("(steps.triage.output.score < 3 or file.path == 'a.rs') and true"), json!(true));
        assert_eq!(eval("'3' < 10"), json!(true));
        assert_eq!(eval("-2 < 1"), json!(true));
    }

    #[test]
    fn missing_is_null_and_false() {
        assert_eq!(eval("steps.nope.output"), Value::Null);
        assert_eq!(eval("steps.triage.output.files.1"), json!("b.rs"));
        assert_eq!(eval("steps.triage.output.files.9"), Value::Null);
        assert_eq!(eval("steps.nope.output == null"), json!(true));
        assert!(!truthy(&eval("steps.triage.output.missing")));
    }

    #[test]
    fn reports_what_is_wrong() {
        assert!(parse("").unwrap_err().contains("empty"));
        assert!(parse("a ==").unwrap_err().contains("ends too early"));
        assert!(parse("(a").unwrap_err().contains("isn't closed"));
        assert!(parse("a b").unwrap_err().contains("Unexpected"));
        assert!(parse("'open").unwrap_err().contains("isn't closed"));
        assert!(parse("a.").unwrap_err().contains("name after the dot"));
        assert!(parse("a # b").unwrap_err().contains("Unexpected `#`"));
    }

    #[test]
    fn lists_its_paths() {
        let e = parse("steps.a.output.x == file and not steps-b").unwrap();
        let paths: Vec<String> = e.paths().iter().map(|p| p.join(".")).collect();
        assert_eq!(paths, ["steps.a.output.x", "file", "steps-b"]);
    }

    #[test]
    fn renders_templates() {
        let d = data();
        let lookup = |p: &[String]| dig(d.clone(), p);
        assert_eq!(render("Fix {{ file.path }} ({{steps.triage.output.severity}})", &lookup).unwrap(), "Fix a.rs (High)");
        assert_eq!(render("{{ steps.triage.output.files }}", &lookup).unwrap(), "[\n  \"a.rs\",\n  \"b.rs\"\n]");
        assert_eq!(render("no templates", &lookup).unwrap(), "no templates");
        assert!(render("{{ open", &lookup).unwrap_err().contains("isn't closed"));
    }
}
