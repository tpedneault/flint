//! The calculator (prefix `=`): the query evaluated as arithmetic, its
//! result one item that `Enter` copies.
//!
//! A small recursive-descent parser over `f64`, written here rather than
//! taken from a crate or a shell (`bc`, PowerShell): nothing typed into
//! Flint is ever run, it answers within the keystroke's frame, and the
//! grammar is exactly what this header says.
//!
//! - `+ - * / %` (remainder), `^` or `**` (power, right-associative,
//!   binding tighter than a leading minus: `-2^2` is -4), parentheses, and
//!   `×`, `÷` as typed on some keyboards.
//! - Numbers: `1.5`, `.5`, `1e-3`, `0x1f`, `0b101`, `0o17`, with `_` as a
//!   separator. A comma is never a digit separator or decimal point: it
//!   separates function arguments, and guessing the locale would make
//!   `1,5` mean two things.
//! - Constants `pi`, `tau`, `e`; functions `sqrt abs round floor ceil ln
//!   log (base 10) log2 exp sin cos tan asin acos atan min max` (radians).
//!
//! Unscoped, the calculator answers only when the whole query parses and
//! does some arithmetic (an operator or a function), so typing a bare
//! number or a word never shows it; with `= ` it answers whatever parses.
//! It deliberately has no variables, units or currency.

use flint_protocol::item::{Action, ActionKind};
use flint_protocol::Item;

/// The provider's id.
pub const ID: &str = "calculator";
/// Its badge and chip.
pub const TITLE: &str = "Calculator";
/// Its scope keyword.
pub const PREFIX: &str = "=";

/// The result for `text` as an item, if it is arithmetic. `scoped` when
/// the user typed `= `, which drops the "does some arithmetic" test.
pub fn item(text: &str, scoped: bool) -> Option<Item> {
    // `=2+2`, typed without the space a keyword needs, is as clear.
    let (text, scoped) = match text.trim_start().strip_prefix('=') {
        Some(rest) => (rest, true),
        None => (text, scoped),
    };
    let (value, arithmetic) = evaluate(text).ok()?;
    if !scoped && !arithmetic {
        return None;
    }
    let result = format_number(value);
    Some(Item {
        // One id for every result: frecency then learns that the
        // calculator is wanted, not which number was.
        id: format!("{ID}:result"),
        title: result.clone(),
        subtitle: Some(format!("= {}", text.trim())),
        icon: Some("calculator".into()),
        keywords: Vec::new(),
        actions: vec![Action {
            id: "copy".into(),
            title: "Copy result".into(),
            key: Some("c".into()),
            kind: ActionKind::Copy { text: result },
        }],
    })
}

/// The value of `text`, and whether it did any arithmetic (as opposed to
/// being a number on its own). Errors name what was wrong.
pub fn evaluate(text: &str) -> Result<(f64, bool), String> {
    let tokens = tokenize(text)?;
    if tokens.is_empty() {
        return Err("nothing to calculate".into());
    }
    let mut parser = Parser {
        tokens,
        at: 0,
        arithmetic: false,
    };
    let value = parser.expression()?;
    if let Some(token) = parser.tokens.get(parser.at) {
        return Err(format!("unexpected {}", token.describe()));
    }
    if !value.is_finite() {
        return Err("the result isn't a finite number".into());
    }
    Ok((value, parser.arithmetic))
}

/// A result as people write it: whole numbers exactly while `f64` holds
/// them exactly (to 2^53), others to 12 significant digits so `0.1 + 0.2`
/// reads `0.3`, and anything beyond 10^15 in scientific notation rather
/// than with made-up trailing digits.
pub fn format_number(value: f64) -> String {
    const EXACT: f64 = 9_007_199_254_740_992.0; // 2^53
    if value == 0.0 {
        return "0".into(); // never "-0"
    }
    if value.fract() == 0.0 && value.abs() <= EXACT {
        return format!("{}", value as i64);
    }
    let rounded: f64 = format!("{value:.11e}").parse().unwrap_or(value);
    if rounded.abs() >= 1e15 {
        format!("{rounded:e}")
    } else {
        format!("{rounded}")
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Name(String),
    Op(char),
    Open,
    Close,
    Comma,
}

impl Token {
    fn describe(&self) -> String {
        match self {
            Token::Number(n) => format!("number {}", format_number(*n)),
            Token::Name(name) => format!("`{name}`"),
            Token::Op(c) => format!("`{c}`"),
            Token::Open => "`(`".into(),
            Token::Close => "`)`".into(),
            Token::Comma => "`,`".into(),
        }
    }
}

fn tokenize(text: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' => i += 1,
            '0'..='9' | '.' => {
                let start = i;
                let radix = match (c, chars.get(i + 1)) {
                    ('0', Some('x' | 'X')) => 16,
                    ('0', Some('b' | 'B')) => 2,
                    ('0', Some('o' | 'O')) => 8,
                    _ => 10,
                };
                if radix != 10 {
                    i += 2;
                    let digits_start = i;
                    while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                        i += 1;
                    }
                    let digits: String = chars[digits_start..i]
                        .iter()
                        .filter(|c| **c != '_')
                        .collect();
                    let value = u64::from_str_radix(&digits, radix).map_err(|_| {
                        format!(
                            "`{}` isn't a number",
                            chars[start..i].iter().collect::<String>()
                        )
                    })?;
                    tokens.push(Token::Number(value as f64));
                    continue;
                }
                while i < chars.len()
                    && (chars[i].is_ascii_digit() || matches!(chars[i], '.' | '_'))
                {
                    i += 1;
                }
                // An exponent: `e` then digits, optionally signed. Without
                // the digits the `e` isn't taken (`2e` is then an error,
                // there being no implicit multiplication).
                if i < chars.len() && matches!(chars[i], 'e' | 'E') {
                    let mut j = i + 1;
                    if j < chars.len() && matches!(chars[j], '+' | '-') {
                        j += 1;
                    }
                    if j < chars.len() && chars[j].is_ascii_digit() {
                        i = j;
                        while i < chars.len() && chars[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                }
                let literal: String = chars[start..i].iter().filter(|c| **c != '_').collect();
                let value = literal
                    .parse::<f64>()
                    .map_err(|_| format!("`{literal}` isn't a number"))?;
                tokens.push(Token::Number(value));
            }
            'a'..='z' | 'A'..='Z' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric()) {
                    i += 1;
                }
                let name: String = chars[start..i].iter().collect();
                tokens.push(Token::Name(name.to_ascii_lowercase()));
            }
            '*' if chars.get(i + 1) == Some(&'*') => {
                tokens.push(Token::Op('^'));
                i += 2;
            }
            '+' | '-' | '*' | '/' | '%' | '^' => {
                tokens.push(Token::Op(c));
                i += 1;
            }
            '×' => {
                tokens.push(Token::Op('*'));
                i += 1;
            }
            '÷' => {
                tokens.push(Token::Op('/'));
                i += 1;
            }
            '(' => {
                tokens.push(Token::Open);
                i += 1;
            }
            ')' => {
                tokens.push(Token::Close);
                i += 1;
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            other => return Err(format!("`{other}` isn't something the calculator knows")),
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
    arithmetic: bool,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.at).cloned();
        self.at += 1;
        token
    }

    fn expect(&mut self, wanted: Token) -> Result<(), String> {
        match self.next() {
            Some(token) if token == wanted => Ok(()),
            Some(token) => Err(format!(
                "expected {}, found {}",
                wanted.describe(),
                token.describe()
            )),
            None => Err(format!("expected {} at the end", wanted.describe())),
        }
    }

    /// `term (('+' | '-') term)*`
    fn expression(&mut self) -> Result<f64, String> {
        let mut value = self.term()?;
        while let Some(Token::Op(op @ ('+' | '-'))) = self.peek().cloned() {
            self.at += 1;
            self.arithmetic = true;
            let right = self.term()?;
            value = if op == '+' {
                value + right
            } else {
                value - right
            };
        }
        Ok(value)
    }

    /// `unary (('*' | '/' | '%') unary)*`
    fn term(&mut self) -> Result<f64, String> {
        let mut value = self.unary()?;
        while let Some(Token::Op(op @ ('*' | '/' | '%'))) = self.peek().cloned() {
            self.at += 1;
            self.arithmetic = true;
            let right = self.unary()?;
            value = match op {
                '*' => value * right,
                '/' => value / right,
                _ => value % right,
            };
        }
        Ok(value)
    }

    /// `('-' | '+') unary | power`
    fn unary(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some(Token::Op('-')) => {
                self.at += 1;
                Ok(-self.unary()?)
            }
            Some(Token::Op('+')) => {
                self.at += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    /// `atom ('^' unary)?`, right-associative through `unary`.
    fn power(&mut self) -> Result<f64, String> {
        let base = self.atom()?;
        if let Some(Token::Op('^')) = self.peek() {
            self.at += 1;
            self.arithmetic = true;
            let exponent = self.unary()?;
            return Ok(base.powf(exponent));
        }
        Ok(base)
    }

    /// A number, a constant, a function call or a parenthesised
    /// expression.
    fn atom(&mut self) -> Result<f64, String> {
        match self.next() {
            Some(Token::Number(n)) => Ok(n),
            Some(Token::Open) => {
                let value = self.expression()?;
                self.expect(Token::Close)?;
                Ok(value)
            }
            Some(Token::Name(name)) => {
                if let Some(value) = constant(&name) {
                    return Ok(value);
                }
                if self.peek() != Some(&Token::Open) {
                    return Err(format!("`{name}` isn't a constant or a function"));
                }
                self.at += 1;
                let mut args = vec![self.expression()?];
                while self.peek() == Some(&Token::Comma) {
                    self.at += 1;
                    args.push(self.expression()?);
                }
                self.expect(Token::Close)?;
                self.arithmetic = true;
                call(&name, &args)
            }
            Some(token) => Err(format!("unexpected {}", token.describe())),
            None => Err("the expression ends too soon".into()),
        }
    }
}

fn constant(name: &str) -> Option<f64> {
    Some(match name {
        "pi" => std::f64::consts::PI,
        "tau" => std::f64::consts::TAU,
        "e" => std::f64::consts::E,
        _ => return None,
    })
}

fn call(name: &str, args: &[f64]) -> Result<f64, String> {
    let one = |f: fn(f64) -> f64| match args {
        [x] => Ok(f(*x)),
        _ => Err(format!("`{name}` takes one argument")),
    };
    match name {
        "sqrt" => one(f64::sqrt),
        "abs" => one(f64::abs),
        "round" => one(f64::round),
        "floor" => one(f64::floor),
        "ceil" => one(f64::ceil),
        "ln" => one(f64::ln),
        "log" => one(f64::log10),
        "log2" => one(f64::log2),
        "exp" => one(f64::exp),
        "sin" => one(f64::sin),
        "cos" => one(f64::cos),
        "tan" => one(f64::tan),
        "asin" => one(f64::asin),
        "acos" => one(f64::acos),
        "atan" => one(f64::atan),
        "min" | "max" if args.len() >= 2 => Ok(args.iter().copied().fold(
            if name == "min" {
                f64::INFINITY
            } else {
                f64::NEG_INFINITY
            },
            if name == "min" { f64::min } else { f64::max },
        )),
        "min" | "max" => Err(format!("`{name}` takes two or more arguments")),
        _ => Err(format!("`{name}` isn't a function the calculator knows")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(text: &str) -> f64 {
        evaluate(text).unwrap().0
    }

    #[test]
    fn precedence_and_associativity() {
        assert_eq!(eval("1 + 2 * 3"), 7.0);
        assert_eq!(eval("(1 + 2) * 3"), 9.0);
        assert_eq!(eval("2 ^ 3 ^ 2"), 512.0, "power is right-associative");
        assert_eq!(eval("-2^2"), -4.0, "power binds tighter than minus");
        assert_eq!(eval("2**10"), 1024.0);
        assert_eq!(eval("10 - 4 - 3"), 3.0);
        assert_eq!(eval("7 % 4"), 3.0);
        assert_eq!(eval("6 × 7 ÷ 2"), 21.0);
        assert_eq!(eval("--3"), 3.0);
    }

    #[test]
    fn numbers_constants_and_functions() {
        assert_eq!(eval("0xff + 0b11 + 0o10"), 266.0);
        assert_eq!(eval("1_000 * 1e3"), 1e6);
        assert_eq!(eval(".5 + 1.5e-1"), 0.65);
        assert_eq!(eval("sqrt(16) + abs(-2)"), 6.0);
        assert_eq!(eval("max(1, 5, 3) - min(4, 2)"), 3.0);
        assert_eq!(eval("round(PI * 100)"), 314.0, "names ignore case");
        assert_eq!(eval("log(1000)"), 3.0);
    }

    #[test]
    fn mistakes_are_one_line_and_never_panic() {
        for (text, why) in [
            ("", "nothing to calculate"),
            ("1 +", "the expression ends too soon"),
            ("(1 + 2", "expected `)` at the end"),
            ("1 2", "unexpected number 2"),
            ("foo(1)", "`foo` isn't a function the calculator knows"),
            ("x", "`x` isn't a constant or a function"),
            ("1 / 0", "the result isn't a finite number"),
            ("2 $ 3", "`$` isn't something the calculator knows"),
            ("sqrt(1, 2)", "`sqrt` takes one argument"),
            ("0xzz", "`0xzz` isn't a number"),
            ("1,5", "unexpected `,`"),
            ("2e", "unexpected `e`"),
        ] {
            assert_eq!(evaluate(text).unwrap_err(), why, "{text}");
        }
    }

    #[test]
    fn results_read_as_people_write_them() {
        assert_eq!(format_number(eval("0.1 + 0.2")), "0.3");
        assert_eq!(format_number(eval("2 ^ 10")), "1024");
        assert_eq!(format_number(eval("-0 * 5")), "0");
        assert_eq!(format_number(eval("1 / 3")), "0.333333333333");
        assert_eq!(format_number(eval("2 ^ 53")), "9007199254740992");
        assert_eq!(format_number(eval("2 ^ 60")), "1.15292150461e18");
        assert_eq!(format_number(eval("1e20 / 3")), "3.33333333333e19");
    }

    #[test]
    fn unscoped_only_for_arithmetic() {
        assert!(item("2 + 2", false).is_some());
        assert!(item("sqrt(2)", false).is_some());
        assert!(item("42", false).is_none(), "a bare number isn't a sum");
        assert!(item("pi", false).is_none());
        assert!(item("firefox", false).is_none());
        assert_eq!(
            item("=2*21", false).unwrap().title,
            "42",
            "`=` without its space"
        );
        let scoped = item("pi", true).unwrap();
        assert_eq!(scoped.title, "3.14159265359");
        assert!(scoped.check().is_ok());
        let sum = item("6 * 7", false).unwrap();
        assert_eq!(sum.title, "42");
        assert_eq!(sum.subtitle.as_deref(), Some("= 6 * 7"));
        assert_eq!(sum.actions[0].kind, ActionKind::Copy { text: "42".into() });
    }
}
