//! Finding the next redex, without performing it and without touching the tree.
//!
//! This used to return a *copy of the program* with the redex wrapped in an
//! `Expr::Highlighted` node, which meant `step` had to strip those wrappers back
//! off before it could work and every traversal in the crate needed a case for
//! them. Now that highlighting is view state keyed by `NodeId`, the answer is
//! just an id, and this file is only a search.
//!
//! Its one obligation is to mirror `eval`'s search order exactly, so that what
//! gets painted yellow is always precisely what the next `step` call will act on.
//! Each function here is the shadow of the `step_*` function named in its comment;
//! `tests/common`'s `run_to_value` asserts the two agree at every step.

use crate::ast::{Decl, Expr, ExprKind, NodeId, Program};

use super::eval::{is_value, lookup_rec};

/// The node `step` would reduce next, or `None` if the program is fully reduced.
pub fn highlight_next(program: &Program) -> Option<NodeId> {
    let first = program.first()?;
    let expr = &first.get_val_decl().expr;

    if let Some(id) = highlight_next_expr(expr) {
        return Some(id);
    }

    if program.len() == 1 {
        return None;
    }

    // The first decl is a value and there's more program left: it's about to be
    // substituted into the rest, so it's what's "next".
    Some(expr.id)
}

fn highlight_next_expr(expr: &Expr) -> Option<NodeId> {
    match &expr.kind {
        ExprKind::IntConst(_) | ExprKind::BoolConst(_) | ExprKind::Lambda(..) => None,
        // Mirrors `step_var`: a variable is only a redex when it names a
        // recursive function waiting to be unrolled.
        ExprKind::Var(binder) => lookup_rec(binder.id).map(|_| expr.id),
        ExprKind::Add(l, r)
        | ExprKind::Sub(l, r)
        | ExprKind::Mul(l, r)
        | ExprKind::Div(l, r)
        | ExprKind::Mod(l, r)
        | ExprKind::Eq(l, r)
        | ExprKind::Ne(l, r)
        | ExprKind::Lt(l, r)
        | ExprKind::Le(l, r)
        | ExprKind::Gt(l, r)
        | ExprKind::Ge(l, r) => highlight_next_binop(expr, l, r),
        ExprKind::Neg(inner) => highlight_next_unary(expr, inner),
        // `andalso`/`orelse` only need their left operand to be a value before
        // firing (`r` is never inspected), unlike a binop which waits for both —
        // matches `step_shortcircuit`'s short-circuit shape. Same shape as `Neg`,
        // which likewise fires as soon as its one inspected operand is ready.
        ExprKind::AndAlso(l, _) | ExprKind::OrElse(l, _) => highlight_next_unary(expr, l),
        // Mirrors `step_if`: only the condition is searched into; once it's a
        // value the whole `If` is the redex, whichever branch that picks.
        ExprKind::If(cond, _, _) => highlight_next_unary(expr, cond),
        // Mirrors `step_match`: only the scrutinee is ever searched into (which
        // arm ends up chosen depends on the scrutinee's *value*, not on anything
        // highlighting can see ahead of time).
        ExprKind::Match(scrutinee, _) => highlight_next_unary(expr, scrutinee),
        ExprKind::Let(decls, _) => highlight_next_let(expr, decls),
        ExprKind::Tuple(items) => highlight_next_tuple(items),
        // `step_app` searches `f` then `arg` and fires once both are values —
        // exactly `step_binop`'s shape, so exactly the same search.
        ExprKind::App(f, arg) => highlight_next_binop(expr, f, arg),
    }
}

/// Mirrors `step_binop`/`step_app`'s search: left first, then right; once both
/// are values, `whole` itself is the next redex.
fn highlight_next_binop(whole: &Expr, l: &Expr, r: &Expr) -> Option<NodeId> {
    if !is_value(l) {
        return highlight_next_expr(l);
    }
    if !is_value(r) {
        return highlight_next_expr(r);
    }
    Some(whole.id)
}

/// The shape shared by every rule that inspects exactly one operand before
/// firing: search into it while it's still reducing, and once it's a value,
/// `whole` is the redex. If it isn't actually the right *type*, `step` will panic
/// on it when clicked — that's expected (see `eval.rs`) rather than something
/// this preview needs to pre-empt.
fn highlight_next_unary(whole: &Expr, operand: &Expr) -> Option<NodeId> {
    if !is_value(operand) {
        return highlight_next_expr(operand);
    }
    Some(whole.id)
}

/// Mirrors `step_tuple`'s left-to-right search; once every element is a value the
/// tuple already is one too, so (unlike `highlight_next_binop`) there's no
/// "whole" case that highlights the tuple itself.
fn highlight_next_tuple(items: &[Expr]) -> Option<NodeId> {
    items
        .iter()
        .find(|item| !is_value(item))
        .and_then(highlight_next_expr)
}

/// Mirrors `step_let`, which in turn mirrors `highlight_next`'s own top-level
/// decl-processing shape: reduce the first decl that isn't a value yet, otherwise
/// that decl is about to be substituted away and is itself what's next. Once
/// `decls` runs out the whole `Let` collapses into its body, so it's the redex.
fn highlight_next_let(whole: &Expr, decls: &[Decl]) -> Option<NodeId> {
    let Some(first) = decls.first() else {
        return Some(whole.id);
    };
    let expr = &first.get_val_decl().expr;
    highlight_next_expr(expr).or(Some(expr.id))
}
