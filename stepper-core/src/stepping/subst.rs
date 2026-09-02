use std::collections::HashMap;

use crate::ast::{walk_expr, Binder, BinderId, Decl, Expr, ExprKind, Pattern, PatternBase};
use crate::pretty::{pretty_print_expr, pretty_print_pattern};

/// A copy of `expr` in which every variable it *binds* has a fresh `BinderId`,
/// with the uses inside rewritten to match. Free variables — including a
/// recursive function's reference to itself, which is bound outside — are left
/// alone, and so are node ids and names: this changes identity, not appearance.
///
/// Every duplication of a subtree has to go through here, and the reason is
/// capture. Cloning alone would give the copy the same binder ids as the
/// original, and two live lambdas sharing a binder id are indistinguishable to
/// `substitute`. In continuation-passing style that happens immediately:
///
/// ```text
/// fun factCPS (0 : int) (k : int -> int) : int = k 1
///   | factCPS x k = factCPS (x - 1) (fn res : int => k (x * res))
/// ```
///
/// Each unrolling produces another `fn res => k (x * res)` with the *same* `res`,
/// and `k` is then substituted with a previous one, nesting a `res` inside a `res`
/// that is literally the same variable. Applying the outer one would reach into
/// the inner one's body and `factCPS 3 (fn x => x)` would quietly come out as 3
/// instead of 6.
///
/// Renaming on duplication maintains the invariant this relies on: within one
/// live program, each binding site has an id nothing else shares. That's what
/// lets the collection pass below simply gather every binder in the subtree
/// without tracking scopes.
pub(super) fn refresh_binders(expr: &Expr) -> Expr {
    let mut renaming = HashMap::new();
    walk_expr(expr, &mut |e| {
        let patterns: &[(Pattern, Expr)] = match &e.kind {
            ExprKind::Lambda(cases) | ExprKind::Match(_, cases) => cases,
            ExprKind::Let(decls, _) => {
                for decl in decls {
                    for binder in decl.get_val_decl().pat.binders() {
                        renaming.insert(binder.id, BinderId::fresh());
                    }
                }
                &[]
            }
            _ => &[],
        };
        for (pat, _) in patterns {
            for binder in pat.binders() {
                renaming.insert(binder.id, BinderId::fresh());
            }
        }
    });
    if renaming.is_empty() {
        return expr.clone();
    }
    renamed_expr(expr, &renaming)
}

fn renamed_binder(binder: &Binder, renaming: &HashMap<BinderId, BinderId>) -> Binder {
    Binder {
        id: renaming.get(&binder.id).copied().unwrap_or(binder.id),
        name: binder.name.clone(),
    }
}

fn renamed_pattern(pat: &Pattern, renaming: &HashMap<BinderId, BinderId>) -> Pattern {
    let base = match &pat.pat {
        PatternBase::Var(binder) => PatternBase::Var(renamed_binder(binder, renaming)),
        PatternBase::Tuple(pats) => {
            PatternBase::Tuple(pats.iter().map(|p| renamed_pattern(p, renaming)).collect())
        }
        other => other.clone(),
    };
    Pattern {
        id: pat.id,
        pat: base,
        typ: pat.typ.clone(),
    }
}

fn renamed_cases(
    cases: &[(Pattern, Expr)],
    renaming: &HashMap<BinderId, BinderId>,
) -> Vec<(Pattern, Expr)> {
    cases
        .iter()
        .map(|(pat, body)| (renamed_pattern(pat, renaming), renamed_expr(body, renaming)))
        .collect()
}

fn renamed_decls(decls: &[Decl], renaming: &HashMap<BinderId, BinderId>) -> Vec<Decl> {
    decls
        .iter()
        .map(|d| {
            let val_decl = d.get_val_decl();
            d.copy_val_type(crate::ast::ValDecl {
                pat: renamed_pattern(&val_decl.pat, renaming),
                expr: renamed_expr(&val_decl.expr, renaming),
            })
        })
        .collect()
}

fn renamed_expr(expr: &Expr, renaming: &HashMap<BinderId, BinderId>) -> Expr {
    let recur = |e: &Expr| Box::new(renamed_expr(e, renaming));
    let kind = match &expr.kind {
        ExprKind::Var(binder) => ExprKind::Var(renamed_binder(binder, renaming)),
        ExprKind::IntConst(_) | ExprKind::BoolConst(_) => return expr.clone(),
        ExprKind::Add(l, r) => ExprKind::Add(recur(l), recur(r)),
        ExprKind::Sub(l, r) => ExprKind::Sub(recur(l), recur(r)),
        ExprKind::Mul(l, r) => ExprKind::Mul(recur(l), recur(r)),
        ExprKind::Div(l, r) => ExprKind::Div(recur(l), recur(r)),
        ExprKind::Mod(l, r) => ExprKind::Mod(recur(l), recur(r)),
        ExprKind::Neg(inner) => ExprKind::Neg(recur(inner)),
        ExprKind::Eq(l, r) => ExprKind::Eq(recur(l), recur(r)),
        ExprKind::Ne(l, r) => ExprKind::Ne(recur(l), recur(r)),
        ExprKind::Lt(l, r) => ExprKind::Lt(recur(l), recur(r)),
        ExprKind::Le(l, r) => ExprKind::Le(recur(l), recur(r)),
        ExprKind::Gt(l, r) => ExprKind::Gt(recur(l), recur(r)),
        ExprKind::Ge(l, r) => ExprKind::Ge(recur(l), recur(r)),
        ExprKind::AndAlso(l, r) => ExprKind::AndAlso(recur(l), recur(r)),
        ExprKind::OrElse(l, r) => ExprKind::OrElse(recur(l), recur(r)),
        ExprKind::App(f, arg) => ExprKind::App(recur(f), recur(arg)),
        ExprKind::If(c, t, e) => ExprKind::If(recur(c), recur(t), recur(e)),
        ExprKind::Tuple(items) => {
            ExprKind::Tuple(items.iter().map(|i| renamed_expr(i, renaming)).collect())
        }
        ExprKind::Let(decls, body) => {
            ExprKind::Let(renamed_decls(decls, renaming), recur(body))
        }
        ExprKind::Match(scrutinee, arms) => {
            ExprKind::Match(recur(scrutinee), renamed_cases(arms, renaming))
        }
        ExprKind::Lambda(cases) => ExprKind::Lambda(renamed_cases(cases, renaming)),
    };
    expr.same_id(kind)
}

/// Replace every use of the variable `target` in `expr` with `value`.
///
/// There is no capture-avoidance and no shadowing check here, and none is
/// needed: `target` names one binding site, so a use either refers to it or
/// refers to a different variable with a different id. An inner `fn x => ...`
/// that happens to spell its parameter the same way binds a *different* id, so
/// its body's `x`s simply aren't matched. That's the whole reason variables carry
/// binder ids rather than names (see `frontend::resolve`) — before that, this
/// function had to stop at every `let`, `fn` case, and `case` arm that rebound
/// the name, and `substitute_into_decls` had to report back whether it had.
///
/// The copies placed here keep `value`'s node ids rather than getting fresh ones
/// (see `NodeId`), which is what lets the caller mark everything this
/// substitution just placed green by naming a single id.
pub(super) fn substitute(expr: &Expr, target: BinderId, value: &Expr) -> Expr {
    let recur = |e: &Expr| Box::new(substitute(e, target, value));
    let kind = match &expr.kind {
        ExprKind::Var(binder) => {
            if binder.id == target {
                // Each placement gets its own binders, so two copies of the same
                // value can be stepped independently and neither can capture the
                // other's variables. See `refresh_binders`.
                return refresh_binders(value);
            }
            return expr.clone();
        }
        ExprKind::IntConst(_) | ExprKind::BoolConst(_) => return expr.clone(),
        ExprKind::Add(l, r) => ExprKind::Add(recur(l), recur(r)),
        ExprKind::Sub(l, r) => ExprKind::Sub(recur(l), recur(r)),
        ExprKind::Mul(l, r) => ExprKind::Mul(recur(l), recur(r)),
        ExprKind::Div(l, r) => ExprKind::Div(recur(l), recur(r)),
        ExprKind::Mod(l, r) => ExprKind::Mod(recur(l), recur(r)),
        ExprKind::Neg(inner) => ExprKind::Neg(recur(inner)),
        ExprKind::Eq(l, r) => ExprKind::Eq(recur(l), recur(r)),
        ExprKind::Ne(l, r) => ExprKind::Ne(recur(l), recur(r)),
        ExprKind::Lt(l, r) => ExprKind::Lt(recur(l), recur(r)),
        ExprKind::Le(l, r) => ExprKind::Le(recur(l), recur(r)),
        ExprKind::Gt(l, r) => ExprKind::Gt(recur(l), recur(r)),
        ExprKind::Ge(l, r) => ExprKind::Ge(recur(l), recur(r)),
        ExprKind::AndAlso(l, r) => ExprKind::AndAlso(recur(l), recur(r)),
        ExprKind::OrElse(l, r) => ExprKind::OrElse(recur(l), recur(r)),
        ExprKind::App(f, arg) => ExprKind::App(recur(f), recur(arg)),
        ExprKind::If(cond, then_branch, else_branch) => {
            ExprKind::If(recur(cond), recur(then_branch), recur(else_branch))
        }
        ExprKind::Let(decls, body) => {
            ExprKind::Let(substitute_into_decls(decls, target, value), recur(body))
        }
        ExprKind::Tuple(items) => ExprKind::Tuple(
            items
                .iter()
                .map(|item| substitute(item, target, value))
                .collect(),
        ),
        ExprKind::Match(scrutinee, arms) => {
            ExprKind::Match(recur(scrutinee), substitute_into_cases(arms, target, value))
        }
        ExprKind::Lambda(cases) => {
            ExprKind::Lambda(substitute_into_cases(cases, target, value))
        }
    };
    expr.same_id(kind)
}

/// The `(pattern, body)` cases of a `fn` or a `case`. Every arm gets the
/// substitution unconditionally — an arm whose pattern rebinds the same *name*
/// binds a different id, so its body never mentions `target` to begin with.
fn substitute_into_cases(
    cases: &[(Pattern, Expr)],
    target: BinderId,
    value: &Expr,
) -> Vec<(Pattern, Expr)> {
    cases
        .iter()
        .map(|(pat, body)| (pat.clone(), substitute(body, target, value)))
        .collect()
}

/// Substitutes into each decl's right-hand side, all the way down the list.
///
/// This used to stop at the first decl whose pattern rebound the name and report
/// back that it had, so callers knew whether to substitute into a trailing `let`
/// body too. With binder ids there's nothing to stop for: `val x = 5  val x = 10
/// val y = x + 1` gives `y`'s `x` the *second* decl's id, so substituting the
/// first decl's binding walks right past it and correctly leaves it alone.
pub(super) fn substitute_into_decls(decls: &[Decl], target: BinderId, value: &Expr) -> Vec<Decl> {
    decls
        .iter()
        .map(|d| d.new_expr(substitute(&d.get_val_decl().expr, target, value)))
        .collect()
}

/// Matches `pat` against `value` — which must already be a value (see
/// `eval::is_value`), since patterns only ever match a fully-reduced expression —
/// returning each variable `pat` binds paired with the sub-expression it binds
/// to, in the pattern's left-to-right order, or `None` if a literal pattern
/// doesn't match `value`. `Wildcard` and a matching literal both contribute
/// nothing (`Some(vec![])`, not `None`).
///
/// The typechecker has already ruled out any *shape* mismatch (a `Tuple` pattern
/// only ever reaches here against an `ExprKind::Tuple` of the same arity,
/// `IntConst` only against an `ExprKind::IntConst`, etc.), so those are
/// `unreachable!()`, not a `None` — a well-typed program can't hit them. A
/// literal *value* mismatch is different: no typechecker can rule out e.g.
/// `val 5 = 2 + 2` ahead of time, so that's a genuine runtime condition callers
/// decide how to handle — `destructure` treats it as fatal (there's only ever one
/// pattern to satisfy), while `stepping::eval::step_match` uses `None` here to
/// move on and try the next arm.
pub(super) fn try_match(pat: &Pattern, value: &Expr) -> Option<Vec<(BinderId, Expr)>> {
    match &pat.pat {
        PatternBase::Wildcard => Some(Vec::new()),
        PatternBase::Var(binder) => Some(vec![(binder.id, value.clone())]),
        PatternBase::IntConst(n) => {
            let ExprKind::IntConst(v) = &value.kind else {
                unreachable!("typechecked: IntConst pattern only meets an int")
            };
            (v == n).then(Vec::new)
        }
        PatternBase::BoolConst(b) => {
            let ExprKind::BoolConst(v) = &value.kind else {
                unreachable!("typechecked: BoolConst pattern only meets a bool")
            };
            (v == b).then(Vec::new)
        }
        PatternBase::Tuple(pats) => {
            let ExprKind::Tuple(values) = &value.kind else {
                unreachable!("typechecked: Tuple pattern only meets a same-arity tuple")
            };
            let mut bindings = Vec::new();
            for (p, v) in pats.iter().zip(values) {
                bindings.extend(try_match(p, v)?);
            }
            Some(bindings)
        }
    }
}

/// `val` decls only ever have one pattern to satisfy, so a mismatch here is
/// unconditionally fatal — real SML raises `Bind`; until this project has
/// exceptions (see `view::HighlightColor::Red`) that's a panic instead.
pub(super) fn destructure(pat: &Pattern, value: &Expr) -> Vec<(BinderId, Expr)> {
    try_match(pat, value).unwrap_or_else(|| {
        panic!(
            "Bind failure: pattern `{}` does not match value `{}`",
            pretty_print_pattern(pat),
            pretty_print_expr(value)
        )
    })
}
