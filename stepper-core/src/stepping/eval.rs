use crate::ast::*;
use crate::pretty::{pretty_print_expr, pretty_print_pattern};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use super::highlight::{strip_highlight_expr, strip_highlights};
use super::subst::{destructure, substitute, substitute_into_decls, try_match};

#[derive(Debug, PartialEq)]
pub struct StepOutcome {
    pub program: Program,
    pub message: String,
}

/// Where `val rec` puts its lambdas. A recursive function can't be substituted
/// into its scope the way a `val` value is — its body mentions its own name, so
/// inlining the lambda at every use would leave that name dangling (and, done
/// self-referentially, would blow the displayed program up by a whole copy of the
/// function per unrolling). Instead the lambda is parked here under a name that's
/// unique for this program run, and every use in scope is substituted with that
/// *name*; `step_ident` looks it back up and unrolls one copy whenever the name
/// reaches a redex position. Keeping the name in the program is what keeps
/// `fact (n - 1)` readable in the display.
type RecursiveValueEnv = HashMap<String, Expr>;

thread_local! {
    static REC_ENV: RefCell<RecursiveValueEnv> = RefCell::new(RecursiveValueEnv::new());
}

/// Forget every recursive binding. Called when a new program is entered
/// (`lib::enter_formula`) so its names don't have to dodge a previous run's.
pub fn reset_rec_env() {
    REC_ENV.with(|env| env.borrow_mut().clear());
}

/// The lambda `name` was parked under by `bind_rec`, if it names a recursive
/// function at all. Also used by `highlight_next` to mirror `step_ident`.
pub(super) fn lookup_rec(name: &str) -> Option<Expr> {
    REC_ENV.with(|env| env.borrow().get(name).cloned())
}

/// `0 -> ""`, `1 -> "A"`, ... `26 -> "Z"`, `27 -> "AA"`. Letters only, since
/// that's all `grammar.l` accepts in an identifier — a renamed function still has
/// to print as something the program could be re-entered as.
fn alpha_suffix(mut n: usize) -> String {
    let mut suffix = String::new();
    while n > 0 {
        n -= 1;
        suffix.insert(0, (b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    suffix
}

/// `lambda` with its own recursive references renamed from `from` to `to`, and
/// without the green markers `substitute` would otherwise leave behind (this is a
/// rename, not a reduction the user just watched happen).
fn rename_self_refs(lambda: &Expr, from: &str, to: &str) -> Expr {
    if from == to {
        return lambda.clone();
    }
    strip_highlight_expr(&substitute(lambda, from, &Expr::Ident(to.to_string())))
}

/// Every identifier bound by a pattern anywhere in `decls` (and `body`, for a
/// `let`), however deeply nested. Deliberately blind to scoping — it exists only
/// to steer `bind_rec` off a name, so over-reporting costs an occasional needless
/// rename while under-reporting would cost correctness.
fn bound_names(decls: &[Decl], body: Option<&Expr>) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_decl_names(decls, &mut names);
    if let Some(body) = body {
        collect_expr_names(body, &mut names);
    }
    names
}

fn collect_pattern_names(pat: &Pattern, out: &mut HashSet<String>) {
    match &pat.pat {
        PatternBase::Ident(name) => {
            out.insert(name.clone());
        }
        PatternBase::Wildcard | PatternBase::IntConst(_) | PatternBase::BoolConst(_) => {}
        PatternBase::Tuple(pats) => pats.iter().for_each(|p| collect_pattern_names(p, out)),
    }
}

fn collect_decl_names(decls: &[Decl], out: &mut HashSet<String>) {
    for decl in decls {
        let ValDecl { pat, expr } = decl.get_val_decl();
        collect_pattern_names(pat, out);
        collect_expr_names(expr, out);
    }
}

fn collect_expr_names(expr: &Expr, out: &mut HashSet<String>) {
    match expr {
        Expr::IntConst(_) | Expr::BoolConst(_) | Expr::Ident(_) => {}
        Expr::Neg(inner) | Expr::Highlighted(inner, _) => collect_expr_names(inner, out),
        Expr::Add(l, r)
        | Expr::Sub(l, r)
        | Expr::Mul(l, r)
        | Expr::Div(l, r)
        | Expr::Mod(l, r)
        | Expr::Eq(l, r)
        | Expr::Ne(l, r)
        | Expr::Lt(l, r)
        | Expr::Le(l, r)
        | Expr::Gt(l, r)
        | Expr::Ge(l, r)
        | Expr::AndAlso(l, r)
        | Expr::OrElse(l, r)
        | Expr::App(l, r) => {
            collect_expr_names(l, out);
            collect_expr_names(r, out);
        }
        Expr::If(cond, then_branch, else_branch) => {
            collect_expr_names(cond, out);
            collect_expr_names(then_branch, out);
            collect_expr_names(else_branch, out);
        }
        Expr::Let(decls, body) => {
            collect_decl_names(decls, out);
            collect_expr_names(body, out);
        }
        Expr::Tuple(items) => items.iter().for_each(|i| collect_expr_names(i, out)),
        Expr::Match(scrutinee, arms) => {
            collect_expr_names(scrutinee, out);
            for (pat, arm_expr) in arms {
                collect_pattern_names(pat, out);
                collect_expr_names(arm_expr, out);
            }
        }
        Expr::Lambda(cases) => {
            for (pat, arm_expr) in cases {
                collect_pattern_names(pat, out);
                collect_expr_names(arm_expr, out);
            }
        }
    }
}

/// Parks `lambda` in `REC_ENV` under `name` — or under the first free variant of
/// it (`fact`, `factA`, `factB`, ...) — and returns the name actually used
/// alongside the lambda as stored (self-references rewritten to that name).
///
/// A variant is needed when `name` already holds a *different* function (two
/// `val rec fact = ...` decls in one program are two different functions, and a
/// value substituted out of the earlier scope may still refer to the earlier one),
/// and equally when anything in `scope` *rebinds* `name` later: the references
/// planted for this function are ordinary `Expr::Ident`s, so a later binding of
/// the same name would substitute right over them once a value carrying one gets
/// moved into its scope. Re-binding an identical lambda reuses its name rather
/// than inventing another, so re-entering the same program doesn't accumulate
/// renames.
fn bind_rec(name: &str, lambda: &Expr, scope: &HashSet<String>) -> (String, Expr) {
    for n in 0.. {
        let candidate = format!("{name}{}", alpha_suffix(n));
        let value = rename_self_refs(lambda, name, &candidate);
        let taken = matches!(lookup_rec(&candidate), Some(existing) if existing != value)
            || scope.contains(&candidate);
        if taken {
            continue;
        }
        REC_ENV.with(|env| env.borrow_mut().insert(candidate.clone(), value.clone()));
        return (candidate, value);
    }
    unreachable!("0.. never runs out of candidate names")
}

pub(super) fn is_value(expr: &Expr) -> bool {
    match expr {
        Expr::IntConst(_) | Expr::BoolConst(_) => true,
        Expr::Highlighted(inner, _) => is_value(inner),
        Expr::Tuple(items) => items.iter().all(is_value),
        Expr::Lambda(..) => true,
        _ => false,
    }
}



/// SML's `div`/`mod` floor toward negative infinity (unlike Rust's `/`/`%`, which
/// truncate toward zero) — e.g. `~7 div 2` is `~4`, not `~3`.
fn div_floor(a: i64, b: i64) -> i64 {
    let q = a / b;
    let r = a % b;
    if r != 0 && (r < 0) != (b < 0) {
        q - 1
    } else {
        q
    }
}

fn mod_floor(a: i64, b: i64) -> i64 {
    let r = a % b;
    if r != 0 && (r < 0) != (b < 0) {
        r + b
    } else {
        r
    }
}

fn step_expr(expr: &Expr) -> Option<(Expr, String)> {
    match expr {
        Expr::IntConst(_) | Expr::BoolConst(_) | Expr::Highlighted(_, _) | Expr::Lambda(..) => {
            None
        }
        Expr::Ident(name) => step_ident(name),
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
        Expr::AndAlso(l, r) => step_shortcircuit(
            l,
            r,
            Expr::AndAlso,
            |r| r.clone(),
            |_| Expr::BoolConst(false),
            "andalso",
        ),
        Expr::OrElse(l, r) => step_shortcircuit(
            l,
            r,
            Expr::OrElse,
            |_| Expr::BoolConst(true),
            |r| r.clone(),
            "orelse",
        ),
        Expr::If(cond, then_branch, else_branch) => step_if(cond, then_branch, else_branch),
        Expr::Let(decls, body) => step_let(decls, body),
        Expr::Tuple(items) => step_tuple(items),
        Expr::Match(scrutinee, arms) => step_match(scrutinee, arms),
        Expr::App(f, arg) => step_app(f, arg),
    }
}

/// A bare identifier is stuck — every `val` binding was substituted away when its
/// decl was consumed, so there's no environment left to look one up in — *unless*
/// it names a recursive function parked by `bind_rec`, in which case stepping it
/// unrolls one copy of the lambda. That copy still refers to the same name, so the
/// next recursive call unrolls again: this is the only reduction in the stepper
/// that can run forever, exactly as a recursive SML function can.
fn step_ident(name: &str) -> Option<(Expr, String)> {
    let lambda = lookup_rec(name)?;
    let message = format!("Unrolled {name} to {}", pretty_print_expr(&lambda));
    // Green for the same reason `substitute` paints its replacements green: this
    // text is here because the last step put it here.
    Some((
        Expr::Highlighted(Box::new(lambda), HighlightColor::Green),
        message,
    ))
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
        panic!(
            "type error: `{op}` expects a bool, got `{}`",
            pretty_print_expr(l)
        );
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
        panic!(
            "type error: `~` expects an int, got `{}`",
            pretty_print_expr(inner)
        );
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
        _ => panic!(
            "type error: if-condition is not a bool: `{}`",
            pretty_print_expr(cond)
        ),
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

/// What a finished decl (its right-hand side already a value) hands to everything
/// in its scope: the `(name, value)` bindings to substitute, plus the message
/// describing the substitution. Shared by `step_let` and `step`, which differ only
/// in what "its scope" is.
///
/// A plain `val` hands over its pattern's bindings, pointing straight at the value
/// itself. A `val rec` instead parks its lambda (see `REC_ENV`) and hands over the
/// *name* it was parked under, which `step_ident` unrolls on demand.
fn decl_bindings(
    decl: &Decl,
    scope: &[Decl],
    body: Option<&Expr>,
) -> (Vec<(String, Expr)>, String) {
    let ValDecl { pat, expr } = decl.get_val_decl();
    match decl {
        Decl::ValDecl(_) => {
            let bindings = destructure(pat, expr);
            let message = format!(
                "Substituted {} = {}",
                pretty_print_pattern(pat),
                pretty_print_expr(expr)
            );
            (bindings, message)
        }
        Decl::ValRecDecl(_) => {
            // `typecheck::typecheck_val_rec_decl` has already rejected anything
            // but a bare identifier bound to a `fn`, which is what makes exactly
            // one binding (and a lambda to unroll) the only possibility here.
            let PatternBase::Ident(name) = &pat.pat else {
                unreachable!("typechecked: val rec binds a single identifier")
            };
            let scope_names = bound_names(scope, body);
            let (bound_name, _) = bind_rec(name, expr, &scope_names);
            let mut message = format!("Bound recursive {name} = {}", pretty_print_expr(expr));
            if bound_name != *name {
                message.push_str(&format!(", shown as {bound_name} since {name} is taken"));
            }
            (vec![(name.clone(), Expr::Ident(bound_name))], message)
        }
    }
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
fn substitute_bindings(
    decls: &[Decl],
    body: &Expr,
    bindings: &[(String, Expr)],
) -> (Vec<Decl>, Expr) {
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
    let Some(first_decl) = decls.first() else {panic!("Empty `let` expression") };
    let first = first_decl.get_val_decl();

    if !is_value(&first.expr) {
        let (new_expr, msg) = step_expr(&first.expr)?;
        let mut new_decls = decls.to_vec();
        new_decls[0] =  first_decl.new_expr(new_expr);

        return Some((Expr::Let(new_decls, Box::new(body.clone())), msg));
    }

    let (bindings, message) = decl_bindings(first_decl, &decls[1..], Some(body));
    let (rest_decls, new_body) = substitute_bindings(&decls[1..], body, &bindings);
    if rest_decls.is_empty() {
        Some((new_body, message))
    } else {
        Some((Expr::Let(rest_decls, Box::new(new_body)), message))
    }
}

/// `case e of p1 => e1 | ...`: reduce `e` first; once it's a value, find the
/// first arm whose pattern matches it (`subst::try_match`, tried in order) and
/// replace the whole `Match` with that arm's expression, substituting the
/// pattern's bindings into it — the same substitution a `val` decl gets (see
/// `step_let`), just chosen from several candidate patterns instead of the one.
/// No arm matching is a runtime match failure (SML's `Match` exception); until
/// this project has exceptions (see `HighlightColor::Red`) that's a panic, same
/// treatment `destructure` gives an unmatched literal `val` pattern.
fn step_match(scrutinee: &Expr, arms: &[(Pattern, Expr)]) -> Option<(Expr, String)> {
    if !is_value(scrutinee) {
        let (new_scrutinee, msg) = step_expr(scrutinee)?;
        return Some((Expr::Match(Box::new(new_scrutinee), arms.to_vec()), msg));
    }

    let (pat, arm_expr, bindings) = arms
        .iter()
        .find_map(|(pat, arm_expr)| try_match(pat, scrutinee).map(|b| (pat, arm_expr, b)))
        .unwrap_or_else(|| {
            panic!(
                "Match failure: `{}` matches no arm",
                pretty_print_expr(scrutinee)
            )
        });

    let mut result = arm_expr.clone();
    for (name, value) in &bindings {
        result = substitute(&result, name, value);
    }
    let message = format!(
        "Substituted {} = {}",
        pretty_print_pattern(pat),
        pretty_print_expr(scrutinee)
    );
    Some((result, message))
}

/// `e1 e2`: reduce `e1` then `e2` left to right, same order `step_binop` uses. Once
/// both are values, `e1` must be a `Lambda` (the typechecker has already ruled out
/// anything else) — apply it by substituting `e2`'s value for its parameter
/// pattern in its body, the same substitution a `val` decl or `case` arm gets (see
/// `step_let`/`step_match`).
fn step_app(f: &Expr, arg: &Expr) -> Option<(Expr, String)> {
    if !is_value(f) {
        let (new_f, msg) = step_expr(f)?;
        return Some((Expr::App(Box::new(new_f), Box::new(arg.clone())), msg));
    }
    if !is_value(arg) {
        let (new_arg, msg) = step_expr(arg)?;
        return Some((Expr::App(Box::new(f.clone()), Box::new(new_arg)), msg));
    }

    // No type checker gap here in practice — a well-typed `App` always has a
    // `Lambda` on the left once it's a value — but panic rather than pretend to
    // make progress, matching every other "genuine type error" case in this file.
    let Expr::Lambda(cases) = f else {
        panic!(
            "type error: applying a non-function `{}`",
            pretty_print_expr(f)
        );
    };
    step_match(arg, cases)
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
    let program = strip_highlights(program);
    let first = program.first()?;
    let expr = &first.get_val_decl().expr;

    if !is_value(expr) {
        let (new_expr, message) = step_expr(expr)?;
        let mut new_program = program.clone();
        new_program[0] = first.new_expr(new_expr);
        return Some(StepOutcome {
            program: new_program,
            message,
        });
    }

    if program.len() == 1 {
        return None;
    }

    let (bindings, message) = decl_bindings(first, &program[1..], None);
    // Stops substituting each bound name at the first later decl (if any) that
    // rebinds it, matching SML shadowing: `val x = 5  val x = 10  val y = x + 1`
    // must bind y to 11, not 6, since y's `x` refers to the second (nearer)
    // declaration.
    let rest = substitute_bindings_into_decls(&program[1..], &bindings);

    Some(StepOutcome {
        program: rest,
        message,
    })
}
