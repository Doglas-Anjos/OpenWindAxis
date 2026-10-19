//! Algebraic expressions of the calculated columns (manual
//! articles-algebraic-expressions.htm, dlg-cc-algebraic-expression.htm):
//! statements in C syntax that assign `X` from up to four columns `A`–`D`.
//!
//! - statements: `X = expression;`, `if (condition) statement`, optionally
//!   `else statement` (so `else if` too), and `{ … }` blocks;
//! - operators, loosest first: `?:`, `||`, `&&`, `==` `!=`, `<` `<=` `>`
//!   `>=` (also `≤` `≥`), `+` `-`, `*` `/` `%`, unary `-` `+` `!`, and `^`
//!   for the power (the manual's, not C's exclusive or), right-associative
//!   and tighter than the unary minus (`-A^2` is `-(A²)`);
//! - comparisons and logic give 1 or 0; a condition holds when not 0;
//! - functions: `min`, `max`, `sqrt`, `pow`, `exp`, `log`, `log10`, `sin`,
//!   `cos`, `tan` (radians), `mod` (the manual's: `mod(373.3, 360)` is 13.3;
//!   a negative value wraps into [0, divisor)) and `abs`.
//!
//! A step where a column the expression names has no value (missing, or
//! flagged to exclude) gets none, as every calculated column; a step where
//! `X` is not assigned gets none too.

/// The columns an expression can name, in order.
pub const VARIABLES: [char; 4] = ['A', 'B', 'C', 'D'];
/// The variable an expression assigns.
const RESULT: char = 'X';

/// Why an expression does not parse: a message and the character position.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpressionError {
    /// What went wrong.
    pub message: String,
    /// Character offset in the text.
    pub position: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Number(f64),
    /// `A`–`D` by index, or `X` (index 4).
    Variable(usize),
    Unary(char, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(&'static str, Vec<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
enum Statement {
    Assign(Expr),
    If(Expr, Box<Statement>, Option<Box<Statement>>),
    Block(Vec<Statement>),
}

/// A parsed expression, ready to evaluate step after step.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    statements: Vec<Statement>,
    /// Which of `A`–`D` it names.
    uses: [bool; VARIABLES.len()],
}

const RESULT_SLOT: usize = VARIABLES.len();
/// Functions and how many arguments each takes.
const FUNCTIONS: [(&str, usize); 12] =
    [("min", 2), ("max", 2), ("sqrt", 1), ("pow", 2), ("exp", 1), ("log", 1), ("log10", 1), ("sin", 1), ("cos", 1), ("tan", 1), ("mod", 2), ("abs", 1)];
/// Binary operators by precedence level, loosest first (after `?:`).
const LEVELS: [&[&str]; 6] = [&["||"], &["&&"], &["==", "!="], &["<=", ">=", "<", ">", "≤", "≥"], &["+", "-"], &["*", "/", "%"]];

struct Parser {
    chars: Vec<char>,
    at: usize,
    uses: [bool; VARIABLES.len()],
}

impl Parser {
    fn error<T>(&self, message: &str) -> Result<T, ExpressionError> {
        Err(ExpressionError { message: message.to_string(), position: self.at })
    }

    fn skip_space(&mut self) {
        while self.chars.get(self.at).is_some_and(|char| char.is_whitespace()) {
            self.at += 1;
        }
    }

    /// Takes `token` when it comes next (after spaces).
    fn eat(&mut self, token: &str) -> bool {
        self.skip_space();
        let token: Vec<char> = token.chars().collect();
        if self.chars[self.at..].starts_with(&token) {
            // A word must not run on into a longer one ("if" in "iff").
            let word = token.iter().all(|char| char.is_alphanumeric());
            if word && self.chars.get(self.at + token.len()).is_some_and(|char| char.is_alphanumeric() || *char == '_') {
                return false;
            }
            self.at += token.len();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, token: &str) -> Result<(), ExpressionError> {
        if self.eat(token) {
            Ok(())
        } else {
            self.error(&format!("expected '{token}'"))
        }
    }

    fn word(&mut self) -> Option<String> {
        self.skip_space();
        let start = self.at;
        while self.chars.get(self.at).is_some_and(|char| char.is_alphanumeric() || *char == '_') {
            self.at += 1;
        }
        (self.at > start).then(|| self.chars[start..self.at].iter().collect())
    }

    fn statement(&mut self) -> Result<Statement, ExpressionError> {
        if self.eat("{") {
            let mut block = Vec::new();
            while !self.eat("}") {
                if self.at_end() {
                    return self.error("expected '}'");
                }
                block.push(self.statement()?);
            }
            return Ok(Statement::Block(block));
        }
        if self.eat("if") {
            self.expect("(")?;
            let condition = self.expression()?;
            self.expect(")")?;
            let then = Box::new(self.statement()?);
            let otherwise = if self.eat("else") { Some(Box::new(self.statement()?)) } else { None };
            return Ok(Statement::If(condition, then, otherwise));
        }
        if self.eat(";") {
            return Ok(Statement::Block(Vec::new()));
        }
        let start = self.at;
        let result = RESULT.to_string();
        if self.word().as_deref() != Some(result.as_str()) {
            self.at = start;
            return self.error("expected 'X =', 'if' or '{'");
        }
        self.expect("=")?;
        let value = self.expression()?;
        self.expect(";")?;
        Ok(Statement::Assign(value))
    }

    fn at_end(&mut self) -> bool {
        self.skip_space();
        self.at >= self.chars.len()
    }

    fn expression(&mut self) -> Result<Expr, ExpressionError> {
        let condition = self.binary(0)?;
        if self.eat("?") {
            let then = self.expression()?;
            self.expect(":")?;
            let otherwise = self.expression()?;
            return Ok(Expr::Conditional(Box::new(condition), Box::new(then), Box::new(otherwise)));
        }
        Ok(condition)
    }

    fn binary(&mut self, level: usize) -> Result<Expr, ExpressionError> {
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut left = self.binary(level + 1)?;
        'operators: loop {
            for &operator in LEVELS[level] {
                // "=" alone is assignment, never "==" cut short; "!" alone is not "!=".
                if self.eat(operator) {
                    let right = self.binary(level + 1)?;
                    left = Expr::Binary(operator, Box::new(left), Box::new(right));
                    continue 'operators;
                }
            }
            return Ok(left);
        }
    }

    fn unary(&mut self) -> Result<Expr, ExpressionError> {
        for operator in ['-', '+', '!'] {
            self.skip_space();
            let next_is_not_equal = operator == '!' && self.chars.get(self.at + 1) == Some(&'=');
            if !next_is_not_equal && self.eat(&operator.to_string()) {
                return Ok(Expr::Unary(operator, Box::new(self.unary()?)));
            }
        }
        self.power()
    }

    fn power(&mut self) -> Result<Expr, ExpressionError> {
        let base = self.primary()?;
        if self.eat("^") {
            let exponent = self.unary()?;
            return Ok(Expr::Binary("^", Box::new(base), Box::new(exponent)));
        }
        Ok(base)
    }

    fn primary(&mut self) -> Result<Expr, ExpressionError> {
        self.skip_space();
        if self.eat("(") {
            let inner = self.expression()?;
            self.expect(")")?;
            return Ok(inner);
        }
        let start = self.at;
        if self.chars.get(self.at).is_some_and(|char| char.is_ascii_digit() || *char == '.') {
            while self.chars.get(self.at).is_some_and(|char| char.is_ascii_digit() || *char == '.') {
                self.at += 1;
            }
            if self.chars.get(self.at).is_some_and(|char| *char == 'e' || *char == 'E') {
                self.at += 1;
                if self.chars.get(self.at).is_some_and(|char| *char == '+' || *char == '-') {
                    self.at += 1;
                }
                while self.chars.get(self.at).is_some_and(char::is_ascii_digit) {
                    self.at += 1;
                }
            }
            let text: String = self.chars[start..self.at].iter().collect();
            return text.parse().map(Expr::Number).or_else(|_| {
                self.at = start;
                self.error("not a number")
            });
        }
        let Some(name) = self.word() else { return self.error("expected a number, a column (A–D), a function or '('") };
        let mut letters = name.chars();
        if let (Some(letter), None) = (letters.next(), letters.next()) {
            if let Some(index) = VARIABLES.iter().position(|variable| *variable == letter) {
                self.uses[index] = true;
                return Ok(Expr::Variable(index));
            }
            if letter == RESULT {
                return Ok(Expr::Variable(RESULT_SLOT));
            }
        }
        let Some(&(function, arity)) = FUNCTIONS.iter().find(|(function, _)| *function == name) else {
            self.at = start;
            return self.error(&format!("unknown name '{name}'"));
        };
        self.expect("(")?;
        let mut arguments = vec![self.expression()?];
        while self.eat(",") {
            arguments.push(self.expression()?);
        }
        self.expect(")")?;
        if arguments.len() != arity {
            self.at = start;
            return self.error(&format!("{function} takes {arity} argument(s)"));
        }
        Ok(Expr::Call(function, arguments))
    }
}

impl Program {
    /// Parses `text`.
    ///
    /// # Errors
    /// [`ExpressionError`] at the first thing that does not parse, or when no
    /// statement assigns `X`.
    pub fn parse(text: &str) -> Result<Program, ExpressionError> {
        let mut parser = Parser { chars: text.chars().collect(), at: 0, uses: [false; VARIABLES.len()] };
        let mut statements = Vec::new();
        while !parser.at_end() {
            statements.push(parser.statement()?);
        }
        if !statements.iter().any(assigns) {
            return Err(ExpressionError { message: "no statement assigns X".into(), position: 0 });
        }
        Ok(Program { statements, uses: parser.uses })
    }

    /// Which of `A`–`D` it names.
    pub fn uses(&self) -> [bool; VARIABLES.len()] {
        self.uses
    }

    /// `X` for the column values `values` (`A`–`D` in order; NaN = none):
    /// NaN when a named column has none or `X` is never assigned.
    pub fn evaluate(&self, values: &[f64]) -> f64 {
        let mut slots = [f64::NAN; VARIABLES.len() + 1];
        for (index, used) in self.uses.iter().enumerate() {
            if *used {
                let value = values.get(index).copied().unwrap_or(f64::NAN);
                if !value.is_finite() {
                    return f64::NAN;
                }
                slots[index] = value;
            }
        }
        for statement in &self.statements {
            run(statement, &mut slots);
        }
        slots[RESULT_SLOT]
    }
}

fn assigns(statement: &Statement) -> bool {
    match statement {
        Statement::Assign(_) => true,
        Statement::If(_, then, otherwise) => assigns(then) || otherwise.as_deref().is_some_and(assigns),
        Statement::Block(block) => block.iter().any(assigns),
    }
}

fn run(statement: &Statement, slots: &mut [f64; VARIABLES.len() + 1]) {
    match statement {
        Statement::Assign(value) => slots[RESULT_SLOT] = eval(value, slots),
        Statement::If(condition, then, otherwise) => {
            if eval(condition, slots) != 0.0 {
                run(then, slots);
            } else if let Some(otherwise) = otherwise {
                run(otherwise, slots);
            }
        }
        Statement::Block(block) => block.iter().for_each(|statement| run(statement, slots)),
    }
}

fn truth(holds: bool) -> f64 {
    if holds {
        1.0
    } else {
        0.0
    }
}

fn eval(expr: &Expr, slots: &[f64; VARIABLES.len() + 1]) -> f64 {
    match expr {
        Expr::Number(value) => *value,
        Expr::Variable(index) => slots[*index],
        Expr::Unary(operator, inner) => {
            let value = eval(inner, slots);
            match operator {
                '-' => -value,
                '!' => truth(value == 0.0),
                _ => value,
            }
        }
        Expr::Binary(operator, left, right) => {
            let (left, right) = (eval(left, slots), eval(right, slots));
            match *operator {
                "+" => left + right,
                "-" => left - right,
                "*" => left * right,
                "/" => left / right,
                "%" => left % right,
                "^" => left.powf(right),
                "==" => truth(left == right),
                "!=" => truth(left != right),
                "<" => truth(left < right),
                "<=" | "≤" => truth(left <= right),
                ">" => truth(left > right),
                ">=" | "≥" => truth(left >= right),
                "&&" => truth(left != 0.0 && right != 0.0),
                "||" => truth(left != 0.0 || right != 0.0),
                _ => f64::NAN,
            }
        }
        Expr::Conditional(condition, then, otherwise) => {
            if eval(condition, slots) != 0.0 {
                eval(then, slots)
            } else {
                eval(otherwise, slots)
            }
        }
        Expr::Call(function, arguments) => {
            let values: Vec<f64> = arguments.iter().map(|argument| eval(argument, slots)).collect();
            let (first, second) = (values[0], values.get(1).copied().unwrap_or(f64::NAN));
            match *function {
                "min" => first.min(second),
                "max" => first.max(second),
                "sqrt" => first.sqrt(),
                "pow" => first.powf(second),
                "exp" => libm::exp(first),
                "log" => libm::log(first),
                "log10" => libm::log10(first),
                "sin" => libm::sin(first),
                "cos" => libm::cos(first),
                "tan" => libm::tan(first),
                "mod" => first.rem_euclid(second),
                "abs" => first.abs(),
                _ => f64::NAN,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x(text: &str, values: &[f64]) -> f64 {
        Program::parse(text).unwrap().evaluate(values)
    }

    #[test]
    fn the_manuals_examples() {
        assert_eq!(x("X = A + 4;", &[1.0]), 5.0);
        assert_eq!(x("X = A ^ 3;", &[2.0]), 8.0);
        assert_eq!(x("X = (A == 6.5);", &[6.5]), 1.0);
        assert_eq!(x("if (A > 50) X = A; else X = 50;", &[20.0]), 50.0);
        assert_eq!(x("if (A ≥ 0) X = sqrt(A); else X = -1;", &[9.0]), 3.0);
        assert_eq!(x("if ((A>0) && (B>A)) X = 1; else X = 0;", &[1.0, 2.0]), 1.0);
        assert_eq!(x("X = max(0.5 * A / B, 0);", &[-1.0, 1.0]), 0.0);
        assert!((x("X = mod(A, 360);", &[373.3]) - 13.3).abs() < 1e-9);
        assert_eq!(x("X = mod(A, 360);", &[-10.0]), 350.0);
        assert_eq!(x("X = 0.5 * B * A * A * A;", &[10.0, 1.2]), 600.0);
        assert_eq!(x("X = A < 13.5 ? 0.5 * B * A^3 : 1600;", &[20.0, 1.2]), 1600.0);
        assert_eq!(x("if (A < 0) { X = 0; } else if (A < 10) { X = A; } else { X = 10; }", &[5.0]), 5.0);
    }

    #[test]
    fn precedence_and_missing_values() {
        assert_eq!(x("X = -A^2;", &[3.0]), -9.0, "the power binds tighter than the minus");
        assert_eq!(x("X = 2^3^2;", &[]), 512.0, "right-associative");
        assert_eq!(x("X = 1 + 2 * 3 - 4 / 2;", &[]), 5.0);
        assert_eq!(x("X = !(A != 0);", &[0.0]), 1.0);
        assert!(x("X = A + B;", &[1.0, f64::NAN]).is_nan(), "a named column without a value");
        assert_eq!(x("X = A;", &[1.0, f64::NAN]), 1.0, "an unnamed one does not matter");
        assert!(x("if (A > 5) X = 1;", &[1.0]).is_nan(), "X never assigned");
    }

    #[test]
    fn errors_say_where() {
        let error = Program::parse("X = A +;").unwrap_err();
        assert_eq!(error.position, 7);
        assert!(Program::parse("Y = 1;").is_err());
        assert!(Program::parse("X = foo(A);").unwrap_err().message.contains("foo"));
        assert!(Program::parse("X = min(A);").unwrap_err().message.contains("2 argument"));
        assert!(Program::parse("if (A > 1) ;").unwrap_err().message.contains("assigns X"));
    }
}
