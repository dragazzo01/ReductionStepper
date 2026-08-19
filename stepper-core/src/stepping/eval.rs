use crate::ast::{Decl, Expr, Program};
use crate::pretty::{pretty_print_expr, pretty_print_pattern};

use super::subst::{destructure, substitute, substitute_into_decls};

#[derive(Debug, PartialEq)]
pub struct StepOutcome {
    pub program: Program,
    pub message: String,
}

pub(super) fn is_value(expr: &Expr) -> bool {
    match expr {
        Expr::IntConst(_) | Expr::BoolConst(_) => true,
        Expr::Highlighted(inner, _) => is_value(inner),
        Expr::Tuple(items) => items.iter().all(is_value),
        _ => false,
    }
}

fn strip_highlight_expr(expr: &Expr) -> Expr {
    match expr {
        Expr::Highlighted(inner, _) => strip_highlight_expr(inner),
        Expr::IntConst(_) | Expr::BoolConst(_) | Expr::Ident(_) => expr.clone(),
        Expr::Add(l, r) => Expr::Add(
            Box::new(strip_highlight_expr(l)),
            Box::new(strip_highlight_expr(r)),
        ),
        Expr::Sub(l, r) => Expr::Sub(
            Box::new(strip_highlight_expr(l)),
            Box::new(strip_highlight_expr(r)),
        ),
        Expr::Mul(l, r) => Expr::Mul(
            Box::new(strip_highlight_expr(l)),
            Box::new(strip_highlight_expr(r)),
        ),
        Expr::Div(l, r) => Expr::Div(
            Box::new(strip_highlight_expr(l)),
            Box::new(strip_highlight_expr(r)),
        ),
        Expr::Mod(l, r) => Expr::Mod(
            Box::new(strip_highlight_expr(l)),
            Box::new(strip_highlight_expr(r)),
        ),
        Expr::Neg(inner) => Expr::Neg(Box::new(strip_highlight_expr(inner))),
        Expr::Eq(l, r) => Expr::Eq(
            Box::new(strip_highlight_expr(l)),
            Box::new(strip_highlight_expr(r)),
        ),
        Expr::Ne(l, r) => Expr::Ne(
            Box::new(strip_highlight_expr(l)),
            Box::new(strip_highlight_expr(r)),
        ),
        Expr::Lt(l, r) => Expr::Lt(
            Box::new(strip_highlight_expr(l)),
            Box::new(strip_highlight_expr(r)),
        ),
        Expr::Le(l, r) => Expr::Le(
            Box::new(strip_highlight_expr(l)),
            Box::new(strip_highlight_expr(r)),
        ),
        Expr::Gt(l, r) => Expr::Gt(
            Box::new(strip_highlight_expr(l)),
            Box::new(strip_highlight_expr(r)),
        ),
        Expr::Ge(l, r) => Expr::Ge(
            Box::new(strip_highlight_expr(l)),
            Box::new(strip_highlight_expr(r)),
        ),
        Expr::AndAlso(l, r) => Expr::AndAlso(
            Box::new(strip_highlight_expr(l)),
            Box::new(strip_highlight_expr(r)),
        ),
        Expr::OrElse(l, r) => Expr::OrElse(
            Box::new(strip_highlight_expr(l)),
            Box::new(strip_highlight_expr(r)),
        ),
        Expr::If(cond, then_branch, else_branch) => Expr::If(
            Box::new(strip_highlight_expr(cond)),
            Box::new(strip_highlight_expr(then_branch)),
            Box::new(strip_highlight_expr(else_branch)),
        ),
        Expr::Let(decls, body) => Expr::Let(
            strip_highlight_decls(decls),
            Box::new(strip_highlight_expr(body)),
        ),
        Expr::Tuple(items) => Expr::Tuple(items.iter().map(strip_highlight_expr).collect()),
    }
}

fn strip_highlight_decls(decls: &[Decl]) -> Vec<Decl> {
    decls
        .iter()
        .map(|d| Decl {
            pat: d.pat.clone(),
            ty: d.ty.clone(),
            expr: strip_highlight_expr(&d.expr),
        })
        .collect()
}

/// SML's `div`/`mod` floor toward negative infinity (unlike Rust's `/`/`%`, which
/// truncate toward zero) — e.g. `~7 div 2` is `~4`, not `~3`.
fn div_floor(a: i64, b: i64) -> i64 {
    let q = a / b;
    let r = a % b;
    if r != 0 && (r < 0) != (b < 0) { q - 1 } else { q }
}

fn mod_floor(a: i64, b: i64) -> i64 {
    let r = a % b;
    if r != 0 && (r < 0) != (b < 0) { r + b } else { r }
}

fn step_expr(expr: &Expr) -> Option<(Expr, String)> {
    match expr {
        Expr::IntConst(_) | Expr::BoolConst(_) | Expr::Ident(_) | Expr::Highlighted(_, _) => None,
        Expr::Add(l, r) => step_binop(l, r, Expr::Add, |a, b| Expr::IntConst(a + b), "+"),
        Expr::Sub(l, r) => step_binop(l, r, Expr::Sub, |a, b| Expr::IntConst(a - b), "-"),
        Expr::Mul(l, r) => step_binop(l, r, Expr::Mul, |a, b| Expr::IntConst(a * b), "*"),
        // b == 0 should raise SML's `Div` exception; until exceptions exist
        // (see HighlightColor::Red) this falls back to 0 rather than panicking.
        Expr::Div(l, r) => step_binop(
            l,
            r,
            Expr::Div,
            |a, b| Expr::IntConst(if b == 0 { 0 } else { div_floor(a, b) }),
            "div",
        ),
        Expr::Mod(l, r) => step_binop(
            l,
            r,
            Expr::Mod,
            |a, b| Expr::IntConst(if b == 0 { 0 } else { mod_floor(a, b) }),
            "mod",
        ),
        Expr::Neg(inner) => step_neg(inner),
        Expr::Eq(l, r) => step_binop(l, r, Expr::Eq, |a, b| Expr::BoolConst(a == b), "="),
        Expr::Ne(l, r) => step_binop(l, r, Expr::Ne, |a, b| Expr::BoolConst(a != b), "<>"),
        Expr::Lt(l, r) => step_binop(l, r, Expr::Lt, |a, b| Expr::BoolConst(a < b), "<"),
        Expr::Le(l, r) => step_binop(l, r, Expr::Le, |a, b| Expr::BoolConst(a <= b), "<="),
        Expr::Gt(l, r) => step_binop(l, r, Expr::Gt, |a, b| Expr::BoolConst(a > b), ">"),
        Expr::Ge(l, r) => step_binop(l, r, Expr::Ge, |a, b| Expr::BoolConst(a >= b), ">="),
        Expr::AndAlso(l, r) => {
            step_shortcircuit(l, r, Expr::AndAlso, |r| r.clone(), |_| Expr::BoolConst(false), "andalso")
        }
        Expr::OrElse(l, r) => {
            step_shortcircuit(l, r, Expr::OrElse, |_| Expr::BoolConst(true), |r| r.clone(), "orelse")
        }
        Expr::If(cond, then_branch, else_branch) => step_if(cond, then_branch, else_branch),
        Expr::Let(decls, body) => step_let(decls, body),
        Expr::Tuple(items) => step_tuple(items),
    }
}

/// Steps the first not-yet-value element left to right, leaving the rest untouched;
/// once every element is a value the whole tuple already is one, so there's nothing
/// left to reduce (matches `is_value`).
fn step_tuple(items: &[Expr]) -> Option<(Expr, String)> {
    for (i, item) in items.iter().enumerate() {
        if !is_value(item) {
            let (new_item, msg) = step_expr(item)?;
            let mut new_items = items.to_vec();
            new_items[i] = new_item;
            return Some((Expr::Tuple(new_items), msg));
        }
    }
    None
}

/// `andalso`/`orelse`: reduce `l` first; once it's a bool, the result is picked by
/// `on_true`/`on_false` from `l`'s value and `r` — `r` is only ever touched (via
/// further stepping, on a later call) when the picker actually returns it, so a
/// short-circuited `r` is discarded completely unevaluated, same as `step_if`.
fn step_shortcircuit(
    l: &Expr,
    r: &Expr,
    rebuild: fn(Box<Expr>, Box<Expr>) -> Expr,
    on_true: fn(&Expr) -> Expr,
    on_false: fn(&Expr) -> Expr,
    op: &str,
) -> Option<(Expr, String)> {
    if !is_value(l) {
        let (new_l, msg) = step_expr(l)?;
        return Some((rebuild(Box::new(new_l), Box::new(r.clone())), msg));
    }

    // No type checker exists yet to have ruled this out ahead of time, but a
    // non-bool left operand here is a genuine type error — panic rather than
    // pretend to make progress.
    let Expr::BoolConst(lb) = l else {
        panic!("type error: `{op}` expects a bool, got `{}`", pretty_print_expr(l));
    };
    let whole = rebuild(Box::new(l.clone()), Box::new(r.clone()));
    let result = if *lb { on_true(r) } else { on_false(r) };
    let message = format!(
        "Evaluated {} to {}",
        pretty_print_expr(&whole),
        pretty_print_expr(&result)
    );
    Some((result, message))
}

fn step_neg(inner: &Expr) -> Option<(Expr, String)> {
    if !is_value(inner) {
        let (new_inner, msg) = step_expr(inner)?;
        return Some((Expr::Neg(Box::new(new_inner)), msg));
    }

    // No type checker exists yet to have ruled this out ahead of time, but a
    // non-int operand here is a genuine type error — panic rather than pretend to
    // make progress.
    let Expr::IntConst(n) = inner else {
        panic!("type error: `~` expects an int, got `{}`", pretty_print_expr(inner));
    };
    let result = Expr::IntConst(n.checked_neg().unwrap_or(*n));
    let message = format!(
        "Evaluated ~{} to {}",
        pretty_print_expr(inner),
        pretty_print_expr(&result)
    );
    Some((result, message))
}

/// `if cond then t else e`: reduce `cond` first; once it's a value, the whole node
/// is replaced by whichever branch was chosen — the *other* branch is discarded
/// unevaluated, matching normal short-circuiting if-semantics.
fn step_if(cond: &Expr, then_branch: &Expr, else_branch: &Expr) -> Option<(Expr, String)> {
    if !is_value(cond) {
        let (new_cond, msg) = step_expr(cond)?;
        return Some((
            Expr::If(
                Box::new(new_cond),
                Box::new(then_branch.clone()),
                Box::new(else_branch.clone()),
            ),
            msg,
        ));
    }

    // No type checker exists yet to have ruled this out ahead of time, but a
    // non-bool condition here is a genuine type error — panic rather than pretend
    // to make progress.
    let taken = match cond {
        Expr::BoolConst(true) => then_branch,
        Expr::BoolConst(false) => else_branch,
        _ => panic!("type error: if-condition is not a bool: `{}`", pretty_print_expr(cond)),
    };
    let whole = Expr::If(
        Box::new(cond.clone()),
        Box::new(then_branch.clone()),
        Box::new(else_branch.clone()),
    );
    let message = format!(
        "Evaluated {} to {}",
        pretty_print_expr(&whole),
        pretty_print_expr(taken)
    );
    Some((taken.clone(), message))
}

/// Applies each `(name, value)` binding `destructure` produced for one decl's
/// pattern to `decls`, threading each name's shadow status through independently —
/// matches `substitute_into_decls`'s single-binding contract, just looped once per
/// name a pattern introduces (e.g. `val (x, y) = ...` runs this twice).
fn substitute_bindings_into_decls(decls: &[Decl], bindings: &[(String, Expr)]) -> Vec<Decl> {
    let mut decls = decls.to_vec();
    for (name, value) in bindings {
        decls = substitute_into_decls(&decls, name, value).0;
    }
    decls
}

/// Same idea as `substitute_bindings_into_decls`, but also threads each binding
/// into a trailing `let`-body wherever it isn't shadowed first — mirrors how
/// `step_let` substitutes a single binding into its body today, just looped.
fn substitute_bindings(decls: &[Decl], body: &Expr, bindings: &[(String, Expr)]) -> (Vec<Decl>, Expr) {
    let mut decls = decls.to_vec();
    let mut body = body.clone();
    for (name, value) in bindings {
        let (new_decls, shadowed) = substitute_into_decls(&decls, name, value);
        decls = new_decls;
        if !shadowed {
            body = substitute(&body, name, value);
        }
    }
    (decls, body)
}

/// `let decls in body end`: the same decl-processing shape as the top-level
/// program (reduce the first not-yet-value decl, or substitute it into the rest
/// once it is one), except once `decls` runs out the whole `Let` collapses into
/// `body`, which then continues stepping on its own via later calls.
fn step_let(decls: &[Decl], body: &Expr) -> Option<(Expr, String)> {
    let Some(first) = decls.first() else {panic!("Empty `let` expression") };

    if !is_value(&first.expr) {
        let (new_expr, msg) = step_expr(&first.expr)?;
        let mut new_decls = decls.to_vec();
        new_decls[0] = Decl {
            expr: new_expr,
            ..first.clone()
        };
        return Some((Expr::Let(new_decls, Box::new(body.clone())), msg));
    }

    let bindings = destructure(&first.pat, &first.expr);
    let (rest_decls, new_body) = substitute_bindings(&decls[1..], body, &bindings);
    let message = format!(
        "Substituted {} = {}",
        pretty_print_pattern(&first.pat),
        pretty_print_expr(&first.expr)
    );
    if rest_decls.is_empty() {
        Some((new_body, message))
    } else {
        Some((Expr::Let(rest_decls, Box::new(new_body)), message))
    }
}

fn step_binop(
    l: &Expr,
    r: &Expr,
    rebuild: fn(Box<Expr>, Box<Expr>) -> Expr,
    apply: fn(i64, i64) -> Expr,
    op: &str,
) -> Option<(Expr, String)> {
    if !is_value(l) {
        let (new_l, msg) = step_expr(l)?;
        return Some((rebuild(Box::new(new_l), Box::new(r.clone())), msg));
    }
    if !is_value(r) {
        let (new_r, msg) = step_expr(r)?;
        return Some((rebuild(Box::new(l.clone()), Box::new(new_r)), msg));
    }

    // No type checker exists yet to have ruled this out ahead of time, but a
    // non-int operand here is a genuine type error — panic rather than pretend to
    // make progress.
    let (Expr::IntConst(a), Expr::IntConst(b)) = (l, r) else {
        panic!(
            "type error: `{op}` expects two ints, got `{}` and `{}`",
            pretty_print_expr(l),
            pretty_print_expr(r)
        );
    };
    let result = apply(*a, *b);
    let message = format!(
        "Evaluated {} {} {} to {}",
        pretty_print_expr(l),
        op,
        pretty_print_expr(r),
        pretty_print_expr(&result)
    );
    Some((result, message))
}

/// Perform one reduction step on the whole program. Returns `None` once the program
/// is fully reduced (a single declaration whose expression is already a value).
///
/// Any `Expr::Highlighted` left over from the previous step (namely, green
/// substitution markers — see `substitute`) is stripped first: this is what makes
/// green "go away after the next step". The result is otherwise highlight-free;
/// `highlight_next` (yellow) and `substitute` (green) are the only things that ever
/// introduce a highlight.
pub fn step(program: &Program) -> Option<StepOutcome> {
    let program = strip_highlight_decls(program);
    let first = program.first()?;

    if !is_value(&first.expr) {
        let (new_expr, message) = step_expr(&first.expr)?;
        let mut new_program = program.clone();
        new_program[0] = Decl {
            expr: new_expr,
            ..first.clone()
        };
        return Some(StepOutcome {
            program: new_program,
            message,
        });
    }

    if program.len() == 1 {
        return None;
    }

    let bindings = destructure(&first.pat, &first.expr);
    // Stops substituting each bound name at the first later decl (if any) that
    // rebinds it, matching SML shadowing: `val x = 5  val x = 10  val y = x + 1`
    // must bind y to 11, not 6, since y's `x` refers to the second (nearer)
    // declaration.
    let rest = substitute_bindings_into_decls(&program[1..], &bindings);
    let message = format!(
        "Substituted {} = {}",
        pretty_print_pattern(&first.pat),
        pretty_print_expr(&first.expr)
    );

    Some(StepOutcome {
        program: rest,
        message,
    })
}
