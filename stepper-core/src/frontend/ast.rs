#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Int,
    Bool,
    Product(Vec<Box<Type>>)
}

/// Why an `Expr::Highlighted` node is marked, and which color it renders as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightColor {
    /// The next thing `stepping::step` will act on. Computed fresh at render time;
    /// never stored in the persisted program.
    Yellow,
    /// A value that a substitution step just placed. Persists in the stored program
    /// for exactly one render, then `stepping::step` strips it on the next call.
    Green,
    /// Reserved for a future exception/error indicator.
    Red,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    IntConst(i64),
    BoolConst(bool),
    Ident(String),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
    Mod(Box<Expr>, Box<Expr>),
    /// Unary negation (`~e` in SML syntax, e.g. `~5`, `~x`, `~(1 + 2)`).
    Neg(Box<Expr>),
    /// Int comparisons; all produce a bool. Nonassociative (no `a < b < c`),
    /// matching SML — `a < b` is a bool, and `<` isn't defined over bools.
    Eq(Box<Expr>, Box<Expr>),
    Ne(Box<Expr>, Box<Expr>),
    Lt(Box<Expr>, Box<Expr>),
    Le(Box<Expr>, Box<Expr>),
    Gt(Box<Expr>, Box<Expr>),
    Ge(Box<Expr>, Box<Expr>),
    /// Short-circuiting: only evaluates the right side if needed. Right-associative
    /// (`a andalso b andalso c` is `a andalso (b andalso c)`), matching SML.
    AndAlso(Box<Expr>, Box<Expr>),
    OrElse(Box<Expr>, Box<Expr>),
    /// `if cond then t else e`. `else` is always required (no dangling-else
    /// ambiguity), matching SML's pure-expression `if`.
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `let <decls> in <body> end`. Reuses `Decl`/`Program` directly, since a
    /// let's decl list is exactly a nested program. Bracketed by `let`/`end`, so
    /// (unlike `if`) it's atomic from the outside — never needs its own parens.
    Let(Vec<Decl>, Box<Expr>),
    /// `(e1, e2, ...)`, always two or more elements — a single parenthesized
    /// expression is just grouping and doesn't produce this. A value once every
    /// element is; for now tuples can only be built and passed around, not
    /// destructured (no pattern matching yet).
    Tuple(Vec<Expr>),
    /// Marks the wrapped expression as a point of interest for rendering. Carries no
    /// semantic weight — see `HighlightColor` for what each color means and how long
    /// it lives.
    Highlighted(Box<Expr>, HighlightColor),
}

/// The left-hand side of a `val` declaration. `match` expressions (and the richer
/// patterns they'll eventually need) come later.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Ident(String),
    /// `_`: matches anything, binds nothing.
    Wildcard,
    /// A literal pattern (`5`, `~5`, `true`) — matches only that exact value and
    /// binds nothing. Unlike every other pattern here, this one is refutable: a
    /// `val` decl whose right-hand side doesn't equal the literal has no
    /// well-defined binding (SML raises `Bind` at runtime; see `stepping::subst`
    /// for how this project handles it until exceptions exist).
    IntConst(i64),
    BoolConst(bool),
    /// `(p1, p2, ...)`, always two or more elements, matching `Expr::Tuple`.
    Tuple(Vec<Pattern>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decl {
    pub pat: Pattern,
    pub ty: Option<Type>,
    pub expr: Expr,
}

pub type Program = Vec<Decl>;
