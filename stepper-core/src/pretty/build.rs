//! Turning a `Program` into a layout [`Doc`].
//!
//! Two things are going on in every function here, and they're independent:
//!
//! - **Precedence**, which decides parentheses. This is the same numeric ladder
//!   the printer has always used and it has to stay in step with how SML groups
//!   things (see `frontend::lower`), or printing a program and re-parsing it stops
//!   giving back the same tree. See
//!   [`expr_doc`] for the ladder itself.
//! - **Grouping**, which decides line breaks. Every construct that can be spread
//!   over several lines wraps its pieces in a `Group` with `Line`s between them,
//!   so the layout algorithm can keep it on one line when it fits. Laying out at
//!   `UNLIMITED_WIDTH` keeps every group flat and reproduces the single-line
//!   output this module produced before it could break lines at all — which is
//!   what the round-trip tests check.
//!
//! Collapsed lambdas are read from `ViewState` *here*, rather than at render
//! time, because folding one changes its width and therefore the line-breaking of
//! every group around it.

use crate::ast::{BinOp, Decl, Expr, ExprKind, Pattern, PatternBase, Program, Type, ValDecl};
use crate::view::ViewState;

use super::doc::{Ann, Doc};

// ---------------------------------------------------------------------------
// Token helpers — every leaf gets a syntactic class, so a stylesheet can color
// keywords apart from literals without the renderer knowing any grammar.
// ---------------------------------------------------------------------------

fn kw(s: &str) -> Doc {
    Doc::ann(Ann::Class("kw"), Doc::text(s))
}

fn op(s: &str) -> Doc {
    Doc::ann(Ann::Class("op"), Doc::text(s))
}

fn lit(s: String) -> Doc {
    Doc::ann(Ann::Class("lit"), Doc::text(s))
}

fn var(s: &str) -> Doc {
    Doc::ann(Ann::Class("var"), Doc::text(s))
}

fn space() -> Doc {
    Doc::text(" ")
}

fn parens_if(needed: bool, doc: Doc) -> Doc {
    if needed {
        Doc::concat(vec![Doc::text("("), doc, Doc::text(")")])
    } else {
        doc
    }
}

/// Whether this expression prints starting with a `~` — the two forms that do
/// being a negation and a negative literal. See the `Neg` arm of `expr_body`.
fn starts_with_tilde(expr: &Expr) -> bool {
    match expr.kind {
        ExprKind::Neg(_) => true,
        ExprKind::IntConst(n) => n < 0,
        _ => false,
    }
}

/// SML writes negative numbers with `~`, not `-` (which is reserved for the binary
/// subtraction operator). `unsigned_abs` avoids overflow on `i64::MIN`.
pub(super) fn format_int(n: i64) -> String {
    if n < 0 {
        format!("~{}", n.unsigned_abs())
    } else {
        n.to_string()
    }
}

// ---------------------------------------------------------------------------
// Programs and declarations
// ---------------------------------------------------------------------------

/// Top-level decls are separated by hard newlines: one `val` per line is the
/// shape of the source the user typed, and it shouldn't depend on the width.
pub fn program_doc(program: &Program, view: &ViewState) -> Doc {
    Doc::join(
        Doc::HardLine,
        program.iter().map(|d| decl_doc(d, view)).collect(),
    )
}

fn decl_doc(decl: &Decl, view: &ViewState) -> Doc {
    let (keyword, ValDecl { pat, expr }) = match decl {
        Decl::ValDecl(d) => ("val", d),
        Decl::ValRecDecl(d) => ("val rec", d),
    };
    // A right-hand side that brings its own line breaks stays on the `=`'s line
    // and breaks internally; anything else gets a break point before it, for when
    // the decl is too wide to fit. Without the distinction a `let` would be
    // pushed onto its own line first — `val y =` alone, then the block — which
    // wastes a line and buys nothing, since the block was going to break anyway.
    let separator = if is_block_form(expr) {
        space()
    } else {
        Doc::Line
    };
    Doc::group(Doc::nest(
        4,
        Doc::concat(vec![
            kw(keyword),
            space(),
            pattern_doc(pat, view),
            space(),
            op("="),
            separator,
            expr_doc(expr, 0, view),
        ]),
    ))
}

/// Whether `expr` lays itself out over several lines when it doesn't fit, and so
/// needs no break point in front of it.
fn is_block_form(expr: &Expr) -> bool {
    matches!(
        expr.kind,
        ExprKind::Let(..) | ExprKind::Match(..) | ExprKind::Lambda(..)
    )
}

// ---------------------------------------------------------------------------
// Patterns and types
// ---------------------------------------------------------------------------

pub fn pattern_doc(pat: &Pattern, view: &ViewState) -> Doc {
    let base = pattern_base_doc(&pat.pat, view);
    let body = match &pat.typ {
        Some(typ) => Doc::concat(vec![base, space(), op(":"), space(), type_doc(typ)]),
        None => base,
    };
    Doc::ann(Ann::Node(pat.id), body)
}

fn pattern_base_doc(base: &PatternBase, view: &ViewState) -> Doc {
    match base {
        // The name is display-only — identity lives in the binder's id (see
        // `ast::Binder`), so nothing but this function ever reads it.
        PatternBase::Var(binder) => var(&binder.name),
        PatternBase::Wildcard => var("_"),
        PatternBase::IntConst(n) => lit(format_int(*n)),
        PatternBase::BoolConst(b) => lit(b.to_string()),
        PatternBase::Tuple(pats) => Doc::group(Doc::concat(vec![
            Doc::text("("),
            Doc::nest(
                1,
                Doc::join(
                    Doc::concat(vec![Doc::text(","), Doc::Line]),
                    pats.iter().map(|p| pattern_doc(p, view)).collect(),
                ),
            ),
            Doc::text(")"),
        ])),
    }
}

/// Types print as one unbreakable token. They're short enough that breaking them
/// would cost more readability than it buys, and keeping them a single `Text`
/// keeps this module's parenthesization rules for types in one place.
fn type_doc(ty: &Type) -> Doc {
    Doc::ann(Ann::Class("ty"), Doc::text(type_string(ty)))
}

pub(super) fn type_string(ty: &Type) -> String {
    match ty {
        Type::Int => "int".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Product(types) => types
            .iter()
            .map(|t| type_atom_string(t))
            .collect::<Vec<_>>()
            .join(" * "),
        // Right-associative, so atom string ensures left properly prints parens 
        // in the event to (t1 -> t2) -> t3 but not t1 -> t2 -> t3
        Type::Arrow(param, result) => {
            format!("{} -> {}", type_atom_string(param), type_string(result))
        }
    }
}

/// Prints `ty` parenthesized if it wouldn't otherwise round-trip as one element of
/// a `*`-separated list or as the left operand of `->`.
fn type_atom_string(ty: &Type) -> String {
    match ty {
        Type::Product(_) | Type::Arrow(_, _) => format!("({})", type_string(ty)),
        Type::Int | Type::Bool => type_string(ty),
    }
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

/// Builds `expr`, parenthesizing it if its own precedence is lower than
/// `min_prec` (the precedence required by whatever position it's sitting in).
/// Lowest to highest: `if`/`case`/`fn` are 0; `orelse` is 1; `andalso` is 2;
/// comparisons (`=`/`<>`/`<`/`<=`/`>`/`>=`) are 3; `+`/`-` are 4; `*`/`div`/`mod`
/// are 5; function application is 6 — above every infix operator, as in SML;
/// `~` is 7; atoms (literals, variables, tuples, `let`) are unconditionally high
/// and ignore `min_prec` entirely. A left-associative binary op passes its own
/// precedence to its left child and one more than that to its right child, so
/// left-associative same-precedence nesting (`(a + b) + c`) prints bare while the
/// same tree on the right (`a + (b + c)`) — only reachable via explicit parens in
/// the source — gets parens back, since without them it would reparse
/// differently. Every binary operator here is left-associative, comparisons and
/// `andalso`/`orelse` included, matching how the parser groups them.
///
/// Every expression is wrapped in its own `Ann::Node`, so the backends can find
/// the region belonging to any node — that's what highlighting paints and what
/// clicking a lambda toggles.
pub fn expr_doc(expr: &Expr, min_prec: u8, view: &ViewState) -> Doc {
    Doc::ann(Ann::Node(expr.id), expr_body(expr, min_prec, view))
}

fn expr_body(expr: &Expr, min_prec: u8, view: &ViewState) -> Doc {
    match &expr.kind {
        ExprKind::IntConst(n) => lit(format_int(*n)),
        ExprKind::BoolConst(b) => lit(b.to_string()),
        ExprKind::Var(binder) => var(&binder.name),
        ExprKind::OrElse(l, r) => binop_doc(l, r, "orelse", (1, 1, 2), min_prec, view),
        ExprKind::AndAlso(l, r) => binop_doc(l, r, "andalso", (2, 2, 3), min_prec, view),
        ExprKind::BinOp(op, l, r) => {
            binop_doc(l, r, op.symbol(), binop_prec(*op), min_prec, view)
        }
        // Atomic: its operand is an atom, so anything compound — an application
        // included — takes parens (`~(f x)`), while `~x` stays bare. Never broken:
        // there's no room for a line break between `~` and its operand.
        //
        // The space matters when the operand starts with a `~` of its own. SML
        // lexes the longest symbolic identifier it can, so `~~5` is the identifier
        // `~~` applied to `5`, and printing `~ (~5)` that way would reparse as
        // something else entirely.
        ExprKind::Neg(inner) => parens_if(
            7 < min_prec,
            Doc::concat(vec![
                op("~"),
                if starts_with_tilde(inner) {
                    space()
                } else {
                    Doc::Nil
                },
                expr_doc(inner, 7, view),
            ]),
        ),
        // Precedence 0 (lower than everything, matching the grammar's
        // `%nonassoc 'ELSE'` at the lowest level): as soon as an if-expression
        // sits inside any other expression at all, it needs parens to stay
        // unambiguous — the else-branch is otherwise free to keep extending
        // across trailing operators (`if c then a else b + 1` means
        // `if c then a else (b + 1)`), so an `if` used as, say, the left operand
        // of `+` would misparse without them. Its own cond/then/else branches
        // are built unrestricted (0): the keywords already delimit them
        // unambiguously, which is also why `else if ...` chains stay bare.
        ExprKind::If(cond, then_branch, else_branch) => parens_if(
            0 < min_prec,
            Doc::group(Doc::concat(vec![
                kw("if"),
                space(),
                expr_doc(cond, 0, view),
                Doc::Line,
                kw("then"),
                space(),
                expr_doc(then_branch, 0, view),
                Doc::Line,
                kw("else"),
                space(),
                expr_doc(else_branch, 0, view),
            ])),
        ),
        // Bracketed by its own parens, so — like `Let` — this is atomic from the
        // outside and its elements are built unrestricted (0).
        ExprKind::Tuple(items) => Doc::group(Doc::concat(vec![
            Doc::text("("),
            Doc::nest(
                1,
                Doc::join(
                    Doc::concat(vec![Doc::text(","), Doc::Line]),
                    items.iter().map(|e| expr_doc(e, 0, view)).collect(),
                ),
            ),
            Doc::text(")"),
        ])),
        // Bracketed by `case`/`of` on the left but open-ended on the right (no
        // `end`), matching real SML — so, like `If`, this needs parens as soon as
        // it sits inside anything else. The scrutinee and each arm's body are
        // built unrestricted (0), since the keywords/`=>`/`|` already delimit
        // them; a nested `case` as the last arm's body prints bare and simply
        // absorbs any further `|` arms, the same nearest-wins resolution SML
        // itself gives a dangling arm.
        ExprKind::Match(scrutinee, arms) => parens_if(
            0 < min_prec,
            Doc::align(Doc::group(Doc::concat(vec![
                kw("case"),
                space(),
                expr_doc(scrutinee, 0, view),
                space(),
                kw("of"),
                cases_doc(arms, view),
            ]))),
        ),
        // Open-ended (no closing keyword), same treatment as `If`/`Match`:
        // precedence 0, needs parens the instant it sits inside anything else
        // (e.g. as an application operand — see `App` below).
        ExprKind::Lambda(cases) => parens_if(
            0 < min_prec,
            if view.is_collapsed(expr.id) {
                collapsed_lambda_doc(cases, view)
            } else {
                Doc::align(Doc::group(Doc::concat(vec![
                    kw("fn"),
                    cases_doc(cases, view),
                ])))
            },
        ),
        // Binds tighter than every infix operator (own precedence 6, above `*`'s
        // 5), so an application prints bare as any infix operand — `f x + y`.
        // Only `~` (7, atomic) outranks it, which is why `~f x` prints bare too
        // and means `(~f) x`, matching the grammar. Left-associative, so the left
        // child accepts its own precedence (nested application prints bare —
        // `f x y`, not `(f x) y`) while the right child needs strictly higher,
        // since SML's `atexp` never includes a bare `App` — `f (g x)` only
        // round-trips with parens.
        ExprKind::App(..) => app_body(expr, min_prec, view),
        // Bracketed by `let`/`end` on both sides, so — unlike `if` — this is a
        // true atom, one of SML's `atexp` forms: never needs its own wrapping
        // parens regardless of `min_prec`, even as an
        // application argument. Its decls and body are likewise built unrestricted
        // (0), since the keywords already delimit them.
        ExprKind::Let(decls, body) => {
            // `Align` rather than `Nest`, so `let`/`in`/`end` line up under each
            // other wherever the `let` landed, and the contents sit two columns
            // in from them:
            //
            //     val y = let
            //               val a = 1
            //             in
            //               a + a
            //             end
            let mut pieces = vec![kw("let")];
            if !decls.is_empty() {
                pieces.push(Doc::nest(
                    2,
                    Doc::concat(vec![
                        Doc::Line,
                        Doc::join(Doc::Line, decls.iter().map(|d| decl_doc(d, view)).collect()),
                    ]),
                ));
            }
            pieces.push(Doc::Line);
            pieces.push(kw("in"));
            pieces.push(Doc::nest(
                2,
                Doc::concat(vec![Doc::Line, expr_doc(body, 0, view)]),
            ));
            pieces.push(Doc::Line);
            pieces.push(kw("end"));
            Doc::align(Doc::group(Doc::concat(pieces)))
        }
    }
}

/// An application spine — `f a b c` — as one group rather than one per `App`
/// node.
///
/// Built naively, each `App` gets its own `Group` and `Nest`, and since the tree
/// is left-nested the *last* argument ends up least indented and the first most:
///
/// ```text
/// foo (a + b) (c + d)
///     (e + f)
///   (g + h)
/// ```
///
/// Collecting the spine puts every argument in one group at one indent, so they
/// break together and line up. The intermediate `App` nodes still get their own
/// `Ann::Node` — `f a` is a node in its own right, and `step` can pick it as the
/// redex before the outer application — they just no longer each introduce
/// layout.
fn app_body(whole: &Expr, min_prec: u8, view: &ViewState) -> Doc {
    // Walk down the left spine collecting the `App` nodes, outermost first.
    let mut apps = Vec::new();
    let mut head = whole;
    while let ExprKind::App(f, _) = &head.kind {
        apps.push(head);
        head = f;
    }
    apps.reverse();

    let mut doc = expr_doc(head, 6, view);
    let last = apps.len() - 1;
    for (i, app) in apps.iter().enumerate() {
        let ExprKind::App(_, arg) = &app.kind else {
            unreachable!("collected from `App` nodes only")
        };
        let joined = Doc::concat(vec![doc, Doc::Line, expr_doc(arg, 7, view)]);
        // The outermost is `whole`, whose annotation `expr_doc` has already
        // applied; the inner ones need theirs here.
        doc = if i == last {
            joined
        } else {
            Doc::ann(Ann::Node(app.id), joined)
        };
    }
    parens_if(6 < min_prec, Doc::group(Doc::nest(4, doc)))
}

/// The `p => e | p => e` tail shared by `case` and `fn`.
///
/// With one arm there's nothing to line up, so it simply follows the keyword and
/// breaks inside itself if it must:
///
/// ```text
/// fn n : int =>
///     if n = 0 then 1 else n * 2
/// ```
///
/// With several, each gets its own line and the `|`s line up under the keyword,
/// which is where SML normally puts them:
///
/// ```text
/// case n of
///   0 => 1
/// | n => n * 2
/// ```
///
/// Flat, the same document reads `case n of 0 => 1 | n => n * 2`.
fn cases_doc(cases: &[(Pattern, Expr)], view: &ViewState) -> Doc {
    let arm = |(pat, body): &(Pattern, Expr)| {
        Doc::group(Doc::nest(
            4,
            Doc::concat(vec![
                pattern_doc(pat, view),
                space(),
                op("=>"),
                Doc::Line,
                expr_doc(body, 0, view),
            ]),
        ))
    };
    if let [only] = cases {
        return Doc::concat(vec![space(), arm(only)]);
    }
    let mut pieces = vec![Doc::nest(2, Doc::concat(vec![Doc::Line, arm(&cases[0])]))];
    for case in &cases[1..] {
        pieces.push(Doc::Line);
        pieces.push(Doc::text("| "));
        pieces.push(arm(case));
    }
    // These `Line`s land at whatever indent encloses them; the `Doc::align` that
    // makes that the keyword's own column is applied by the caller, since it has
    // to start at `case`/`fn`, not here.
    Doc::concat(pieces)
}

/// A folded lambda: its parameter, and nothing else.
///
/// The annotation is dropped along with the body, so `fn x : int => x + 1` folds
/// to `fn x => ...` — the point is to get the function out of the way, and its
/// type is part of what's in the way. Note this is the one rendering that
/// deliberately isn't valid SML: a folded program is for looking at, not for
/// re-entering, and nothing ever parses the printer's output back except the
/// round-trip tests, which fold nothing.
fn collapsed_lambda_doc(cases: &[(Pattern, Expr)], view: &ViewState) -> Doc {
    let (pat, _) = &cases[0];
    Doc::concat(vec![
        kw("fn"),
        space(),
        Doc::ann(Ann::Node(pat.id), pattern_base_doc(&pat.pat, view)),
        space(),
        op("=>"),
        space(),
        Doc::ann(Ann::Class("folded"), Doc::text("...")),
    ])
}

/// Where each [`BinOp`] sits on the ladder, as `(own, left_min, right_min)`.
///
/// Every one of these is left-associative, so the left child accepts the
/// operator's own precedence and the right child needs one more. The numbers have
/// to mirror the standard basis' fixities, which is what the parser groups by —
/// see `sml_fixity::STD_BASIS`.
fn binop_prec(op: BinOp) -> (u8, u8, u8) {
    match op {
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => (3, 3, 4),
        BinOp::Add | BinOp::Sub => (4, 4, 5),
        BinOp::Mul | BinOp::Div | BinOp::Mod => (5, 5, 6),
    }
}

fn binop_doc(
    l: &Expr,
    r: &Expr,
    operator: &str,
    (own_prec, left_min, right_min): (u8, u8, u8),
    min_prec: u8,
    view: &ViewState,
) -> Doc {
    parens_if(
        own_prec < min_prec,
        Doc::group(Doc::nest(
            2,
            Doc::concat(vec![
                expr_doc(l, left_min, view),
                space(),
                op(operator),
                Doc::Line,
                expr_doc(r, right_min, view),
            ]),
        )),
    )
}
