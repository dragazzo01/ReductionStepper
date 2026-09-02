use std::collections::HashMap;

use crate::ast::{walk_expr, Binder, BinderId, Decl, Expr, ExprKind, NodeId, Pattern, PatternBase};
use crate::pretty::{pretty_print_expr, pretty_print_pattern};

/// An independent copy of `expr`: every node gets a fresh [`NodeId`], and every
/// variable it *binds* a fresh [`BinderId`], with the uses inside rewritten to
/// match. Free variables — including a recursive function's reference to itself,
/// which is bound outside — are left alone, as are names. Nothing about how the
/// copy *reads* changes; only who it is.
///
/// Every duplication of a subtree goes through here, and both halves matter.
///
/// **Fresh binders**, or substitution captures. Cloning alone would give the copy
/// the original's binder ids, and two live lambdas sharing one are
/// indistinguishable to `substitute`. Continuation-passing style hits this
/// immediately:
///
/// ```text
/// fun factCPS (0 : int) (k : int -> int) : int = k 1
///   | factCPS x k = factCPS (x - 1) (fn res : int => k (x * res))
/// ```
///
/// Each unrolling produces another `fn res => k (x * res)` with the *same* `res`,
/// and `k` is then substituted with a previous one, nesting a `res` inside a `res`
/// that is literally the same variable. Applying the outer one would reach into
/// the inner one's body, and `factCPS 3 (fn x => x)` would quietly come out as 3
/// instead of 6.
///
/// **Fresh nodes**, or the display can't tell the copies apart. A `NodeId` is what
/// the view layer points at, and `highlight_next` returns exactly one of them —
/// so if two copies share ids, marking the redex in one paints the other too:
///
/// ```text
/// val a = [y1 * 100y]
/// val b = (fn n : int => [yn * 100y]) 7     <- not being evaluated
/// ```
///
/// It also decides what folding a lambda means: distinct ids make it fold the one
/// you clicked rather than every copy of it at once.
///
/// Together these maintain the invariant the rest of the crate assumes: within one
/// live program, each node and each binding site has an id nothing else shares.
/// That's also what lets the collection pass below gather binders without tracking
/// scopes.
pub(super) fn fresh_copy(expr: &Expr) -> Expr {
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
    Pattern::new(base, pat.typ.clone())
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
        ExprKind::IntConst(n) => ExprKind::IntConst(*n),
        ExprKind::BoolConst(b) => ExprKind::BoolConst(*b),
        ExprKind::BinOp(op, l, r) => ExprKind::BinOp(*op, recur(l), recur(r)),
        ExprKind::Neg(inner) => ExprKind::Neg(recur(inner)),
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
    Expr::new(kind)
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
/// Each placement is an independent [`fresh_copy`], so `value`'s own ids appear
/// nowhere in the result. That's why `placed` exists: it collects the root
/// `NodeId` of every copy made, which is what the caller marks green. Nodes the
/// substitution didn't touch keep the ids they had, so display state attached to
/// them survives the step untouched.
pub(super) fn substitute(
    expr: &Expr,
    target: BinderId,
    value: &Expr,
    placed: &mut Vec<NodeId>,
) -> Expr {
    // A macro rather than a closure: each arm needs `placed` mutably twice, and a
    // closure capturing it can't be called twice inside one expression.
    macro_rules! sub {
        ($e:expr) => {
            Box::new(substitute($e, target, value, placed))
        };
    }
    let kind = match &expr.kind {
        ExprKind::Var(binder) => {
            if binder.id == target {
                let copy = fresh_copy(value);
                placed.push(copy.id);
                return copy;
            }
            return expr.clone();
        }
        ExprKind::IntConst(_) | ExprKind::BoolConst(_) => return expr.clone(),
        ExprKind::BinOp(op, l, r) => ExprKind::BinOp(*op, sub!(l), sub!(r)),
        ExprKind::Neg(inner) => ExprKind::Neg(sub!(inner)),
        ExprKind::AndAlso(l, r) => ExprKind::AndAlso(sub!(l), sub!(r)),
        ExprKind::OrElse(l, r) => ExprKind::OrElse(sub!(l), sub!(r)),
        ExprKind::App(f, arg) => ExprKind::App(sub!(f), sub!(arg)),
        ExprKind::If(cond, then_branch, else_branch) => {
            ExprKind::If(sub!(cond), sub!(then_branch), sub!(else_branch))
        }
        ExprKind::Let(decls, body) => ExprKind::Let(
            substitute_into_decls(decls, target, value, placed),
            sub!(body),
        ),
        ExprKind::Tuple(items) => ExprKind::Tuple(
            items
                .iter()
                .map(|item| substitute(item, target, value, placed))
                .collect(),
        ),
        ExprKind::Match(scrutinee, arms) => {
            let scrutinee = sub!(scrutinee);
            ExprKind::Match(
                scrutinee,
                substitute_into_cases(arms, target, value, placed),
            )
        }
        ExprKind::Lambda(cases) => {
            ExprKind::Lambda(substitute_into_cases(cases, target, value, placed))
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
    placed: &mut Vec<NodeId>,
) -> Vec<(Pattern, Expr)> {
    cases
        .iter()
        .map(|(pat, body)| (pat.clone(), substitute(body, target, value, placed)))
        .collect()
}

/// Substitutes into each decl's right-hand side, all the way down the list.
///
/// This used to stop at the first decl whose pattern rebound the name and report
/// back that it had, so callers knew whether to substitute into a trailing `let`
/// body too. With binder ids there's nothing to stop for: `val x = 5  val x = 10
/// val y = x + 1` gives `y`'s `x` the *second* decl's id, so substituting the
/// first decl's binding walks right past it and correctly leaves it alone.
pub(super) fn substitute_into_decls(
    decls: &[Decl],
    target: BinderId,
    value: &Expr,
    placed: &mut Vec<NodeId>,
) -> Vec<Decl> {
    decls
        .iter()
        .map(|d| d.new_expr(substitute(&d.get_val_decl().expr, target, value, placed)))
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
