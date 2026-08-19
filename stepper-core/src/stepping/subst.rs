use crate::ast::{Decl, Expr, HighlightColor, Pattern};
use crate::pretty::{pretty_print_expr, pretty_print_pattern};

/// Replace free occurrences of `name` in `expr` with `value`, marking every
/// replaced occurrence green (a substitution just placed it there).
// For now this is braindead copy. Eventually would like to replace in place
pub fn substitute(expr: &Expr, name: &str, value: &Expr) -> Expr {
    match expr {
        Expr::Ident(id) => {
            if id == name {
                Expr::Highlighted(Box::new(value.clone()), HighlightColor::Green)
            } else {
                expr.clone()
            }
        }
        Expr::IntConst(_) | Expr::BoolConst(_) => expr.clone(),
        Expr::Add(l, r) => Expr::Add(
            Box::new(substitute(l, name, value)),
            Box::new(substitute(r, name, value)),
        ),
        Expr::Sub(l, r) => Expr::Sub(
            Box::new(substitute(l, name, value)),
            Box::new(substitute(r, name, value)),
        ),
        Expr::Mul(l, r) => Expr::Mul(
            Box::new(substitute(l, name, value)),
            Box::new(substitute(r, name, value)),
        ),
        Expr::Div(l, r) => Expr::Div(
            Box::new(substitute(l, name, value)),
            Box::new(substitute(r, name, value)),
        ),
        Expr::Mod(l, r) => Expr::Mod(
            Box::new(substitute(l, name, value)),
            Box::new(substitute(r, name, value)),
        ),
        Expr::Neg(inner) => Expr::Neg(Box::new(substitute(inner, name, value))),
        Expr::Eq(l, r) => Expr::Eq(
            Box::new(substitute(l, name, value)),
            Box::new(substitute(r, name, value)),
        ),
        Expr::Ne(l, r) => Expr::Ne(
            Box::new(substitute(l, name, value)),
            Box::new(substitute(r, name, value)),
        ),
        Expr::Lt(l, r) => Expr::Lt(
            Box::new(substitute(l, name, value)),
            Box::new(substitute(r, name, value)),
        ),
        Expr::Le(l, r) => Expr::Le(
            Box::new(substitute(l, name, value)),
            Box::new(substitute(r, name, value)),
        ),
        Expr::Gt(l, r) => Expr::Gt(
            Box::new(substitute(l, name, value)),
            Box::new(substitute(r, name, value)),
        ),
        Expr::Ge(l, r) => Expr::Ge(
            Box::new(substitute(l, name, value)),
            Box::new(substitute(r, name, value)),
        ),
        Expr::AndAlso(l, r) => Expr::AndAlso(
            Box::new(substitute(l, name, value)),
            Box::new(substitute(r, name, value)),
        ),
        Expr::OrElse(l, r) => Expr::OrElse(
            Box::new(substitute(l, name, value)),
            Box::new(substitute(r, name, value)),
        ),
        Expr::If(cond, then_branch, else_branch) => Expr::If(
            Box::new(substitute(cond, name, value)),
            Box::new(substitute(then_branch, name, value)),
            Box::new(substitute(else_branch, name, value)),
        ),
        Expr::Let(decls, body) => {
            let (new_decls, shadowed) = substitute_into_decls(decls, name, value);
            let new_body = if shadowed {
                (**body).clone()
            } else {
                substitute(body, name, value)
            };
            Expr::Let(new_decls, Box::new(new_body))
        }
        Expr::Highlighted(inner, color) => {
            Expr::Highlighted(Box::new(substitute(inner, name, value)), *color)
        }
        Expr::Tuple(items) => Expr::Tuple(
            items.iter().map(|item| substitute(item, name, value)).collect(),
        ),
    }
}

/// Whether `pat` binds `name` anywhere within it (through nested tuples).
/// `Wildcard` never binds anything; used by `substitute_into_decls` to decide when
/// a later decl shadows an in-flight substitution.
fn pattern_binds(pat: &Pattern, name: &str) -> bool {
    match pat {
        Pattern::Ident(n) => n == name,
        Pattern::Wildcard | Pattern::IntConst(_) | Pattern::BoolConst(_) => false,
        Pattern::Tuple(pats) => pats.iter().any(|p| pattern_binds(p, name)),
    }
}

/// Matches `pat` against `value` — which must already be a value (see
/// `eval::is_value`), since patterns only ever destructure a decl's fully-reduced
/// right-hand side — returning each identifier `pat` binds paired with the
/// sub-expression it binds to, in the pattern's left-to-right order. `Wildcard` and
/// a matching literal both contribute nothing.
///
/// The typechecker has already ruled out any *shape* mismatch (a `Tuple` pattern
/// only ever reaches here against an `Expr::Tuple` of the same arity, `IntConst`
/// only against an `Expr::IntConst`, etc.), so those are `unreachable!()`, not a
/// panic message — a well-typed program can't hit them. A literal *value*
/// mismatch is different: no typechecker can rule out `val 5 = 2 + 2` ahead of
/// time, so that's a genuine runtime condition. Real SML raises `Bind` there;
/// until this project has exceptions (see `HighlightColor::Red`) that's a panic
/// instead of a silent non-substitution.
pub(super) fn destructure(pat: &Pattern, value: &Expr) -> Vec<(String, Expr)> {
    match pat {
        Pattern::Wildcard => Vec::new(),
        Pattern::Ident(name) => vec![(name.clone(), value.clone())],
        Pattern::IntConst(n) => {
            let Expr::IntConst(v) = value else { unreachable!("typechecked: IntConst pattern only meets an int") };
            if v != n {
                panic!(
                    "Bind failure: pattern `{}` does not match value `{}`",
                    pretty_print_pattern(pat),
                    pretty_print_expr(value)
                );
            }
            Vec::new()
        }
        Pattern::BoolConst(b) => {
            let Expr::BoolConst(v) = value else { unreachable!("typechecked: BoolConst pattern only meets a bool") };
            if v != b {
                panic!(
                    "Bind failure: pattern `{}` does not match value `{}`",
                    pretty_print_pattern(pat),
                    pretty_print_expr(value)
                );
            }
            Vec::new()
        }
        Pattern::Tuple(pats) => {
            let Expr::Tuple(values) = value else { unreachable!("typechecked: Tuple pattern only meets a same-arity tuple") };
            pats.iter()
                .zip(values)
                .flat_map(|(p, v)| destructure(p, v))
                .collect()
        }
    }
}

/// Substitute `name = value` into each decl in `decls` in order, stopping as soon
/// as a decl's pattern rebinds `name` — that decl's own right-hand side still
/// receives the substitution (it's evaluated in the scope *before* its own binding
/// takes effect), but nothing after it does, since `name` is shadowed from that
/// point on. Returns the new decls and whether shadowing occurred, so callers with
/// a trailing expression that sees this scope (a `let` body, or `step`'s "rest of
/// the program") know whether to substitute into it too.
pub(super) fn substitute_into_decls(decls: &[Decl], name: &str, value: &Expr) -> (Vec<Decl>, bool) {
    let mut result = Vec::with_capacity(decls.len());
    let mut shadowed = false;
    for d in decls {
        if shadowed {
            result.push(d.clone());
            continue;
        }
        result.push(Decl {
            pat: d.pat.clone(),
            ty: d.ty.clone(),
            expr: substitute(&d.expr, name, value),
        });
        if pattern_binds(&d.pat, name) {
            shadowed = true;
        }
    }
    (result, shadowed)
}
