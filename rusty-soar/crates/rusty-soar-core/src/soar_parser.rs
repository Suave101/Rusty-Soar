//! Lexer and parser for Soar productions, ported from upstream
//! `Core/SoarKernel/src/parsing/lexer.cpp` and `parser.cpp`.
//!
//! The grammar, lexing rules, placeholder variables, attribute-path (`^a.b.c`) expansion,
//! negation handling and preference syntax follow upstream. Validation and condition
//! ordering, which upstream does in `production_reorder.cpp`, live in
//! [`crate::production::compile`].
//!
//! Script-level handling is a local extension: `sp {…}` and `sp "…"` commands are extracted
//! from a file, other commands are skipped, and simple Tcl `proc`/`foreach` rule generators are
//! expanded first.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

// ======================================================================
// AST
// ======================================================================

/// A symbol as written in a production (upstream `make_symbol_for_lexeme`).
#[derive(Debug, Clone, PartialEq)]
pub enum SoarSymbol {
    /// String constant. Identifier-like lexemes (`S1`) also become string constants.
    Str(String),
    /// Integer constant.
    Int(i64),
    /// Floating-point constant.
    Float(f64),
    /// Variable, including its angle brackets (`<s>`).
    Variable(String),
}

impl SoarSymbol {
    /// Whether this is a variable.
    pub fn is_variable(&self) -> bool {
        matches!(self, SoarSymbol::Variable(_))
    }

    /// Upstream `first_letter_from_symbol`.
    fn first_letter(&self) -> char {
        match self {
            SoarSymbol::Variable(name) => name.chars().nth(1).unwrap_or('*'),
            SoarSymbol::Str(s) => s.chars().next().map_or('*', |c| c.to_ascii_lowercase()),
            SoarSymbol::Int(_) => 'i',
            SoarSymbol::Float(_) => 'f',
        }
    }
}

impl fmt::Display for SoarSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SoarSymbol::Str(s) => write!(f, "{}", s),
            SoarSymbol::Int(i) => write!(f, "{}", i),
            SoarSymbol::Float(x) => write!(f, "{}", x),
            SoarSymbol::Variable(v) => write!(f, "{}", v),
        }
    }
}

/// Relation of a relational test (upstream `TestType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoarRelation {
    /// `=` or no relation.
    Equal,
    /// `<>`
    NotEqual,
    /// `<`
    Less,
    /// `>`
    Greater,
    /// `<=`
    LessOrEqual,
    /// `>=`
    GreaterOrEqual,
    /// `<=>`
    SameType,
    /// `@` (semantic-memory link)
    SmemLink,
    /// `!@`
    SmemLinkNot,
    /// `$$` (constant match, used by chunking)
    ConstantMatch,
}

/// A test on one field of a condition (upstream `test`).
#[derive(Debug, Clone, PartialEq)]
pub enum SoarTest {
    /// `[relation] referent`
    Relational(SoarRelation, SoarSymbol),
    /// `<< a b c >>`
    Disjunction(Vec<SoarSymbol>),
    /// `{ t1 t2 … }`
    Conjunctive(Vec<SoarTest>),
    /// `state` keyword.
    Goal,
    /// `impasse` keyword.
    Impasse,
    /// `@+`
    SmemLinkUnary,
    /// `@-`
    SmemLinkUnaryNot,
}

impl SoarTest {
    /// The equality part of a test, if it has one (upstream `eq_test`).
    pub fn eq_test(&self) -> Option<&SoarSymbol> {
        match self {
            SoarTest::Relational(SoarRelation::Equal, symbol) => Some(symbol),
            SoarTest::Conjunctive(tests) => tests.iter().find_map(SoarTest::eq_test),
            _ => None,
        }
    }

    /// Upstream `first_letter_from_test`.
    fn first_letter(&self) -> char {
        match self {
            SoarTest::Relational(SoarRelation::Equal, symbol) => symbol.first_letter(),
            SoarTest::Conjunctive(_) => self.eq_test().map_or('*', SoarSymbol::first_letter),
            SoarTest::Goal => 's',
            SoarTest::Impasse => 'i',
            _ => '*',
        }
    }

    /// Upstream `add_test`: conjoins `other` onto this test.
    fn add(self, other: SoarTest) -> SoarTest {
        match (self, other) {
            (SoarTest::Conjunctive(mut tests), SoarTest::Conjunctive(more)) => {
                tests.extend(more);
                SoarTest::Conjunctive(tests)
            }
            (SoarTest::Conjunctive(mut tests), other) => {
                tests.push(other);
                SoarTest::Conjunctive(tests)
            }
            (this, SoarTest::Conjunctive(mut more)) => {
                more.insert(0, this);
                SoarTest::Conjunctive(more)
            }
            (this, other) => SoarTest::Conjunctive(vec![this, other]),
        }
    }
}

/// The three field tests of a single condition.
#[derive(Debug, Clone, PartialEq)]
pub struct SoarConditionTests {
    /// Identifier test.
    pub id: SoarTest,
    /// Attribute test.
    pub attr: SoarTest,
    /// Value test.
    pub value: SoarTest,
    /// Whether the condition tests an acceptable preference (`+`).
    pub acceptable: bool,
}

/// A left-hand-side condition (upstream `condition`).
#[derive(Debug, Clone, PartialEq)]
pub enum SoarCondition {
    /// Must match a WME.
    Positive(SoarConditionTests),
    /// `-( … )` or `-^attr …`: no WME may match.
    Negative(SoarConditionTests),
    /// `-{ … }`: the conjunction may not match.
    ConjunctiveNegation(Vec<SoarCondition>),
}

/// A right-hand-side value (upstream `rhs_value`).
#[derive(Debug, Clone, PartialEq)]
pub enum SoarRhsValue {
    /// Constant or variable.
    Symbol(SoarSymbol),
    /// `(name args…)`
    FunctionCall {
        /// Function name (`+`, `-`, `write`, …).
        name: String,
        /// Arguments.
        args: Vec<SoarRhsValue>,
    },
}

/// Preference type of a RHS make action (upstream `PreferenceType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoarPreference {
    /// `+` (default)
    Acceptable,
    /// `-`
    Reject,
    /// `!`
    Require,
    /// `~`
    Prohibit,
    /// `> referent`
    Better,
    /// `< referent`
    Worse,
    /// `>` alone
    Best,
    /// `<` alone
    Worst,
    /// `=` alone
    UnaryIndifferent,
    /// `= referent`
    BinaryIndifferent,
    /// `= number`
    NumericIndifferent,
}

impl SoarPreference {
    /// Whether the preference takes a referent (upstream `preference_is_binary`).
    pub fn is_binary(self) -> bool {
        matches!(
            self,
            SoarPreference::Better
                | SoarPreference::Worse
                | SoarPreference::BinaryIndifferent
                | SoarPreference::NumericIndifferent
        )
    }
}

/// A right-hand-side action (upstream `action`).
#[derive(Debug, Clone, PartialEq)]
pub enum SoarAction {
    /// `(<id> ^attr value [preference])`
    Make {
        /// Identifier variable.
        id: SoarSymbol,
        /// Attribute.
        attr: SoarRhsValue,
        /// Value.
        value: SoarRhsValue,
        /// Preference type.
        preference: SoarPreference,
        /// Referent of a binary preference.
        referent: Option<SoarRhsValue>,
    },
    /// A stand-alone function call such as `(write …)`.
    FunctionCall(SoarRhsValue),
}

/// Declared support flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SoarSupport {
    /// No flag.
    #[default]
    Undeclared,
    /// `:o-support`
    OSupport,
    /// `:i-support`
    ISupport,
}

/// Production type flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SoarProductionType {
    /// No flag.
    #[default]
    User,
    /// `:chunk`
    Chunk,
    /// `:default`
    Default,
    /// `:template`
    Template,
}

/// A parsed `sp` production.
#[derive(Debug, Clone, PartialEq)]
pub struct SoarProduction {
    /// Production name.
    pub name: String,
    /// Optional documentation string.
    pub documentation: Option<String>,
    /// `:o-support` / `:i-support`.
    pub support: SoarSupport,
    /// `:chunk` / `:default` / `:template`.
    pub production_type: SoarProductionType,
    /// `:interrupt`
    pub interrupt: bool,
    /// `:explain`
    pub explain: bool,
    /// Left-hand side.
    pub conditions: Vec<SoarCondition>,
    /// Right-hand side, in source order.
    pub actions: Vec<SoarAction>,
    /// Warnings upstream would print while parsing.
    pub warnings: Vec<String>,
}

/// Errors encountered while parsing `.soar` scripts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SoarParseError {
    /// Syntax error message.
    InvalidSyntax(String),
}

impl fmt::Display for SoarParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SoarParseError::InvalidSyntax(msg) => write!(f, "Soar Script Parse Error: {}", msg),
        }
    }
}

fn err<T>(message: impl Into<String>) -> Result<T, SoarParseError> {
    Err(SoarParseError::InvalidSyntax(message.into()))
}

// ======================================================================
// Lexer (upstream lexer.cpp)
// ======================================================================

#[derive(Debug, Clone, PartialEq)]
enum Lexeme {
    Eof,
    Identifier(char, u64),
    Variable(String),
    Str(String),
    Int(i64),
    Float(f64),
    Quoted(String),
    RightArrow,
    LParen,
    RParen,
    LBrace,
    RBrace,
    UpArrow,
    Comma,
    Period,
    Plus,
    Minus,
    Exclamation,
    Tilde,
    Equal,
    NotEqual,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    LessEqualGreater,
    LessLess,
    GreaterGreater,
    Ampersand,
    At,
    NotAt,
    UnaryAt,
    UnaryNotAt,
    Dollar,
    DoubleDollar,
}

fn is_constituent(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"$%&*+-/:<=>?_@".contains(&c)
}

fn is_space(c: Option<u8>) -> bool {
    c.is_some_and(|c| c.is_ascii_whitespace())
}

struct Lexer<'a> {
    src: &'a [u8],
    /// Index of the character after `current`.
    next: usize,
    current: Option<u8>,
    prev: Option<u8>,
    text: Vec<u8>,
    lexeme: Lexeme,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src: src.as_bytes(),
            next: 0,
            current: Some(b' '),
            prev: None,
            text: Vec::new(),
            lexeme: Lexeme::Eof,
        }
    }

    fn get_next_char(&mut self) {
        if self.current.is_none() {
            self.prev = None;
            return;
        }
        self.prev = self.current;
        self.current = self.src.get(self.next).copied().filter(|c| *c != 0);
        self.next += 1;
    }

    fn store_and_advance(&mut self) {
        if let Some(c) = self.current {
            self.text.push(c);
        }
        self.get_next_char();
    }

    fn read_constituent_string(&mut self) {
        while self.current.is_some_and(is_constituent) {
            self.store_and_advance();
        }
    }

    fn save(&self) -> (Vec<u8>, usize, Option<u8>, Option<u8>) {
        (self.text.clone(), self.next, self.current, self.prev)
    }

    fn restore(&mut self, state: (Vec<u8>, usize, Option<u8>, Option<u8>)) {
        (self.text, self.next, self.current, self.prev) = state;
    }

    fn read_rest_of_floating_point_number(&mut self) -> bool {
        let saved = self.save();
        self.store_and_advance();
        while self.current.is_some_and(|c| c.is_ascii_digit()) {
            self.store_and_advance();
        }
        if matches!(self.current, Some(b'e' | b'E')) {
            self.store_and_advance();
            if matches!(self.current, Some(b'+' | b'-')) {
                self.store_and_advance();
            }
            while self.current.is_some_and(|c| c.is_ascii_digit()) {
                self.store_and_advance();
            }
        }
        if self.current.is_some_and(is_constituent) && !is_space(self.prev) {
            self.restore(saved);
            return false;
        }
        true
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.text).into_owned()
    }

    /// Upstream `determine_type_of_constituent_string`.
    fn determine_type(&mut self) -> Result<(), SoarParseError> {
        let s = self.text();
        let bytes = s.as_bytes();
        let types = possible_symbol_types(bytes);
        self.lexeme = if types.var {
            Lexeme::Variable(s)
        } else if types.int {
            match s.parse::<i64>() {
                Ok(i) => Lexeme::Int(i),
                Err(_) => return err(format!("bad integer (probably too large): {}", s)),
            }
        } else if types.float {
            Lexeme::Float(parse_float(&s))
        } else if types.id {
            let letter = (bytes[0] as char).to_ascii_uppercase();
            match s[1..].parse::<u64>() {
                Ok(n) => Lexeme::Identifier(letter, n),
                Err(_) => return err(format!("bad number for identifier (probably too large): {}", s)),
            }
        } else if types.str {
            Lexeme::Str(s)
        } else {
            Lexeme::Quoted(s)
        };
        Ok(())
    }

    fn get_lexeme(&mut self) -> Result<(), SoarParseError> {
        // A period directly after a string or variable is a path separator, not a float.
        let last_was_str_or_var = matches!(self.lexeme, Lexeme::Str(_) | Lexeme::Variable(_));
        self.text.clear();
        self.consume_whitespace_and_comments();
        let Some(c) = self.current else {
            self.lexeme = Lexeme::Eof;
            return Ok(());
        };
        match c {
            b'@' => self.lex_at(),
            b'(' => self.single(Lexeme::LParen),
            b')' => self.single(Lexeme::RParen),
            b'+' => return self.lex_number_or(Lexeme::Plus),
            b'-' => return self.lex_number_or(Lexeme::Minus),
            b'~' => self.single(Lexeme::Tilde),
            b'^' => self.single(Lexeme::UpArrow),
            b'{' => self.single(Lexeme::LBrace),
            b'}' => self.single(Lexeme::RBrace),
            b'!' => {
                if self.src.get(self.next) == Some(&b'@') {
                    self.store_and_advance();
                    self.single(Lexeme::NotAt)
                } else {
                    self.single(Lexeme::Exclamation)
                }
            }
            b'>' => {
                self.read_constituent_string();
                match self.text.as_slice() {
                    b">" => self.lexeme = Lexeme::Greater,
                    b">>" => self.lexeme = Lexeme::GreaterGreater,
                    b">=" => self.lexeme = Lexeme::GreaterEqual,
                    _ => return self.determine_type(),
                }
            }
            b'<' => {
                self.read_constituent_string();
                match self.text.as_slice() {
                    b"<" => self.lexeme = Lexeme::Less,
                    b"<>" => self.lexeme = Lexeme::NotEqual,
                    b"<=" => self.lexeme = Lexeme::LessEqual,
                    b"<<" => self.lexeme = Lexeme::LessLess,
                    b"<=>" => self.lexeme = Lexeme::LessEqualGreater,
                    _ => return self.determine_type(),
                }
            }
            b'=' | b'&' => {
                self.read_constituent_string();
                if self.text.len() == 1 {
                    self.lexeme = if c == b'=' { Lexeme::Equal } else { Lexeme::Ampersand };
                } else {
                    return self.determine_type();
                }
            }
            b'|' => return self.lex_delimited(b'|'),
            b'"' => return self.lex_delimited(b'"'),
            b',' => self.single(Lexeme::Comma),
            b'.' => {
                let float_disallowed = !is_space(self.prev) && last_was_str_or_var;
                self.store_and_advance();
                if !float_disallowed && self.current.is_some_and(|c| c.is_ascii_digit()) {
                    self.read_rest_of_floating_point_number();
                }
                if self.text.len() == 1 {
                    self.lexeme = Lexeme::Period;
                } else {
                    return self.determine_type();
                }
            }
            b'$' => {
                self.store_and_advance();
                if self.current == Some(b'$') {
                    self.store_and_advance();
                    self.lexeme = Lexeme::DoubleDollar;
                } else {
                    self.lexeme = Lexeme::Dollar;
                }
            }
            c if c.is_ascii_digit() => return self.lex_number_or(Lexeme::Eof),
            c if is_constituent(c) => {
                self.read_constituent_string();
                return self.determine_type();
            }
            _ => {
                // Upstream `lex_unknown`: skip the character.
                self.get_next_char();
                return self.get_lexeme();
            }
        }
        Ok(())
    }

    fn single(&mut self, lexeme: Lexeme) {
        self.store_and_advance();
        self.lexeme = lexeme;
    }

    fn lex_at(&mut self) {
        let saved = self.save();
        self.read_constituent_string();
        match self.text.as_slice() {
            b"@+" => self.lexeme = Lexeme::UnaryAt,
            b"@-" => self.lexeme = Lexeme::UnaryNotAt,
            _ => {
                self.restore(saved);
                self.single(Lexeme::At);
            }
        }
    }

    /// Upstream `lex_plus`, `lex_minus` and `lex_digit`. `alone` is the lexeme for a lone
    /// `+` or `-`.
    fn lex_number_or(&mut self, alone: Lexeme) -> Result<(), SoarParseError> {
        self.read_constituent_string();
        if self.current == Some(b'.') && self.text[1..].iter().all(u8::is_ascii_digit) {
            self.read_rest_of_floating_point_number();
        }
        if self.text.len() == 1 && alone != Lexeme::Eof {
            self.lexeme = alone;
            return Ok(());
        }
        if self.text.as_slice() == b"-->" {
            self.lexeme = Lexeme::RightArrow;
            return Ok(());
        }
        self.determine_type()
    }

    /// Upstream `lex_vbar` / `lex_quote`: backslash escapes the next character.
    fn lex_delimited(&mut self, close: u8) -> Result<(), SoarParseError> {
        self.get_next_char();
        loop {
            match self.current {
                None => return err("unterminated string"),
                Some(b'\\') => {
                    self.get_next_char();
                    self.store_and_advance();
                }
                Some(c) if c == close => {
                    self.get_next_char();
                    break;
                }
                Some(_) => self.store_and_advance(),
            }
        }
        let s = self.text();
        self.lexeme = if close == b'|' { Lexeme::Str(s) } else { Lexeme::Quoted(s) };
        Ok(())
    }

    fn consume_whitespace_and_comments(&mut self) {
        loop {
            match self.current {
                Some(c) if c.is_ascii_whitespace() || c == b';' => self.get_next_char(),
                Some(b'#') => {
                    while !matches!(self.current, Some(b'\n') | None) {
                        self.get_next_char();
                    }
                    self.get_next_char();
                }
                _ => break,
            }
        }
    }
}

struct PossibleTypes {
    id: bool,
    var: bool,
    str: bool,
    int: bool,
    float: bool,
}

/// Upstream `determine_possible_symbol_types_for_string`.
fn possible_symbol_types(s: &[u8]) -> PossibleTypes {
    let mut types = PossibleTypes { id: false, var: false, str: false, int: false, float: false };
    if s.is_empty() {
        return types;
    }
    if matches!(s[0], b'+' | b'-' | b'.') || s[0].is_ascii_digit() {
        let mut i = usize::from(matches!(s[0], b'+' | b'-'));
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
        }
        if i == s.len() && i > 0 && s[i - 1].is_ascii_digit() {
            types.int = true;
        }
        if i < s.len() && s[i] == b'.' {
            i += 1;
            while i < s.len() && s[i].is_ascii_digit() {
                i += 1;
            }
            if i < s.len() && matches!(s[i], b'e' | b'E') {
                i += 1;
                if i < s.len() && matches!(s[i], b'+' | b'-') {
                    i += 1;
                }
                while i < s.len() && s[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i == s.len() {
                types.float = true;
            }
        }
    }
    if !s.iter().all(|c| is_constituent(*c)) {
        return types;
    }
    types.str = true;
    if s[0] == b'<' && s[s.len() - 1] == b'>' {
        types.var = true;
    }
    if s[0].is_ascii_alphabetic() && s.len() > 1 && s[1..].iter().all(u8::is_ascii_digit) {
        types.id = true;
    }
    types
}

/// Upstream `make_string_rereadable`: a string constant prints in `|bars|` (with `|` and `\`
/// escaped) unless it would read back as the same string constant.
pub(crate) fn rereadable_string(s: &str) -> String {
    let bytes = s.as_bytes();
    let types = possible_symbol_types(bytes);
    // Upstream `length_of_longest_special_lexeme` is 3; the lexer's rereadability test.
    let rereadable = types.str && (bytes.iter().all(u8::is_ascii_alphanumeric) || bytes.len() > 3 || s == "*");
    if !types.str || types.var || types.int || types.float || types.id || !rereadable {
        let mut escaped = String::from("|");
        for c in s.chars() {
            if c == '|' || c == '\\' {
                escaped.push('\\');
            }
            escaped.push(c);
        }
        escaped.push('|');
        return escaped;
    }
    s.to_string()
}

/// `strtod` semantics: parse the longest valid prefix (e.g. a dangling exponent is ignored).
fn parse_float(s: &str) -> f64 {
    let mut end = s.len();
    while end > 0 {
        if let Ok(value) = s[..end].parse::<f64>() {
            return value;
        }
        end -= 1;
    }
    0.0
}

// ======================================================================
// Parser (upstream parser.cpp)
// ======================================================================

/// Condition under construction: id/attr tests are filled in by the caller, as upstream.
#[derive(Debug, Clone)]
enum RawCondition {
    Positive(RawTests),
    Negative(RawTests),
    Ncc(Vec<RawCondition>),
}

#[derive(Debug, Clone, Default)]
struct RawTests {
    id: Option<SoarTest>,
    attr: Option<SoarTest>,
    value: Option<SoarTest>,
    acceptable: bool,
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    placeholder_counters: [u64; 26],
    warnings: Vec<String>,
}

impl<'a> Parser<'a> {
    fn current(&self) -> &Lexeme {
        &self.lexer.lexeme
    }

    fn advance(&mut self) -> Result<(), SoarParseError> {
        self.lexer.get_lexeme()
    }

    fn at(&self, lexeme: &Lexeme) -> bool {
        self.current() == lexeme
    }

    /// Upstream `make_placeholder_var`: a name no user variable can have.
    fn placeholder(&mut self, letter: char) -> SoarTest {
        let letter = if letter.is_ascii_alphabetic() { letter } else { 'v' };
        let index = (letter.to_ascii_lowercase() as u8 - b'a') as usize;
        let n = self.placeholder_counters[index];
        self.placeholder_counters[index] += 1;
        SoarTest::Relational(SoarRelation::Equal, SoarSymbol::Variable(format!("<#{}*{}>", letter, n)))
    }

    fn symbol_for_lexeme(&self) -> Option<SoarSymbol> {
        Some(match self.current() {
            Lexeme::Str(s) => SoarSymbol::Str(s.clone()),
            Lexeme::Variable(v) => SoarSymbol::Variable(v.clone()),
            Lexeme::Int(i) => SoarSymbol::Int(*i),
            Lexeme::Float(x) => SoarSymbol::Float(*x),
            Lexeme::Identifier(letter, n) => SoarSymbol::Str(format!("{}{}", letter, n)),
            _ => return None,
        })
    }

    // ---------------------------------------------------------------- tests

    fn parse_relational_test(&mut self) -> Result<SoarTest, SoarParseError> {
        let relation = match self.current() {
            Lexeme::Equal => Some(SoarRelation::Equal),
            Lexeme::NotEqual => Some(SoarRelation::NotEqual),
            Lexeme::Less => Some(SoarRelation::Less),
            Lexeme::Greater => Some(SoarRelation::Greater),
            Lexeme::LessEqual => Some(SoarRelation::LessOrEqual),
            Lexeme::GreaterEqual => Some(SoarRelation::GreaterOrEqual),
            Lexeme::LessEqualGreater => Some(SoarRelation::SameType),
            Lexeme::At => Some(SoarRelation::SmemLink),
            Lexeme::NotAt => Some(SoarRelation::SmemLinkNot),
            Lexeme::DoubleDollar => Some(SoarRelation::ConstantMatch),
            Lexeme::Dollar => {
                // `$x` is shorthand for `{ x $$x }`.
                self.advance()?;
                let Some(referent) = self.symbol_for_lexeme() else {
                    return err("Expected variable or constant after $ in shorthand test");
                };
                self.advance()?;
                return Ok(SoarTest::Conjunctive(vec![
                    SoarTest::Relational(SoarRelation::Equal, referent.clone()),
                    SoarTest::Relational(SoarRelation::ConstantMatch, referent),
                ]));
            }
            _ => None,
        };
        if relation.is_some() {
            self.advance()?;
        }
        let Some(referent) = self.symbol_for_lexeme() else {
            return err("Expected variable or constant for test");
        };
        self.advance()?;
        Ok(SoarTest::Relational(relation.unwrap_or(SoarRelation::Equal), referent))
    }

    fn parse_disjunction_test(&mut self) -> Result<SoarTest, SoarParseError> {
        self.advance()?;
        let mut values = Vec::new();
        while !self.at(&Lexeme::GreaterGreater) {
            match self.current() {
                Lexeme::Str(_) | Lexeme::Int(_) | Lexeme::Float(_) | Lexeme::Identifier(..) => {
                    values.push(self.symbol_for_lexeme().unwrap_or(SoarSymbol::Int(0)));
                    self.advance()?;
                }
                _ => return err("Expected constant or >> while reading disjunction test"),
            }
        }
        self.advance()?;
        Ok(SoarTest::Disjunction(values))
    }

    fn parse_simple_test(&mut self) -> Result<SoarTest, SoarParseError> {
        match self.current() {
            Lexeme::LessLess => self.parse_disjunction_test(),
            Lexeme::UnaryAt => {
                self.advance()?;
                Ok(SoarTest::SmemLinkUnary)
            }
            Lexeme::UnaryNotAt => {
                self.advance()?;
                Ok(SoarTest::SmemLinkUnaryNot)
            }
            _ => self.parse_relational_test(),
        }
    }

    fn parse_test(&mut self) -> Result<SoarTest, SoarParseError> {
        if !self.at(&Lexeme::LBrace) {
            return self.parse_simple_test();
        }
        self.advance()?;
        let mut t: Option<SoarTest> = None;
        loop {
            let temp = self.parse_simple_test()?;
            t = Some(match t {
                Some(existing) => match (existing.eq_test().cloned(), temp.eq_test().cloned()) {
                    (Some(old), Some(new)) => {
                        self.warnings.push("Soar does not support having two equality tests in one conjunctive test!".into());
                        // Upstream keeps a constant over a variable when both are plain equality tests.
                        let both_plain = matches!(existing, SoarTest::Relational(..)) && matches!(temp, SoarTest::Relational(..));
                        if both_plain && old.is_variable() && !new.is_variable() {
                            self.warnings.push(format!("Ignoring {} in favor of constant {}.  Rule semantics may have changed!", old, new));
                            temp
                        } else {
                            self.warnings.push(format!("Ignoring {} in favor of existing {}.  Rule semantics may have changed!", new, old));
                            existing
                        }
                    }
                    _ => existing.add(temp),
                },
                None => temp,
            });
            if self.at(&Lexeme::RBrace) {
                break;
            }
            if self.at(&Lexeme::Eof) {
                return err("Expected } to end conjunctive test");
            }
        }
        self.advance()?;
        Ok(t.unwrap_or(SoarTest::Conjunctive(Vec::new())))
    }

    // ----------------------------------------------------------- conditions

    fn ends_value_tests(&self) -> bool {
        matches!(self.current(), Lexeme::Minus | Lexeme::UpArrow | Lexeme::RParen)
    }

    fn parse_value_test_star(&mut self, first_letter: char) -> Result<Vec<RawCondition>, SoarParseError> {
        if self.ends_value_tests() {
            let value = self.placeholder(first_letter);
            return Ok(vec![RawCondition::Positive(RawTests { value: Some(value), ..RawTests::default() })]);
        }
        let mut all = Vec::new();
        loop {
            let (value_test, mut new_conds) = if self.at(&Lexeme::LParen) {
                let (conds, id_test) = self.parse_conds_for_one_id(first_letter, true)?;
                (id_test.unwrap_or(SoarTest::Goal), conds)
            } else {
                let mut test = self.parse_test()?;
                if test.eq_test().is_none() {
                    test = test.add(self.placeholder(first_letter));
                }
                (test, Vec::new())
            };
            let acceptable = self.at(&Lexeme::Plus);
            if acceptable {
                self.advance()?;
            }
            new_conds.insert(
                0,
                RawCondition::Positive(RawTests { value: Some(value_test), acceptable, ..RawTests::default() }),
            );
            all.extend(new_conds);
            if self.ends_value_tests() {
                break;
            }
            if self.at(&Lexeme::Eof) {
                return err("Unexpected end of production in value tests");
            }
        }
        Ok(all)
    }

    fn parse_attr_value_tests(&mut self) -> Result<Vec<RawCondition>, SoarParseError> {
        let negate = self.at(&Lexeme::Minus);
        if negate {
            self.advance()?;
        }
        if !self.at(&Lexeme::UpArrow) {
            return err("Expected ^ followed by attribute");
        }
        self.advance()?;

        let mut attr_test = self.parse_test()?;
        if attr_test.eq_test().is_none() {
            attr_test = attr_test.add(self.placeholder('a'));
        }

        let mut conds = Vec::new();
        let mut id_test_to_use: Option<SoarTest> = None;
        while self.at(&Lexeme::Period) {
            self.advance()?;
            let value = self.placeholder(attr_test.first_letter());
            conds.push(RawCondition::Positive(RawTests {
                id: id_test_to_use.clone(),
                attr: Some(attr_test),
                value: Some(value.clone()),
                acceptable: false,
            }));
            id_test_to_use = Some(value);
            attr_test = self.parse_test()?;
            if attr_test.eq_test().is_none() {
                attr_test = attr_test.add(self.placeholder('a'));
            }
        }

        let mut new_conds = self.parse_value_test_star(attr_test.first_letter())?;
        fill_in_attr_tests(&mut new_conds, &attr_test);
        if let Some(id_test) = &id_test_to_use {
            fill_in_id_tests(&mut new_conds, id_test);
        }
        conds.extend(new_conds);

        Ok(if negate { negate_condition_list(conds) } else { conds })
    }

    /// Returns the conditions and, when `return_id_test` is set, the complete id test (the
    /// conditions then carry only its equality part).
    fn parse_conds_for_one_id(
        &mut self,
        first_letter: char,
        return_id_test: bool,
    ) -> Result<(Vec<RawCondition>, Option<SoarTest>), SoarParseError> {
        // --- head: ( [state|impasse] [<id_test>]
        if !self.at(&Lexeme::LParen) {
            return err("Expected ( to begin condition element");
        }
        self.advance()?;
        let mut first_letter = first_letter;
        let goal_test = match self.current() {
            Lexeme::Str(s) if s == "state" => Some(SoarTest::Goal),
            Lexeme::Str(s) if s == "impasse" => Some(SoarTest::Impasse),
            _ => None,
        };
        if let Some(test) = &goal_test {
            first_letter = if *test == SoarTest::Goal { 's' } else { 'i' };
            self.advance()?;
        }
        let mut id_test = if !self.ends_value_tests() {
            let test = self.parse_test()?;
            match test.eq_test() {
                None => test.add(self.placeholder(first_letter)),
                Some(symbol) if !symbol.is_variable() => {
                    return err(format!("Constant {} in id field test. This will never match.", symbol));
                }
                Some(_) => test,
            }
        } else {
            self.placeholder(first_letter)
        };
        if let Some(test) = goal_test.clone() {
            id_test = id_test.add(test);
        }

        // --- tail: <attr_value_tests>* )
        let mut conds = Vec::new();
        if self.at(&Lexeme::RParen) {
            if goal_test.is_some() {
                return err(
                    "Expected attribute-value test after state/impasse test. Did you forget to add \"^type state\" or \"^superstate nil\"?",
                );
            }
            self.advance()?;
            let attr = self.placeholder('a');
            let value = self.placeholder('v');
            conds.push(RawCondition::Positive(RawTests { attr: Some(attr), value: Some(value), ..RawTests::default() }));
        } else {
            while !self.at(&Lexeme::RParen) {
                if self.at(&Lexeme::Eof) {
                    return err("Expected ) to end condition");
                }
                conds.extend(self.parse_attr_value_tests()?);
            }
            self.advance()?;
        }

        if return_id_test {
            let eq = SoarTest::Relational(SoarRelation::Equal, id_test.eq_test().cloned().unwrap_or(SoarSymbol::Int(0)));
            fill_in_id_tests(&mut conds, &eq);
            Ok((conds, Some(id_test)))
        } else {
            fill_in_id_tests(&mut conds, &id_test);
            Ok((conds, None))
        }
    }

    fn parse_cond(&mut self) -> Result<Vec<RawCondition>, SoarParseError> {
        let negate = self.at(&Lexeme::Minus);
        if negate {
            self.advance()?;
        }
        let conds = if self.at(&Lexeme::LBrace) {
            self.advance()?;
            let conds = self.parse_cond_plus()?;
            if !self.at(&Lexeme::RBrace) {
                return err("Expected } to end conjunctive condition");
            }
            self.advance()?;
            conds
        } else {
            self.parse_conds_for_one_id('s', false)?.0
        };
        Ok(if negate { negate_condition_list(conds) } else { conds })
    }

    fn parse_cond_plus(&mut self) -> Result<Vec<RawCondition>, SoarParseError> {
        let mut conds = Vec::new();
        loop {
            conds.extend(self.parse_cond()?);
            if !matches!(self.current(), Lexeme::Minus | Lexeme::LParen | Lexeme::LBrace) {
                return Ok(conds);
            }
        }
    }

    // -------------------------------------------------------------- actions

    fn parse_function_call_after_lparen(&mut self) -> Result<SoarRhsValue, SoarParseError> {
        let name = match self.current() {
            Lexeme::Plus => "+".to_string(),
            Lexeme::Minus => "-".to_string(),
            Lexeme::At => "@".to_string(),
            Lexeme::Str(s) => s.clone(),
            Lexeme::Identifier(letter, n) => format!("{}{}", letter, n),
            _ => return err("Expected function name after ("),
        };
        // Upstream maps the legacy `succeeded`/`failed` functions onto `halt`.
        let name = if name == "succeeded" || name == "failed" { "halt".to_string() } else { name };
        self.advance()?;
        let mut args = Vec::new();
        while !self.at(&Lexeme::RParen) {
            if self.at(&Lexeme::Eof) {
                return err(format!("Expected ) to end call to {}", name));
            }
            args.push(self.parse_rhs_value()?);
        }
        self.advance()?;
        Ok(SoarRhsValue::FunctionCall { name, args })
    }

    fn parse_rhs_value(&mut self) -> Result<SoarRhsValue, SoarParseError> {
        if self.at(&Lexeme::LParen) {
            self.advance()?;
            return self.parse_function_call_after_lparen();
        }
        let Some(symbol) = self.symbol_for_lexeme() else {
            return err("Illegal value for RHS value");
        };
        self.advance()?;
        Ok(SoarRhsValue::Symbol(symbol))
    }

    fn is_preference_lexeme(&self) -> bool {
        matches!(
            self.current(),
            Lexeme::Plus | Lexeme::Minus | Lexeme::Exclamation | Lexeme::Tilde | Lexeme::Greater | Lexeme::Equal | Lexeme::Less | Lexeme::Ampersand
        )
    }

    fn skip_comma(&mut self) -> Result<(), SoarParseError> {
        if self.at(&Lexeme::Comma) {
            self.advance()?;
        }
        Ok(())
    }

    /// Upstream `parse_preference_specifier_without_referent`.
    fn parse_preference_specifier(&mut self) -> Result<SoarPreference, SoarParseError> {
        let unary = |p: &mut Self, pref| -> Result<SoarPreference, SoarParseError> {
            p.advance()?;
            p.skip_comma()?;
            Ok(pref)
        };
        let forced_unary = |p: &Self| {
            matches!(p.current(), Lexeme::Comma | Lexeme::RParen | Lexeme::UpArrow) || p.is_preference_lexeme()
        };
        match self.current() {
            Lexeme::Plus => unary(self, SoarPreference::Acceptable),
            Lexeme::Minus => unary(self, SoarPreference::Reject),
            Lexeme::Exclamation => unary(self, SoarPreference::Require),
            Lexeme::Tilde => unary(self, SoarPreference::Prohibit),
            Lexeme::Greater | Lexeme::Less | Lexeme::Equal => {
                let kind = self.current().clone();
                self.advance()?;
                if !forced_unary(self) {
                    return Ok(match kind {
                        Lexeme::Greater => SoarPreference::Better,
                        Lexeme::Less => SoarPreference::Worse,
                        _ if matches!(self.current(), Lexeme::Int(_) | Lexeme::Float(_)) => SoarPreference::NumericIndifferent,
                        _ => SoarPreference::BinaryIndifferent,
                    });
                }
                self.skip_comma()?;
                Ok(match kind {
                    Lexeme::Greater => SoarPreference::Best,
                    Lexeme::Less => SoarPreference::Worst,
                    _ => SoarPreference::UnaryIndifferent,
                })
            }
            _ => Ok(SoarPreference::Acceptable),
        }
    }

    /// Upstream `parse_preferences` (operator attributes): one make action per preference.
    fn parse_preferences(
        &mut self,
        id: &SoarSymbol,
        attr: &SoarRhsValue,
        value: &SoarRhsValue,
    ) -> Result<Vec<SoarAction>, SoarParseError> {
        let mut actions = Vec::new();
        let mut saw_plus = self.at(&Lexeme::Plus);
        let mut preference = self.parse_preference_specifier()?;
        if preference == SoarPreference::Acceptable && !saw_plus {
            self.skip_comma()?;
        }
        loop {
            let referent = if preference.is_binary() {
                let referent = self.parse_rhs_value()?;
                self.skip_comma()?;
                Some(referent)
            } else {
                None
            };
            actions.push(SoarAction::Make { id: id.clone(), attr: attr.clone(), value: value.clone(), preference, referent });
            saw_plus = self.at(&Lexeme::Plus);
            preference = self.parse_preference_specifier()?;
            if preference == SoarPreference::Acceptable && !saw_plus {
                return Ok(actions);
            }
        }
    }

    /// Upstream `parse_preferences_soar8_non_operator`: only acceptable and reject are allowed.
    fn parse_preferences_non_operator(
        &mut self,
        id: &SoarSymbol,
        attr: &SoarRhsValue,
        value: &SoarRhsValue,
    ) -> Result<Vec<SoarAction>, SoarParseError> {
        let mut actions = Vec::new();
        let mut saw_plus = self.at(&Lexeme::Plus);
        let mut preference = self.parse_preference_specifier()?;
        if preference == SoarPreference::Acceptable && !saw_plus {
            self.skip_comma()?;
        }
        loop {
            if preference.is_binary() {
                return err(format!("Binary preference illegal for non-operator. id = {}", id));
            }
            if preference != SoarPreference::Acceptable && preference != SoarPreference::Reject {
                self.warnings.push(format!(
                    "WARNING: The only allowable non-operator preference is REJECT - . Ignoring specified preferences. id = {}",
                    id
                ));
            }
            if preference == SoarPreference::Reject {
                actions.push(SoarAction::Make {
                    id: id.clone(),
                    attr: attr.clone(),
                    value: value.clone(),
                    preference,
                    referent: None,
                });
            }
            saw_plus = self.at(&Lexeme::Plus);
            preference = self.parse_preference_specifier()?;
            if preference == SoarPreference::Acceptable && !saw_plus {
                if actions.is_empty() {
                    actions.push(SoarAction::Make {
                        id: id.clone(),
                        attr: attr.clone(),
                        value: value.clone(),
                        preference: SoarPreference::Acceptable,
                        referent: None,
                    });
                }
                return Ok(actions);
            }
        }
    }

    fn parse_value_preferences(
        &mut self,
        id: &SoarSymbol,
        attr: &SoarRhsValue,
        value: &SoarRhsValue,
    ) -> Result<Vec<SoarAction>, SoarParseError> {
        if *attr == SoarRhsValue::Symbol(SoarSymbol::Str("operator".into())) {
            self.parse_preferences(id, attr, value)
        } else {
            self.parse_preferences_non_operator(id, attr, value)
        }
    }

    fn parse_attr_value_make(&mut self, id: &SoarSymbol) -> Result<Vec<SoarAction>, SoarParseError> {
        if !self.at(&Lexeme::UpArrow) {
            return err("Expected ^ in RHS make action");
        }
        self.advance()?;
        let mut id = id.clone();
        let mut attr = self.parse_rhs_value()?;
        let mut actions = Vec::new();
        while self.at(&Lexeme::Period) {
            self.advance()?;
            let letter = match &attr {
                SoarRhsValue::Symbol(symbol) => symbol.first_letter(),
                SoarRhsValue::FunctionCall { .. } => '*',
            };
            let SoarTest::Relational(_, new_var) = self.placeholder(letter) else { unreachable!() };
            let value = SoarRhsValue::Symbol(new_var.clone());
            actions.extend(self.parse_value_preferences(&id, &attr, &value)?);
            id = new_var;
            attr = self.parse_rhs_value()?;
        }
        loop {
            let value = self.parse_rhs_value()?;
            actions.extend(self.parse_value_preferences(&id, &attr, &value)?);
            if matches!(self.current(), Lexeme::RParen | Lexeme::UpArrow) {
                return Ok(actions);
            }
            if self.at(&Lexeme::Eof) {
                return err("Expected ) to end RHS action");
            }
        }
    }

    fn parse_rhs_action(&mut self) -> Result<Vec<SoarAction>, SoarParseError> {
        if !self.at(&Lexeme::LParen) {
            return err("Expected ( to begin RHS action");
        }
        self.advance()?;
        let id = match self.current() {
            Lexeme::Variable(v) => SoarSymbol::Variable(v.clone()),
            Lexeme::Identifier(letter, n) => SoarSymbol::Variable(format!("{}{}", letter, n)),
            _ => return Ok(vec![SoarAction::FunctionCall(self.parse_function_call_after_lparen()?)]),
        };
        self.advance()?;
        let mut actions = Vec::new();
        while !self.at(&Lexeme::RParen) {
            if self.at(&Lexeme::Eof) {
                return err("Expected ) to end RHS action");
            }
            actions.extend(self.parse_attr_value_make(&id)?);
        }
        self.advance()?;
        Ok(actions)
    }

    // ----------------------------------------------------------- production

    fn parse_production(source: &'a str) -> Result<SoarProduction, SoarParseError> {
        let mut p = Parser { lexer: Lexer::new(source), placeholder_counters: [1; 26], warnings: Vec::new() };
        p.advance()?;

        let name = match p.current() {
            Lexeme::Str(s) => s.clone(),
            Lexeme::Identifier(letter, n) => format!("{}{}", letter, n),
            _ => return err("Expected symbol for production name"),
        };
        p.advance()?;

        let documentation = match p.current() {
            Lexeme::Quoted(doc) => {
                let doc = doc.clone();
                p.advance()?;
                Some(doc)
            }
            _ => None,
        };

        let mut support = SoarSupport::Undeclared;
        let mut production_type = SoarProductionType::User;
        let mut interrupt = false;
        let mut explain = false;
        while let Lexeme::Str(flag) = p.current() {
            match flag.as_str() {
                ":o-support" => support = SoarSupport::OSupport,
                ":i-support" => support = SoarSupport::ISupport,
                ":chunk" => production_type = SoarProductionType::Chunk,
                ":default" => production_type = SoarProductionType::Default,
                ":template" => production_type = SoarProductionType::Template,
                ":interrupt" => interrupt = true,
                ":explain" => explain = true,
                _ => break,
            }
            p.advance()?;
        }

        let raw = p.parse_cond_plus().map_err(|e| in_production(&name, e))?;
        if !p.at(&Lexeme::RightArrow) {
            return Err(in_production(&name, SoarParseError::InvalidSyntax("Expected --> in production".into())));
        }
        p.advance()?;

        let mut actions = Vec::new();
        while !p.at(&Lexeme::Eof) {
            actions.extend(p.parse_rhs_action().map_err(|e| in_production(&name, e))?);
        }

        let mut conditions: Vec<SoarCondition> = raw.into_iter().map(finish_condition).collect();
        substitute_placeholders(&mut conditions, &mut actions);

        Ok(SoarProduction {
            name,
            documentation,
            support,
            production_type,
            interrupt,
            explain,
            conditions,
            actions,
            warnings: p.warnings,
        })
    }
}

fn in_production(name: &str, error: SoarParseError) -> SoarParseError {
    let SoarParseError::InvalidSyntax(message) = error;
    SoarParseError::InvalidSyntax(format!("(in production {}) {}", name, message))
}

/// Upstream `fill_in_id_tests`: blank id tests get the equality part of `t`, and the first
/// positive condition with a blank id test gets all of `t`.
fn fill_in_id_tests(conds: &mut [RawCondition], t: &SoarTest) {
    fill_in(conds, t, &|tests: &mut RawTests| &mut tests.id);
}

/// Upstream `fill_in_attr_tests`.
fn fill_in_attr_tests(conds: &mut [RawCondition], t: &SoarTest) {
    fill_in(conds, t, &|tests: &mut RawTests| &mut tests.attr);
}

fn fill_in(conds: &mut [RawCondition], t: &SoarTest, field: &dyn Fn(&mut RawTests) -> &mut Option<SoarTest>) {
    let positive = conds.iter_mut().position(|c| match c {
        RawCondition::Positive(tests) => field(tests).is_none(),
        _ => false,
    });
    match positive {
        Some(index) => {
            let eq = SoarTest::Relational(SoarRelation::Equal, t.eq_test().cloned().unwrap_or(SoarSymbol::Int(0)));
            for c in conds.iter_mut() {
                match c {
                    RawCondition::Ncc(sub) => fill_in(sub, &eq, field),
                    RawCondition::Positive(tests) | RawCondition::Negative(tests) => {
                        let slot = field(tests);
                        if slot.is_none() {
                            *slot = Some(eq.clone());
                        }
                    }
                }
            }
            if let RawCondition::Positive(tests) = &mut conds[index] {
                *field(tests) = Some(t.clone());
            }
        }
        None => {
            for c in conds.iter_mut() {
                match c {
                    RawCondition::Ncc(sub) => fill_in(sub, t, field),
                    RawCondition::Positive(tests) | RawCondition::Negative(tests) => {
                        let slot = field(tests);
                        if slot.is_none() {
                            *slot = Some(t.clone());
                        }
                    }
                }
            }
        }
    }
}

/// Upstream `negate_condition_list`.
fn negate_condition_list(mut conds: Vec<RawCondition>) -> Vec<RawCondition> {
    if conds.len() == 1 {
        return match conds.pop() {
            Some(RawCondition::Positive(tests)) => vec![RawCondition::Negative(tests)],
            Some(RawCondition::Negative(tests)) => vec![RawCondition::Positive(tests)],
            Some(RawCondition::Ncc(sub)) => sub,
            None => Vec::new(),
        };
    }
    vec![RawCondition::Ncc(conds)]
}

fn finish_condition(raw: RawCondition) -> SoarCondition {
    let finish = |tests: RawTests| SoarConditionTests {
        id: tests.id.unwrap_or(SoarTest::Conjunctive(Vec::new())),
        attr: tests.attr.unwrap_or(SoarTest::Conjunctive(Vec::new())),
        value: tests.value.unwrap_or(SoarTest::Conjunctive(Vec::new())),
        acceptable: tests.acceptable,
    };
    match raw {
        RawCondition::Positive(tests) => SoarCondition::Positive(finish(tests)),
        RawCondition::Negative(tests) => SoarCondition::Negative(finish(tests)),
        RawCondition::Ncc(sub) => SoarCondition::ConjunctiveNegation(sub.into_iter().map(finish_condition).collect()),
    }
}

// ---------------------------------------------------------------------
// Placeholder substitution (upstream `substitute_for_placeholders_*` and
// `generate_new_variable`)
// ---------------------------------------------------------------------

fn substitute_placeholders(conditions: &mut [SoarCondition], actions: &mut [SoarAction]) {
    let mut used = Vec::new();
    for c in conditions.iter() {
        collect_condition_vars(c, &mut used);
    }
    for a in actions.iter() {
        collect_action_vars(a, &mut used);
    }
    let mut renames: Vec<(String, String)> = Vec::new();
    let mut counters = [1u64; 26];
    let mut rename = |name: &mut String| {
        if !name.starts_with("<#") {
            return;
        }
        if let Some((_, new)) = renames.iter().find(|(old, _)| old == name) {
            *name = new.clone();
            return;
        }
        let letter = name.chars().nth(2).filter(char::is_ascii_alphabetic).unwrap_or('v').to_ascii_lowercase();
        let index = (letter as u8 - b'a') as usize;
        let new = loop {
            let candidate = format!("<{}{}>", letter, counters[index]);
            counters[index] += 1;
            if !used.contains(&candidate) {
                break candidate;
            }
        };
        used.push(new.clone());
        renames.push((name.clone(), new.clone()));
        *name = new;
    };
    for c in conditions.iter_mut() {
        visit_condition_vars(c, &mut rename);
    }
    for a in actions.iter_mut() {
        visit_action_vars(a, &mut rename);
    }
}

fn collect_condition_vars(c: &SoarCondition, out: &mut Vec<String>) {
    let mut c = c.clone();
    visit_condition_vars(&mut c, &mut |name: &mut String| {
        if !out.contains(name) {
            out.push(name.clone());
        }
    });
}

fn collect_action_vars(a: &SoarAction, out: &mut Vec<String>) {
    let mut a = a.clone();
    visit_action_vars(&mut a, &mut |name: &mut String| {
        if !out.contains(name) {
            out.push(name.clone());
        }
    });
}

fn visit_symbol(symbol: &mut SoarSymbol, f: &mut dyn FnMut(&mut String)) {
    if let SoarSymbol::Variable(name) = symbol {
        f(name);
    }
}

fn visit_test_vars(test: &mut SoarTest, f: &mut dyn FnMut(&mut String)) {
    match test {
        SoarTest::Relational(_, symbol) => visit_symbol(symbol, f),
        SoarTest::Conjunctive(tests) => tests.iter_mut().for_each(|t| visit_test_vars(t, f)),
        _ => {}
    }
}

fn visit_condition_vars(c: &mut SoarCondition, f: &mut dyn FnMut(&mut String)) {
    match c {
        SoarCondition::Positive(t) | SoarCondition::Negative(t) => {
            visit_test_vars(&mut t.id, f);
            visit_test_vars(&mut t.attr, f);
            visit_test_vars(&mut t.value, f);
        }
        SoarCondition::ConjunctiveNegation(sub) => sub.iter_mut().for_each(|c| visit_condition_vars(c, f)),
    }
}

fn visit_rhs_vars(value: &mut SoarRhsValue, f: &mut dyn FnMut(&mut String)) {
    match value {
        SoarRhsValue::Symbol(symbol) => visit_symbol(symbol, f),
        SoarRhsValue::FunctionCall { args, .. } => args.iter_mut().for_each(|a| visit_rhs_vars(a, f)),
    }
}

fn visit_action_vars(a: &mut SoarAction, f: &mut dyn FnMut(&mut String)) {
    match a {
        SoarAction::Make { id, attr, value, referent, .. } => {
            visit_symbol(id, f);
            visit_rhs_vars(attr, f);
            visit_rhs_vars(value, f);
            if let Some(r) = referent {
                visit_rhs_vars(r, f);
            }
        }
        SoarAction::FunctionCall(call) => visit_rhs_vars(call, f),
    }
}

// ======================================================================
// smem --add (upstream smem_cli_commands.cpp)
// ======================================================================

/// A long-term identifier as `smem --add` names it.
#[derive(Debug, Clone, PartialEq)]
pub enum SmemAddRef {
    /// `<v>`: a new identifier, the same one wherever the command uses the variable.
    Variable(String),
    /// `@5`: the identifier with that number.
    Lti(u64),
    /// `@name`: the identifier with that alias, made when the command is parsed.
    Alias(String),
}

/// A value in an `smem --add` clause.
#[derive(Debug, Clone, PartialEq)]
pub enum SmemAddValue {
    /// A constant.
    Constant(SoarSymbol),
    /// A long-term identifier.
    Ref(SmemAddRef),
}

/// One attribute and its values. With a path (`^a.b.c v`), each `.` makes a new identifier: the
/// clause's identifier gets `^a <i1>`, `<i1>` gets `^b <i2>` and `<i2>` gets `^c v`.
#[derive(Debug, Clone, PartialEq)]
pub struct SmemAddSlot {
    /// The attributes of the path; the last one takes the values.
    pub path: Vec<SoarSymbol>,
    /// The values, in order.
    pub values: Vec<SmemAddValue>,
}

/// One `(id ^attr value ...)` clause of `smem --add`.
#[derive(Debug, Clone, PartialEq)]
pub struct SmemAddClause {
    /// The identifier.
    pub id: SmemAddRef,
    /// The attributes in order.
    pub slots: Vec<SmemAddSlot>,
}

/// Upstream `CLI_add` and `parse_add_clause`: parses the clauses of `smem --add`. Edge weights
/// (`v (0.5)`) are accepted and ignored, as they only matter for spreading activation.
fn parse_smem_add(text: &str) -> Result<Vec<SmemAddClause>, SoarParseError> {
    let mut lexer = Lexer::new(text);
    lexer.get_lexeme()?;
    let mut clauses = Vec::new();
    let error = |count: usize| SoarParseError::InvalidSyntax(format!("Error parsing clause #{}", count));
    if lexer.lexeme != Lexeme::LParen {
        return Err(error(0));
    }
    while lexer.lexeme == Lexeme::LParen {
        clauses.push(parse_smem_add_clause(&mut lexer)?.ok_or_else(|| error(clauses.len()))?);
    }
    Ok(clauses)
}

/// An `@5` or `@name` reference, the lexer at the `@`.
fn parse_smem_at(lexer: &mut Lexer) -> Result<Option<SmemAddRef>, SoarParseError> {
    lexer.get_lexeme()?;
    Ok(match &lexer.lexeme {
        Lexeme::Int(i) => Some(SmemAddRef::Lti(*i as u64)),
        Lexeme::Str(s) => Some(SmemAddRef::Alias(s.clone())),
        _ => None,
    })
}

fn parse_smem_add_clause(lexer: &mut Lexer) -> Result<Option<SmemAddClause>, SoarParseError> {
    lexer.get_lexeme()?;
    let id = match &lexer.lexeme {
        Lexeme::Variable(v) => SmemAddRef::Variable(v.clone()),
        Lexeme::At => match parse_smem_at(lexer)? {
            Some(id) => id,
            None => return Ok(None),
        },
        _ => return Ok(None),
    };
    lexer.get_lexeme()?;
    let attr = |lexeme: &Lexeme| match lexeme {
        Lexeme::Str(s) => Some(SoarSymbol::Str(s.clone())),
        Lexeme::Int(i) => Some(SoarSymbol::Int(*i)),
        Lexeme::Float(x) => Some(SoarSymbol::Float(*x)),
        _ => None,
    };
    let mut slots = Vec::new();
    let mut closed = false;
    while lexer.lexeme == Lexeme::UpArrow && !closed {
        lexer.get_lexeme()?;
        let Some(first) = attr(&lexer.lexeme) else { return Ok(None) };
        let mut path = alloc::vec![first];
        lexer.get_lexeme()?;
        while lexer.lexeme == Lexeme::Period {
            lexer.get_lexeme()?;
            let Some(next) = attr(&lexer.lexeme) else { return Ok(None) };
            path.push(next);
            lexer.get_lexeme()?;
        }
        let mut values = Vec::new();
        loop {
            let value = match &lexer.lexeme {
                Lexeme::Str(s) => SmemAddValue::Constant(SoarSymbol::Str(s.clone())),
                Lexeme::Identifier(..) => SmemAddValue::Constant(SoarSymbol::Str(lexer.text())),
                Lexeme::Int(i) => SmemAddValue::Constant(SoarSymbol::Int(*i)),
                Lexeme::Float(x) => SmemAddValue::Constant(SoarSymbol::Float(*x)),
                Lexeme::Variable(v) => SmemAddValue::Ref(SmemAddRef::Variable(v.clone())),
                Lexeme::At => match parse_smem_at(lexer)? {
                    Some(r) => SmemAddValue::Ref(r),
                    None => return Ok(None),
                },
                _ => break,
            };
            values.push(value);
            lexer.get_lexeme()?;
            if lexer.lexeme == Lexeme::LParen {
                // An edge weight.
                lexer.get_lexeme()?;
                lexer.get_lexeme()?;
                if lexer.lexeme == Lexeme::RParen {
                    lexer.get_lexeme()?;
                }
            }
            if lexer.lexeme == Lexeme::RParen {
                lexer.get_lexeme()?;
                closed = true;
                break;
            }
        }
        if values.is_empty() {
            return Ok(None);
        }
        slots.push(SmemAddSlot { path, values });
    }
    Ok(closed.then_some(SmemAddClause { id, slots }))
}

// ======================================================================
// Script level (local extension)
// ======================================================================

/// Container for a set of parsed Soar production rules.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SoarScript {
    /// List of parsed rules in the script.
    pub productions: Vec<SoarProduction>,
    /// The script's other commands, each with the number of productions before it.
    pub commands: Vec<(usize, ScriptCommand)>,
}

/// A command other than `sp` that a script may contain. Scripts with any other command fail
/// to parse, rather than run differently from upstream without saying so.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptCommand {
    /// `rl --set <name> <value>` (`-s`).
    Rl(crate::rl::RlSetting),
    /// `smem --set <name> <value>` (`-s`), and `smem --enable` / `--disable` (`-e` / `-d`,
    /// `--on` / `--off`), which set `learning`.
    Smem(crate::smem::SmemSetting),
    /// `chunk always|never|only|except` and `chunk <setting> <value>`.
    Chunk(crate::learning::ChunkSetting),
    /// `chunk singleton <id> <attr> <value>`, and `chunk singleton -r` to remove the pattern.
    ChunkSingleton {
        /// The kind of identifier, unless removing.
        pattern: Option<(crate::learning::SingletonElement, crate::learning::SingletonElement)>,
        /// The attribute.
        attr: String,
    },
    /// `epmem --set <name> <value>` (`-s`), and `epmem --enable` / `--disable` (`-e` / `-d`,
    /// `--on` / `--off`), which set `learning`.
    Epmem(crate::epmem::EpmemSetting),
    /// `decide indifferent-selection --boltzmann` etc. (aliases `indifferent-selection`,
    /// `inds`).
    ExplorationPolicy(crate::preference::ExplorationPolicy),
    /// `decide indifferent-selection --epsilon <value>`.
    Epsilon(f64),
    /// `decide indifferent-selection --temperature <value>`.
    Temperature(f64),
    /// `decide numeric-indifferent-mode --avg | --sum` (alias `numeric-indifferent-mode`).
    NumericIndifferentMode(crate::preference::NumericIndifferentMode),
    /// `decide srand <seed>` (`set-random-seed`; alias `srand`).
    SetRandomSeed(u32),
    /// `echo [--nonewline] <words>`.
    Echo { text: String, newline: bool },
    /// `soar wait-snc <on|off>` (alias `waitsnc`): a state no-change waits instead of making a
    /// substate.
    WaitSnc(bool),
    /// `smem --add {<clauses>}` (`-a`): adds structures to semantic memory.
    SmemAdd(Vec<SmemAddClause>),
    /// `watch <level>` / `trace <level>` / `trace --level <level>`: output only. Level 0 turns
    /// the decision trace off; any higher level turns it on, but only level 1's output (the
    /// decision trace) is produced.
    Trace(u8),
    /// `wma --set <name> <value>` (`wm activation --set`; `-s`).
    Wma(crate::wma::WmaSetting),
    /// `wma`, `wma --get <name>` (`-g`), `wma --history <timetag>` (`-h`) and `wma --stats
    /// [<name>]` (`-S`).
    WmaQuery(crate::wma::WmaQuery),
    /// `trace --wma [remove|0]` (`-a`): traces activation and forgetting.
    TraceWma(bool),
    /// `trace --epmem`, `--smem` or `--rl` (`-e`, `-s`, `-R`), with `remove` or `0` to turn it
    /// off: accepted, but Rusty-Soar does not print these traces.
    TraceUnprinted(String),
}

impl SoarScript {
    /// Parses a `.soar` script: every `sp {…}` / `sp "…"` command is parsed as a production;
    /// other commands are skipped.
    pub fn parse(script_content: &str) -> Result<Self, SoarParseError> {
        let expanded = expand_generated_rules(script_content.trim_start_matches('\u{feff}'))?;
        let mut productions = Vec::new();
        let mut commands = Vec::new();
        for command in script_commands(&expanded)? {
            match command {
                RawCommand::Sp(body) => productions.push(Parser::parse_production(&body)?),
                RawCommand::Other(text) => commands.push((productions.len(), parse_command(&text)?)),
            }
        }
        Ok(SoarScript { productions, commands })
    }

    /// Parses the body of a single `sp` command (the text between the braces).
    pub fn parse_production(body: &str) -> Result<SoarProduction, SoarParseError> {
        Parser::parse_production(body)
    }
}

enum RawCommand {
    /// The argument of an `sp` command.
    Sp(String),
    /// Any other command's full text.
    Other(String),
}

/// Splits a script into commands, in order.
fn script_commands(script: &str) -> Result<Vec<RawCommand>, SoarParseError> {
    let bytes = script.as_bytes();
    let mut bodies = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        // Skip whitespace, command separators and comments between commands.
        match bytes[i] {
            c if c.is_ascii_whitespace() || c == b';' => {
                i += 1;
                continue;
            }
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            _ => {}
        }
        let word_start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && !matches!(bytes[i], b'{' | b'"' | b';') {
            i += 1;
        }
        let word = &script[word_start..i];
        while i < bytes.len() && matches!(bytes[i], b' ' | b'\t') {
            i += 1;
        }
        if word == "sp" {
            let (body, end) = match bytes.get(i) {
                Some(b'{') => {
                    let end = matching_brace(script, i)?;
                    (&script[i + 1..end], end + 1)
                }
                Some(b'"') => {
                    let end = script[i + 1..]
                        .find('"')
                        .map(|offset| i + 1 + offset)
                        .ok_or_else(|| SoarParseError::InvalidSyntax("sp argument missing closing '\"'".into()))?;
                    (&script[i + 1..end], end + 1)
                }
                _ => return err("Rule header missing opening '{'"),
            };
            bodies.push(RawCommand::Sp(body.to_string()));
            i = end;
        } else {
            // The rest of the command, including any braced arguments spanning lines.
            while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b';' {
                if bytes[i] == b'{' {
                    i = matching_brace(script, i)?;
                }
                i += 1;
            }
            bodies.push(RawCommand::Other(script[word_start..i.min(bytes.len())].to_string()));
        }
    }
    Ok(bodies)
}

/// Splits a command into words; `"…"` and `{…}` group words and are removed.
fn command_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let mut word = String::new();
        match c {
            // Upstream `tokenizer::read_quoted_string` and `parse_escape_sequence`.
            '"' => {
                chars.next();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('a') => word.push('\u{7}'),
                            Some('b') => word.push('\u{8}'),
                            Some('f') => word.push('\u{c}'),
                            Some('n') => word.push('\n'),
                            Some('r') => word.push('\r'),
                            Some('t') => word.push('\t'),
                            Some('v') => word.push('\u{b}'),
                            Some('\n') => {
                                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                                    chars.next();
                                }
                                word.push(' ');
                            }
                            Some(other) => word.push(other),
                            None => {}
                        },
                        _ => word.push(c),
                    }
                }
            }
            '{' => {
                let close = if c == '"' { '"' } else { '}' };
                chars.next();
                let mut depth = 0;
                for c in chars.by_ref() {
                    if c == '{' && close == '}' {
                        depth += 1;
                    } else if c == close {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                    }
                    word.push(c);
                }
            }
            _ => {
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() {
                        break;
                    }
                    word.push(c);
                    chars.next();
                }
            }
        }
        words.push(word);
    }
    words
}

/// Parses a command other than `sp` into a [`ScriptCommand`], following upstream's CLI
/// syntax and validation. Upstream aliases are expanded first.
fn parse_command(text: &str) -> Result<ScriptCommand, SoarParseError> {
    use crate::preference::{ExplorationPolicy, NumericIndifferentMode};
    let unsupported = || SoarParseError::InvalidSyntax(format!("Unsupported command: {}", text.trim()));
    let invalid = |why: String| SoarParseError::InvalidSyntax(format!("{}: {}", why, text.trim()));
    let mut words = command_words(text);
    // Upstream aliases (cli_Aliases.h).
    let expansion: &[&str] = match words.first().map(String::as_str) {
        Some("indifferent-selection" | "inds") => &["decide", "indifferent-selection"],
        Some("numeric-indifferent-mode") => &["decide", "numeric-indifferent-mode"],
        Some("srand") => &["decide", "srand"],
        Some("waitsnc") => &["soar", "wait-snc"],
        Some("wma") => &["wm", "activation"],
        Some("w") => &["trace"],
        _ => &[],
    };
    if !expansion.is_empty() {
        words.splice(0..1, expansion.iter().map(|w| w.to_string()));
    }
    let args: Vec<&str> = words.iter().map(String::as_str).collect();
    let number = |value: Option<&&str>| value.and_then(|v| v.parse::<f64>().ok());
    match args.as_slice() {
        ["rl", "-s" | "--set", name, value] => {
            crate::rl::RlSetting::parse(name, value).map(ScriptCommand::Rl).map_err(invalid)
        }
        ["smem", "-s" | "--set", name, value] => {
            crate::smem::SmemSetting::parse(name, value).map(ScriptCommand::Smem).map_err(invalid)
        }
        // JSoar's spellings beside upstream's `on`/`off`.
        ["soar", "wait-snc", "on" | "true" | "-e" | "--enable" | "--on"] => Ok(ScriptCommand::WaitSnc(true)),
        ["soar", "wait-snc", "off" | "false" | "-d" | "--disable" | "--off"] => Ok(ScriptCommand::WaitSnc(false)),
        // JSoar 4 and Soar before 9.6 call `chunk` `learn`.
        ["learn", "-e" | "--enable" | "--on"] => Ok(ScriptCommand::Chunk(crate::learning::ChunkSetting::Learn(crate::learning::LearnIn::Always))),
        ["learn", "-d" | "--disable" | "--off"] => Ok(ScriptCommand::Chunk(crate::learning::ChunkSetting::Learn(crate::learning::LearnIn::Never))),
        ["learn", "-o" | "--only"] => Ok(ScriptCommand::Chunk(crate::learning::ChunkSetting::Learn(crate::learning::LearnIn::Only))),
        ["learn", "-E" | "--except"] => Ok(ScriptCommand::Chunk(crate::learning::ChunkSetting::Learn(crate::learning::LearnIn::Except))),
        ["chunk", "always"] => Ok(ScriptCommand::Chunk(crate::learning::ChunkSetting::Learn(crate::learning::LearnIn::Always))),
        ["chunk", "never"] => Ok(ScriptCommand::Chunk(crate::learning::ChunkSetting::Learn(crate::learning::LearnIn::Never))),
        ["chunk", "only"] => Ok(ScriptCommand::Chunk(crate::learning::ChunkSetting::Learn(crate::learning::LearnIn::Only))),
        ["chunk", "except"] => Ok(ScriptCommand::Chunk(crate::learning::ChunkSetting::Learn(crate::learning::LearnIn::Except))),
        ["chunk", "singleton", id, attr, value] => {
            let id = crate::learning::SingletonElement::parse(id).map_err(invalid)?;
            let value = crate::learning::SingletonElement::parse(value).map_err(invalid)?;
            Ok(ScriptCommand::ChunkSingleton { pattern: Some((id, value)), attr: (*attr).into() })
        }
        ["chunk", "singleton", "-r", id, attr, value] => {
            crate::learning::SingletonElement::parse(id).map_err(invalid)?;
            crate::learning::SingletonElement::parse(value).map_err(invalid)?;
            Ok(ScriptCommand::ChunkSingleton { pattern: None, attr: (*attr).into() })
        }
        ["chunk", name, value] => crate::learning::ChunkSetting::parse(name, value).map(ScriptCommand::Chunk).map_err(invalid),
        ["epmem", "-s" | "--set", name, value] => {
            crate::epmem::EpmemSetting::parse(name, value).map(ScriptCommand::Epmem).map_err(invalid)
        }
        ["epmem", "-e" | "--enable" | "--on"] => Ok(ScriptCommand::Epmem(crate::epmem::EpmemSetting::Learning(true))),
        ["epmem", "-d" | "--disable" | "--off"] => Ok(ScriptCommand::Epmem(crate::epmem::EpmemSetting::Learning(false))),
        ["smem", "-a" | "--add", clauses] => parse_smem_add(clauses).map(ScriptCommand::SmemAdd),
        ["smem", "-e" | "--enable" | "--on"] => Ok(ScriptCommand::Smem(crate::smem::SmemSetting::Learning(true))),
        ["smem", "-d" | "--disable" | "--off"] => Ok(ScriptCommand::Smem(crate::smem::SmemSetting::Learning(false))),
        ["decide", "indifferent-selection", option, rest @ ..] => {
            let policy = match *option {
                "-b" | "--boltzmann" => Some(ExplorationPolicy::Boltzmann),
                "-g" | "--epsilon-greedy" => Some(ExplorationPolicy::EpsilonGreedy),
                "-f" | "--first" => Some(ExplorationPolicy::First),
                "-l" | "--last" => Some(ExplorationPolicy::Last),
                "-x" | "--softmax" => Some(ExplorationPolicy::Softmax),
                _ => None,
            };
            match (policy, *option, rest) {
                (Some(policy), _, []) => Ok(ScriptCommand::ExplorationPolicy(policy)),
                (None, "-e" | "--epsilon", [value]) => match number(Some(value)) {
                    Some(x) if (0.0..=1.0).contains(&x) => Ok(ScriptCommand::Epsilon(x)),
                    _ => Err(invalid("Invalid epsilon (expected a number from 0 to 1)".into())),
                },
                (None, "-t" | "--temperature", [value]) => match number(Some(value)) {
                    Some(x) if x > 0.0 => Ok(ScriptCommand::Temperature(x)),
                    _ => Err(invalid("Invalid temperature (expected a number above 0)".into())),
                },
                _ => Err(unsupported()),
            }
        }
        ["decide", "numeric-indifferent-mode", "-a" | "--avg" | "--average"] => {
            Ok(ScriptCommand::NumericIndifferentMode(NumericIndifferentMode::Avg))
        }
        ["decide", "numeric-indifferent-mode", "-s" | "--sum"] => Ok(ScriptCommand::NumericIndifferentMode(NumericIndifferentMode::Sum)),
        // Upstream reads the seed with `%u`; without a seed it seeds from the clock, which a
        // reproducible run cannot do.
        ["decide", "srand" | "set-random-seed", seed] => {
            seed.parse::<u32>().map(ScriptCommand::SetRandomSeed).map_err(|_| invalid("Invalid random seed".into()))
        }
        ["wm", "activation", "-s" | "--set", name, value] => {
            crate::wma::WmaSetting::parse(name, value).map(ScriptCommand::Wma).map_err(invalid)
        }
        ["wm", "activation"] => Ok(ScriptCommand::WmaQuery(crate::wma::WmaQuery::Settings)),
        ["wm", "activation", "-g" | "--get", name] => Ok(ScriptCommand::WmaQuery(crate::wma::WmaQuery::Get((*name).into()))),
        ["wm", "activation", "-h" | "--history", timetag] => match timetag.parse::<u64>() {
            Ok(t) if t > 0 => Ok(ScriptCommand::WmaQuery(crate::wma::WmaQuery::History(t))),
            _ => Err(invalid("Invalid timetag.".into())),
        },
        ["wm", "activation", "-S" | "--stats"] => Ok(ScriptCommand::WmaQuery(crate::wma::WmaQuery::Stats(None))),
        ["wm", "activation", "-S" | "--stats", name] => Ok(ScriptCommand::WmaQuery(crate::wma::WmaQuery::Stats(Some((*name).into())))),
        ["watch" | "trace", "-a" | "--wma"] => Ok(ScriptCommand::TraceWma(true)),
        ["watch" | "trace", "-a" | "--wma", "remove" | "0"] => Ok(ScriptCommand::TraceWma(false)),
        ["watch" | "trace", category @ ("-e" | "--epmem" | "-s" | "--smem" | "-R" | "--rl")]
        | ["watch" | "trace", category @ ("-e" | "--epmem" | "-s" | "--smem" | "-R" | "--rl"), "remove" | "0"] => {
            Ok(ScriptCommand::TraceUnprinted((*category).into()))
        }
        ["watch" | "trace", level] | ["trace", "-l" | "--level", level] => {
            level.parse::<u8>().map(ScriptCommand::Trace).map_err(|_| invalid("Invalid trace level".into()))
        }
        ["echo", "-n" | "--nonewline", words @ ..] => Ok(ScriptCommand::Echo { text: words.join(" "), newline: false }),
        ["echo", words @ ..] => Ok(ScriptCommand::Echo { text: words.join(" "), newline: true }),
        _ => Err(unsupported()),
    }
}

/// Index of the brace closing the one at `open`, skipping `|…|` strings and `#` comments.
fn matching_brace(content: &str, open: usize) -> Result<usize, SoarParseError> {
    let bytes = content.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(i);
                }
            }
            b'|' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'|' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'#' if i == 0 || bytes[i - 1].is_ascii_whitespace() => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    err("Unclosed brace")
}

/// Expands simple Tcl rule generators: `proc name { args } { foreach v $args { sp "…${v}…" } }`
/// followed by `name { a b c }`.
fn expand_generated_rules(script_content: &str) -> Result<String, SoarParseError> {
    let mut content = script_content.to_string();
    let mut search_from = 0;

    while let Some(proc_offset) = content[search_from..].find("proc ") {
        let proc_start = search_from + proc_offset;
        let header_start = proc_start + "proc ".len();
        let args_start = content[header_start..]
            .find('{')
            .map(|offset| header_start + offset)
            .ok_or_else(|| SoarParseError::InvalidSyntax("Procedure missing arguments".into()))?;
        let args_end = matching_brace(&content, args_start)?;
        let body_start = content[args_end + 1..]
            .find('{')
            .map(|offset| args_end + 1 + offset)
            .ok_or_else(|| SoarParseError::InvalidSyntax("Procedure missing body".into()))?;
        let body_end = matching_brace(&content, body_start)?;

        let name = content[header_start..args_start].trim().to_string();
        let parameter = content[args_start + 1..args_end].trim().to_string();
        let body = content[body_start + 1..body_end].to_string();
        let (loop_body, loop_variable) = if let Some(foreach_offset) = body.find("foreach ") {
            let foreach_start = foreach_offset + "foreach ".len();
            let loop_args_end = body[foreach_start..]
                .find('{')
                .map(|offset| foreach_start + offset)
                .ok_or_else(|| SoarParseError::InvalidSyntax("foreach missing body".into()))?;
            let loop_body_end = matching_brace(&body, loop_args_end)?;
            let loop_variable = body[foreach_start..loop_args_end]
                .split_whitespace()
                .next()
                .ok_or_else(|| SoarParseError::InvalidSyntax("foreach missing variable".into()))?
                .to_string();
            (body[loop_args_end + 1..loop_body_end].to_string(), loop_variable)
        } else {
            (body.clone(), parameter.clone())
        };

        content.replace_range(proc_start..=body_end, "");
        let invocation = format!("{} {{", name);
        let mut invocation_search = 0;
        while let Some(invocation_offset) = content[invocation_search..].find(&invocation) {
            let invocation_start = invocation_search + invocation_offset;
            let list_start = invocation_start + name.len() + 1;
            let list_end = matching_brace(&content, list_start)?;
            let values = content[list_start + 1..list_end].split_whitespace().collect::<Vec<&str>>();
            let mut replacement = String::new();
            for value in values {
                let generated = loop_body
                    .replace(&format!("${{{}}}", loop_variable), value)
                    .replace(&format!("${}", loop_variable), value);
                replacement.push_str(&generated);
                replacement.push('\n');
            }
            content.replace_range(invocation_start..=list_end, &replacement);
            invocation_search = invocation_start + replacement.len();
        }

        search_from = proc_start;
    }

    Ok(content)
}
