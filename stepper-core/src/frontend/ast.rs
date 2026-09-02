//! The abstract syntax tree: purely semantic, with no display concerns in it.
//!
//! Two kinds of identity live here, and they answer different questions:
//!
//! - [`NodeId`] — *which node is this?* Every `Expr` and `Pattern` carries one.
//!   The view layer keys off these (see [`crate::view::ViewState`]) so that
//!   highlighting a redex or collapsing a lambda never has to touch the tree.
//! - [`BinderId`] — *which variable is this?* A binding site and every use of it
//!   share one. Names are carried alongside purely so the program can be printed;
//!   nothing in the stepper or the typechecker ever compares them.
//!
//! Making variables reference their binding site is what removes shadowing as a
//! concern everywhere downstream: `subst.rs` doesn't check whether an inner
//! pattern rebinds the name it's replacing (a different binding is a different
//! id), and `eval.rs` doesn't have to invent a fresh spelling for a recursive
//! function to keep it from colliding with a later one.

use std::cell::Cell;

thread_local! {
    static NEXT_NODE_ID: Cell<u32> = const { Cell::new(0) };
    static NEXT_BINDER_ID: Cell<u32> = const { Cell::new(0) };
}

/// Identifies one node of the tree, for the view layer's benefit.
///
/// **Every live node has an id nothing else shares.** Two rules keep that true,
/// and between them they decide what display state survives a reduction:
///
/// - A step that rewrites a node *in place* reuses its id (see
///   [`Expr::same_id`]), and untouched subtrees are cloned as they are. So the
///   `1 + 2` that becomes `3` is the same place on screen, and a lambda you
///   folded stays folded across steps that don't involve it.
/// - A step that *duplicates* a subtree gives the copy fresh ids
///   (`stepping::subst::fresh_copy`). Substituting a lambda into two use sites
///   makes two independent lambdas: folding one leaves the other alone, and
///   highlighting a redex inside one doesn't mark the identical spot in the
///   other.
///
/// The second rule is why `step` reports the ids it placed rather than letting
/// callers infer them from the value it substituted — after copying, the source's
/// ids appear nowhere in the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

impl NodeId {
    pub fn fresh() -> Self {
        NEXT_NODE_ID.with(|n| {
            let id = n.get();
            n.set(id + 1);
            NodeId(id)
        })
    }
}

/// Identifies one variable — that is, one binding site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BinderId(pub u32);

impl BinderId {
    pub fn fresh() -> Self {
        NEXT_BINDER_ID.with(|n| {
            let id = n.get();
            n.set(id + 1);
            BinderId(id)
        })
    }
}

/// A variable, at its binding site (`PatternBase::Var`) or at a use
/// (`ExprKind::Var`). `id` is the identity; `name` is what the source called it
/// and exists only so the program can be printed back.
///
/// The parser mints a fresh `id` at every occurrence and
/// [`crate::frontend::resolve`] then repoints each *use* at the binder it
/// actually refers to, so by the time anything downstream sees the tree, `id`
/// answers "which variable" exactly.
#[derive(Debug, Clone, Eq)]
pub struct Binder {
    pub id: BinderId,
    pub name: String,
}

impl Binder {
    pub fn new(name: impl Into<String>) -> Self {
        Binder {
            id: BinderId::fresh(),
            name: name.into(),
        }
    }
}

/// Compares by *name*, not by id — see the note on `PartialEq for Expr`.
impl PartialEq for Binder {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

/// A binary operator whose two operands are simply reduced, left to right, and
/// then combined. Every such operator shares one `ExprKind`, one typing rule and
/// one reduction rule; all that distinguishes them is the spelling, the
/// precedence and the arithmetic, each looked up by the module that cares.
///
/// `andalso`/`orelse` are deliberately *not* here: they short-circuit, so their
/// right operand may never be evaluated at all, and neither is `~`, which is
/// unary and atomic in the grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl BinOp {
    /// How SML spells this operator. Lives here rather than in `pretty` because
    /// the stepper's messages need it too, and there is exactly one right answer.
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "div",
            BinOp::Mod => "mod",
            BinOp::Eq => "=",
            BinOp::Ne => "<>",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Int,
    Bool,
    Product(Vec<Box<Type>>),
    Arrow(Box<Type>, Box<Type>),
}

/// An expression node: its identity, plus its shape.
///
/// The split exists so that `step` can pattern-match on `kind` and read like the
/// inference rules it implements, while `id` rides along for the view layer.
#[derive(Debug, Clone)]
pub struct Expr {
    pub id: NodeId,
    pub kind: ExprKind,
}

/// Structural equality, ignoring `id`.
///
/// Ids are minted per occurrence, so two independently parsed copies of the same
/// program never share any — comparing them would make every `assert_eq!` on a
/// tree fail. What callers (and the tests) mean by equality here is always "same
/// shape", and for the same reason `Binder` compares by name.
impl PartialEq for Expr {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

impl Expr {
    pub fn new(kind: ExprKind) -> Self {
        Expr {
            id: NodeId::fresh(),
            kind,
        }
    }

    /// A node with `self`'s identity but a new shape — what an in-place
    /// reduction produces. Reusing the id is what lets the view layer treat
    /// `1 + 2` and the `3` it becomes as the same place on screen.
    pub fn same_id(&self, kind: ExprKind) -> Expr {
        Expr { id: self.id, kind }
    }

    pub fn var(binder: Binder) -> Expr {
        Expr::new(ExprKind::Var(binder))
    }

    pub fn binop(op: BinOp, l: Expr, r: Expr) -> Expr {
        Expr::new(ExprKind::BinOp(op, Box::new(l), Box::new(r)))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    IntConst(i64),
    BoolConst(bool),
    Var(Binder),
    BinOp(BinOp, Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
    AndAlso(Box<Expr>, Box<Expr>),
    OrElse(Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Let(Vec<Decl>, Box<Expr>),
    Tuple(Vec<Expr>),
    Match(Box<Expr>, Vec<(Pattern, Expr)>),
    App(Box<Expr>, Box<Expr>),
    Lambda(Vec<(Pattern, Expr)>),
}

#[derive(Debug, Clone)]
pub struct Pattern {
    pub id: NodeId,
    pub pat: PatternBase,
    pub typ: Option<Type>,
}

/// Structural equality, ignoring `id` — same reasoning as `PartialEq for Expr`.
impl PartialEq for Pattern {
    fn eq(&self, other: &Self) -> bool {
        self.pat == other.pat && self.typ == other.typ
    }
}

impl Pattern {
    pub fn new(pat: PatternBase, typ: Option<Type>) -> Self {
        Pattern {
            id: NodeId::fresh(),
            pat,
            typ,
        }
    }

    /// A pattern carrying no `: type` annotation — every pattern form except the
    /// one grammar production that attaches one.
    pub fn untyped(pat: PatternBase) -> Self {
        Pattern::new(pat, None)
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

    /// Every variable this pattern binds, outermost-first through nested tuples.
    pub fn binders(&self) -> Vec<&Binder> {
        let mut out = Vec::new();
        self.collect_binders(&mut out);
        out
    }

    fn collect_binders<'a>(&'a self, out: &mut Vec<&'a Binder>) {
        match &self.pat {
            PatternBase::Var(binder) => out.push(binder),
            PatternBase::Wildcard | PatternBase::IntConst(_) | PatternBase::BoolConst(_) => {}
            PatternBase::Tuple(pats) => pats.iter().for_each(|p| p.collect_binders(out)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PatternBase {
    Var(Binder),
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
        self.copy_val_type(ValDecl {
            pat: self.get_val_decl().pat.clone(),
            expr,
        })
    }
}

pub type Program = Vec<Decl>;

/// Calls `f` on every expression in `program`, each node before its children.
///
/// One traversal that other modules borrow instead of writing their own: the
/// tree has twenty-odd shapes and a hand-rolled walk that forgets one goes wrong
/// quietly. `stepping` doesn't use it — its traversals each have their own
/// evaluation order to respect — but anything that just wants to see every node
/// should.
pub fn walk_exprs(program: &Program, f: &mut impl FnMut(&Expr)) {
    for decl in program {
        walk_expr(&decl.get_val_decl().expr, f);
    }
}

pub fn walk_expr(expr: &Expr, f: &mut impl FnMut(&Expr)) {
    f(expr);
    match &expr.kind {
        ExprKind::IntConst(_) | ExprKind::BoolConst(_) | ExprKind::Var(_) => {}
        ExprKind::Neg(inner) => walk_expr(inner, f),
        ExprKind::BinOp(_, l, r)
        | ExprKind::AndAlso(l, r)
        | ExprKind::OrElse(l, r)
        | ExprKind::App(l, r) => {
            walk_expr(l, f);
            walk_expr(r, f);
        }
        ExprKind::If(cond, then_branch, else_branch) => {
            walk_expr(cond, f);
            walk_expr(then_branch, f);
            walk_expr(else_branch, f);
        }
        ExprKind::Tuple(items) => items.iter().for_each(|i| walk_expr(i, f)),
        ExprKind::Let(decls, body) => {
            for decl in decls {
                walk_expr(&decl.get_val_decl().expr, f);
            }
            walk_expr(body, f);
        }
        ExprKind::Match(scrutinee, arms) => {
            walk_expr(scrutinee, f);
            arms.iter().for_each(|(_, e)| walk_expr(e, f));
        }
        ExprKind::Lambda(cases) => cases.iter().for_each(|(_, e)| walk_expr(e, f)),
    }
}

/// The id of every lambda in `program`, outermost-first — exactly the nodes the
/// display makes foldable.
pub fn lambda_ids(program: &Program) -> Vec<NodeId> {
    let mut out = Vec::new();
    walk_exprs(program, &mut |e| {
        if matches!(e.kind, ExprKind::Lambda(..)) {
            out.push(e.id);
        }
    });
    out
}
