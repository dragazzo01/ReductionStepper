use crate::ast::{
    BinOp, Binder, BinderId, Decl, Expr, ExprKind, NodeId, Pattern, Program, ValDecl,
};
use crate::pretty::{pretty_print_expr, pretty_print_pattern};
use std::cell::RefCell;
use std::collections::HashMap;

use super::subst::{destructure, fresh_copy, substitute, substitute_into_decls, try_match};

#[derive(Debug, PartialEq)]
pub struct StepOutcome {
    pub program: Program,
    pub message: String,
    /// Nodes this step just placed — what the display paints green. Ids, not
    /// tree nodes: nothing about the program changed to say "I was placed here",
    /// so nothing has to be un-said before the next step either. The view layer
    /// simply replaces its green set (see `view::ViewState::set_highlights`).
    pub green: Vec<NodeId>,
}

/// One reduction's result, threaded through the private `step_*` helpers.
struct Step {
    expr: Expr,
    message: String,
    green: Vec<NodeId>,
}

impl Step {
    /// A reduction that placed nothing — every arithmetic and control-flow rule.
    fn plain(expr: Expr, message: String) -> Self {
        Step {
            expr,
            message,
            green: Vec::new(),
        }
    }

    /// Rewraps an inner step's result in the enclosing node, keeping that node's
    /// identity: reducing `l` inside `l + r` leaves the `+` the same `+`.
    fn wrap(self, whole: &Expr, kind: impl FnOnce(Box<Expr>) -> ExprKind) -> Self {
        Step {
            expr: whole.same_id(kind(Box::new(self.expr))),
            message: self.message,
            green: self.green,
        }
    }
}

/// Where `val rec` puts its lambdas. A recursive function can't be substituted
/// into its scope the way a `val` value is — its body mentions its own name, so
/// inlining the lambda at every use would leave that name dangling (and, done
/// self-referentially, would blow the displayed program up by a whole copy of the
/// function per unrolling). Instead the lambda is parked here under its binder's
/// id, and `step_ident` unrolls one copy whenever a use of that binder reaches a
/// redex position. Leaving the *use* in the program is what keeps `fact (n - 1)`
/// readable in the display.
///
/// Keyed by `BinderId`, which is why nothing in here has to invent a name. Two
/// `val rec fact` decls in one program are two binders and so two entries, with
/// no risk of one shadowing the other — where this code once had to rename the
/// second to `factA` and print that, both now simply print as `fact`.
type RecursiveValueEnv = HashMap<BinderId, Expr>;

thread_local! {
    static REC_ENV: RefCell<RecursiveValueEnv> = RefCell::new(RecursiveValueEnv::new());
}

/// Forget every recursive binding. Called when a new program is entered
/// (`lib::enter_formula`) so the table doesn't grow across runs.
pub fn reset_rec_env() {
    REC_ENV.with(|env| env.borrow_mut().clear());
}

/// The lambda parked for `binder`, if it names a recursive function at all. Also
/// used by `highlight_next` to mirror `step_ident`.
pub(super) fn lookup_rec(binder: BinderId) -> Option<Expr> {
    REC_ENV.with(|env| env.borrow().get(&binder).cloned())
}

pub(super) fn is_value(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::IntConst(_) | ExprKind::BoolConst(_) => true,
        ExprKind::Tuple(items) => items.iter().all(is_value),
        ExprKind::Lambda(..) => true,
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

fn step_expr(expr: &Expr) -> Option<Step> {
    match &expr.kind {
        ExprKind::IntConst(_) | ExprKind::BoolConst(_) | ExprKind::Lambda(..) => None,
        ExprKind::Var(binder) => step_var(binder),
        ExprKind::BinOp(op, l, r) => step_binop(expr, *op, l, r),
        ExprKind::Neg(inner) => step_neg(expr, inner),
        ExprKind::AndAlso(l, r) => step_shortcircuit(
            expr,
            l,
            r,
            ExprKind::AndAlso,
            |_, r| r.clone(),
            |whole, _| whole.same_id(ExprKind::BoolConst(false)),
            "andalso",
        ),
        ExprKind::OrElse(l, r) => step_shortcircuit(
            expr,
            l,
            r,
            ExprKind::OrElse,
            |whole, _| whole.same_id(ExprKind::BoolConst(true)),
            |_, r| r.clone(),
            "orelse",
        ),
        ExprKind::If(cond, then_branch, else_branch) => {
            step_if(expr, cond, then_branch, else_branch)
        }
        ExprKind::Let(decls, body) => step_let(expr, decls, body),
        ExprKind::Tuple(items) => step_tuple(expr, items),
        ExprKind::Match(scrutinee, arms) => step_match(expr, scrutinee, arms),
        ExprKind::App(f, arg) => step_app(expr, f, arg),
    }
}

/// A bare variable is stuck — every `val` binding was substituted away when its
/// decl was consumed, so there's no environment left to look one up in — *unless*
/// it names a recursive function parked by `bind_rec`, in which case stepping it
/// unrolls one copy of the lambda. That copy still refers to the same binder, so
/// the next recursive call unrolls again: this is the only reduction in the
/// stepper that can run forever, exactly as a recursive SML function can.
///
/// The unrolled copy keeps the parked lambda's node ids rather than the `Var`'s,
/// so every unrolling of `fact` is the same node as far as the display is
/// concerned — collapse it once and every later unrolling comes up folded.
fn step_var(binder: &Binder) -> Option<Step> {
    // Every unrolling is a fresh copy that will be stepped alongside the ones
    // before it, so its nodes and bound variables have to be its own — otherwise
    // a continuation built by one unrolling captures the next one's parameter,
    // and highlighting one unrolling's redex marks them all. See
    // `subst::fresh_copy`.
    let lambda = fresh_copy(&lookup_rec(binder.id)?);
    let message = format!(
        "Unrolled {} to {}",
        binder.name,
        pretty_print_expr(&lambda)
    );
    let green = vec![lambda.id];
    Some(Step {
        expr: lambda,
        message,
        green,
    })
}

/// Steps the first not-yet-value element left to right, leaving the rest untouched;
/// once every element is a value the whole tuple already is one, so there's nothing
/// left to reduce (matches `is_value`).
fn step_tuple(whole: &Expr, items: &[Expr]) -> Option<Step> {
    for (i, item) in items.iter().enumerate() {
        if !is_value(item) {
            let step = step_expr(item)?;
            let mut new_items = items.to_vec();
            new_items[i] = step.expr;
            return Some(Step {
                expr: whole.same_id(ExprKind::Tuple(new_items)),
                message: step.message,
                green: step.green,
            });
        }
    }
    None
}

/// `andalso`/`orelse`: reduce `l` first; once it's a bool, the result is picked by
/// `on_true`/`on_false` from `l`'s value and `r` — `r` is only ever touched (via
/// further stepping, on a later call) when the picker actually returns it, so a
/// short-circuited `r` is discarded completely unevaluated, same as `step_if`.
fn step_shortcircuit(
    whole: &Expr,
    l: &Expr,
    r: &Expr,
    rebuild: fn(Box<Expr>, Box<Expr>) -> ExprKind,
    on_true: fn(&Expr, &Expr) -> Expr,
    on_false: fn(&Expr, &Expr) -> Expr,
    op: &str,
) -> Option<Step> {
    if !is_value(l) {
        let step = step_expr(l)?;
        return Some(step.wrap(whole, |new_l| rebuild(new_l, Box::new(r.clone()))));
    }

    // The typechecker has already ruled this out, but a non-bool left operand
    // here is a genuine type error — panic rather than pretend to make progress.
    let ExprKind::BoolConst(lb) = &l.kind else {
        panic!(
            "type error: `{op}` expects a bool, got `{}`",
            pretty_print_expr(l)
        );
    };
    // Both pickers get `whole` and `r`: the one that yields a constant builds it
    // with `whole`'s id, so the node keeps its place on screen, while the one that
    // yields `r` hands back `r` with its own ids intact.
    let result = if *lb {
        on_true(whole, r)
    } else {
        on_false(whole, r)
    };
    let message = format!(
        "Evaluated {} to {}",
        pretty_print_expr(whole),
        pretty_print_expr(&result)
    );
    Some(Step::plain(result, message))
}

fn step_neg(whole: &Expr, inner: &Expr) -> Option<Step> {
    if !is_value(inner) {
        let step = step_expr(inner)?;
        return Some(step.wrap(whole, ExprKind::Neg));
    }

    // The typechecker has already ruled this out, but a non-int operand here is a
    // genuine type error — panic rather than pretend to make progress.
    let ExprKind::IntConst(n) = &inner.kind else {
        panic!(
            "type error: `~` expects an int, got `{}`",
            pretty_print_expr(inner)
        );
    };
    let result = whole.same_id(ExprKind::IntConst(n.checked_neg().unwrap_or(*n)));
    let message = format!(
        "Evaluated ~{} to {}",
        pretty_print_expr(inner),
        pretty_print_expr(&result)
    );
    Some(Step::plain(result, message))
}

/// `if cond then t else e`: reduce `cond` first; once it's a value, the whole node
/// is replaced by whichever branch was chosen — the *other* branch is discarded
/// unevaluated, matching normal short-circuiting if-semantics.
fn step_if(whole: &Expr, cond: &Expr, then_branch: &Expr, else_branch: &Expr) -> Option<Step> {
    if !is_value(cond) {
        let step = step_expr(cond)?;
        return Some(step.wrap(whole, |new_cond| {
            ExprKind::If(
                new_cond,
                Box::new(then_branch.clone()),
                Box::new(else_branch.clone()),
            )
        }));
    }

    // The typechecker has already ruled this out, but a non-bool condition here is
    // a genuine type error — panic rather than pretend to make progress.
    let taken = match &cond.kind {
        ExprKind::BoolConst(true) => then_branch,
        ExprKind::BoolConst(false) => else_branch,
        _ => panic!(
            "type error: if-condition is not a bool: `{}`",
            pretty_print_expr(cond)
        ),
    };
    let message = format!(
        "Evaluated {} to {}",
        pretty_print_expr(whole),
        pretty_print_expr(taken)
    );
    Some(Step::plain(taken.clone(), message))
}

/// What a finished decl (its right-hand side already a value) hands to everything
/// in its scope: the `(variable, value)` bindings to substitute, plus the message
/// describing the substitution. Shared by `step_let` and `step`, which differ only
/// in what "its scope" is.
///
/// A plain `val` hands over its pattern's bindings, pointing straight at the value
/// itself. A `val rec` hands over *nothing*: it parks its lambda under its own
/// binder id (see `REC_ENV`) and every use in scope already refers to that binder,
/// so there's nothing to rewrite. Before variables carried binder ids this had to
/// substitute the function's name for a freshly-invented unique spelling of it,
/// which is what the old `factA` renaming was for.
fn decl_bindings(decl: &Decl) -> (Vec<(BinderId, Expr)>, String) {
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
            // one binder (and a lambda to unroll) the only possibility here.
            let [binder] = pat.binders()[..] else {
                unreachable!("typechecked: val rec binds a single identifier")
            };
            REC_ENV.with(|env| env.borrow_mut().insert(binder.id, expr.clone()));
            let message = format!(
                "Bound recursive {} = {}",
                binder.name,
                pretty_print_expr(expr)
            );
            (Vec::new(), message)
        }
    }
}

/// Applies every binding a decl produced to `decls`, collecting into `green` the
/// id of each copy the substitutions placed — one per placement, since every copy
/// is its own node.
fn apply_bindings_to_decls(
    decls: &[Decl],
    bindings: &[(BinderId, Expr)],
    green: &mut Vec<NodeId>,
) -> Vec<Decl> {
    let mut decls = decls.to_vec();
    for (binder, value) in bindings {
        decls = substitute_into_decls(&decls, *binder, value, green);
    }
    decls
}

/// `let decls in body end`: the same decl-processing shape as the top-level
/// program (reduce the first not-yet-value decl, or substitute it into the rest
/// once it is one), except once `decls` runs out the whole `Let` collapses into
/// `body`, which then continues stepping on its own via later calls.
fn step_let(whole: &Expr, decls: &[Decl], body: &Expr) -> Option<Step> {
    let Some(first_decl) = decls.first() else {
        panic!("Empty `let` expression")
    };
    let first = first_decl.get_val_decl();

    if !is_value(&first.expr) {
        let step = step_expr(&first.expr)?;
        let mut new_decls = decls.to_vec();
        new_decls[0] = first_decl.new_expr(step.expr);
        return Some(Step {
            expr: whole.same_id(ExprKind::Let(new_decls, Box::new(body.clone()))),
            message: step.message,
            green: step.green,
        });
    }

    let (bindings, message) = decl_bindings(first_decl);
    let mut green = Vec::new();
    let rest_decls = apply_bindings_to_decls(&decls[1..], &bindings, &mut green);
    let mut new_body = body.clone();
    for (binder, value) in &bindings {
        new_body = substitute(&new_body, *binder, value, &mut green);
    }

    let expr = if rest_decls.is_empty() {
        new_body
    } else {
        whole.same_id(ExprKind::Let(rest_decls, Box::new(new_body)))
    };
    Some(Step {
        expr,
        message,
        green,
    })
}

/// `case e of p1 => e1 | ...`: reduce `e` first; once it's a value, find the
/// first arm whose pattern matches it (`subst::try_match`, tried in order) and
/// replace the whole `Match` with that arm's expression, substituting the
/// pattern's bindings into it — the same substitution a `val` decl gets (see
/// `step_let`), just chosen from several candidate patterns instead of the one.
/// No arm matching is a runtime match failure (SML's `Match` exception); until
/// this project has exceptions (see `view::HighlightColor::Red`) that's a panic,
/// same treatment `destructure` gives an unmatched literal `val` pattern.
fn step_match(whole: &Expr, scrutinee: &Expr, arms: &[(Pattern, Expr)]) -> Option<Step> {
    if !is_value(scrutinee) {
        let step = step_expr(scrutinee)?;
        return Some(step.wrap(whole, |new_scrutinee| {
            ExprKind::Match(new_scrutinee, arms.to_vec())
        }));
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
    let mut green = Vec::new();
    for (binder, value) in &bindings {
        result = substitute(&result, *binder, value, &mut green);
    }
    let message = format!(
        "Substituted {} = {}",
        pretty_print_pattern(pat),
        pretty_print_expr(scrutinee)
    );
    Some(Step {
        expr: result,
        message,
        green,
    })
}

/// `e1 e2`: reduce `e1` then `e2` left to right, same order `step_binop` uses. Once
/// both are values, `e1` must be a `Lambda` (the typechecker has already ruled out
/// anything else) — apply it by substituting `e2`'s value for its parameter
/// pattern in its body, the same substitution a `val` decl or `case` arm gets (see
/// `step_let`/`step_match`).
fn step_app(whole: &Expr, f: &Expr, arg: &Expr) -> Option<Step> {
    if !is_value(f) {
        let step = step_expr(f)?;
        return Some(step.wrap(whole, |new_f| {
            ExprKind::App(new_f, Box::new(arg.clone()))
        }));
    }
    if !is_value(arg) {
        let step = step_expr(arg)?;
        return Some(step.wrap(whole, |new_arg| {
            ExprKind::App(Box::new(f.clone()), new_arg)
        }));
    }

    // No type checker gap here in practice — a well-typed `App` always has a
    // `Lambda` on the left once it's a value — but panic rather than pretend to
    // make progress, matching every other "genuine type error" case in this file.
    let ExprKind::Lambda(cases) = &f.kind else {
        panic!(
            "type error: applying a non-function `{}`",
            pretty_print_expr(f)
        );
    };
    step_match(whole, arg, cases)
}

/// The one reduction rule every [`BinOp`] shares: reduce `l`, then `r`, then
/// combine the two ints with [`apply_binop`].
fn step_binop(whole: &Expr, op: BinOp, l: &Expr, r: &Expr) -> Option<Step> {
    if !is_value(l) {
        let step = step_expr(l)?;
        return Some(step.wrap(whole, |new_l| {
            ExprKind::BinOp(op, new_l, Box::new(r.clone()))
        }));
    }
    if !is_value(r) {
        let step = step_expr(r)?;
        return Some(step.wrap(whole, |new_r| {
            ExprKind::BinOp(op, Box::new(l.clone()), new_r)
        }));
    }

    // The typechecker has already ruled this out, but a non-int operand here is a
    // genuine type error — panic rather than pretend to make progress.
    let (ExprKind::IntConst(a), ExprKind::IntConst(b)) = (&l.kind, &r.kind) else {
        panic!(
            "type error: `{}` expects two ints, got `{}` and `{}`",
            op.symbol(),
            pretty_print_expr(l),
            pretty_print_expr(r)
        );
    };
    let result = whole.same_id(apply_binop(op, *a, *b));
    let message = format!(
        "Evaluated {} {} {} to {}",
        pretty_print_expr(l),
        op.symbol(),
        pretty_print_expr(r),
        pretty_print_expr(&result)
    );
    Some(Step::plain(result, message))
}

/// What each operator actually computes, once both operands are ints.
///
/// A zero divisor should raise SML's `Div` exception; until exceptions exist (see
/// `view::HighlightColor::Red`) `div`/`mod` fall back to 0 rather than panicking.
fn apply_binop(op: BinOp, a: i64, b: i64) -> ExprKind {
    match op {
        BinOp::Add => ExprKind::IntConst(a + b),
        BinOp::Sub => ExprKind::IntConst(a - b),
        BinOp::Mul => ExprKind::IntConst(a * b),
        BinOp::Div => ExprKind::IntConst(if b == 0 { 0 } else { div_floor(a, b) }),
        BinOp::Mod => ExprKind::IntConst(if b == 0 { 0 } else { mod_floor(a, b) }),
        BinOp::Eq => ExprKind::BoolConst(a == b),
        BinOp::Ne => ExprKind::BoolConst(a != b),
        BinOp::Lt => ExprKind::BoolConst(a < b),
        BinOp::Le => ExprKind::BoolConst(a <= b),
        BinOp::Gt => ExprKind::BoolConst(a > b),
        BinOp::Ge => ExprKind::BoolConst(a >= b),
    }
}

/// Perform one reduction step on the whole program. Returns `None` once the program
/// is fully reduced (a single declaration whose expression is already a value).
///
/// Nothing has to be cleaned up from the previous step first: highlights aren't in
/// the tree any more, so the green markers the last step left are the view layer's
/// business, and the `green` ids returned here simply replace them.
pub fn step(program: &Program) -> Option<StepOutcome> {
    let first = program.first()?;
    let expr = &first.get_val_decl().expr;

    if !is_value(expr) {
        let step = step_expr(expr)?;
        let mut new_program = program.to_vec();
        new_program[0] = first.new_expr(step.expr);
        return Some(StepOutcome {
            program: new_program,
            message: step.message,
            green: step.green,
        });
    }

    if program.len() == 1 {
        return None;
    }

    let (bindings, message) = decl_bindings(first);
    let mut green = Vec::new();
    let rest = apply_bindings_to_decls(&program[1..], &bindings, &mut green);

    Some(StepOutcome {
        program: rest,
        message,
        green,
    })
}
