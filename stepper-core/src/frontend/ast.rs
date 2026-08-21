#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Int,
    Bool,
    Product(Vec<Box<Type>>),
    Arrow(Box<Type>, Box<Type>),
}

/// Why an `Expr::Highlighted` node is marked, and which color it renders as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightColor {
    Yellow,
    Green,
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
    Neg(Box<Expr>),
    Eq(Box<Expr>, Box<Expr>),
    Ne(Box<Expr>, Box<Expr>),
    Lt(Box<Expr>, Box<Expr>),
    Le(Box<Expr>, Box<Expr>),
    Gt(Box<Expr>, Box<Expr>),
    Ge(Box<Expr>, Box<Expr>),
    AndAlso(Box<Expr>, Box<Expr>),
    OrElse(Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Let(Vec<Decl>, Box<Expr>),
    Tuple(Vec<Expr>),
    Match(Box<Expr>, Vec<(Pattern, Expr)>),
    App(Box<Expr>, Box<Expr>),
    Lambda(Vec<(Pattern, Expr)>),
    Highlighted(Box<Expr>, HighlightColor),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    pub pat: PatternBase,
    pub typ: Option<Type>,
}

impl Pattern {
    /// A pattern carrying no `: type` annotation — every pattern form except the
    /// one grammar production that attaches one.
    pub fn untyped(pat: PatternBase) -> Self {
        Pattern { pat, typ: None }
    }

    /// The type this pattern states outright, if it states one — either as its own
    /// `: type` annotation or, for a tuple whose every component states one, as the
    /// product of those. Nothing else is derivable: an unannotated variable or a
    /// wildcard could stand for anything, and a literal's type (`0 : int`) is only
    /// *checked*, never a declaration, since a `fn`'s parameter type must come from
    /// something the programmer wrote rather than from a pattern that happens to be
    /// a literal.
    ///
    /// This is what lets `fn (x : int, y : bool) => ...` and its `fun` spelling be
    /// accepted without repeating the type: annotating the components says the same
    /// thing as annotating the whole.
    pub fn declared_type(&self) -> Option<Type> {
        if let Some(typ) = &self.typ {
            return Some(typ.clone());
        }
        match &self.pat {
            PatternBase::Tuple(pats) => pats
                .iter()
                .map(|p| p.declared_type().map(Box::new))
                .collect::<Option<Vec<_>>>()
                .map(Type::Product),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PatternBase {
    Ident(String),
    Wildcard,
    IntConst(i64),
    BoolConst(bool),
    Tuple(Vec<Pattern>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decl {
    ValDecl(ValDecl),
    ValRecDecl(ValDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValDecl {
    pub pat: Pattern,
    pub expr: Expr,
}

impl Decl {
    pub fn get_val_decl(&self) -> &ValDecl {
        match self {
            Self::ValDecl(decl) => decl,
            Self::ValRecDecl(decl) => decl,
        }
    }

    pub fn copy_val_type(&self, val_decl: ValDecl) -> Self {
        match self {
            Self::ValDecl(_) => Self::ValDecl(val_decl),
            Self::ValRecDecl(_) => Self::ValRecDecl(val_decl),
        }
    }

    pub fn new_expr(&self, expr: Expr) -> Self {
        self.copy_val_type(ValDecl { pat: self.get_val_decl().pat.clone(), expr })
    }
}
pub type Program = Vec<Decl>;
