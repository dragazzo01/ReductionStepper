use crate::ast::{Decl, Expr, HighlightColor, Pattern, Program};

use super::eval::{is_value, lookup_rec};


pub(super) fn strip_highlight_expr(expr: &Expr) -> Expr {
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
            strip_highlights(decls),
            Box::new(strip_highlight_expr(body)),
        ),
        Expr::Tuple(items) => Expr::Tuple(items.iter().map(strip_highlight_expr).collect()),
        Expr::Match(scrutinee, arms) => Expr::Match(
            Box::new(strip_highlight_expr(scrutinee)),
            arms.iter()
                .map(|(pat, arm_expr)| (pat.clone(), strip_highlight_expr(arm_expr)))
                .collect(),
        ),
        Expr::Lambda(cases) => Expr::Lambda(
            cases.iter()
                .map(|(pat, arm_expr)| (pat.clone(), strip_highlight_expr(arm_expr)))
                .collect(),
        ),
        Expr::App(f, arg) => Expr::App(
            Box::new(strip_highlight_expr(f)),
            Box::new(strip_highlight_expr(arg)),
        ),
    }
}

pub fn strip_highlights(program: &Program) -> Program {
    program
        .iter()
        .map(|d| 
            d.new_expr(strip_highlight_expr(&d.get_val_decl().expr))
        )
        .collect()
}


/// Find the location of the *next* reduction step in `program` (without performing
/// it) and return a copy with that location wrapped in `Expr::Highlighted` (yellow),
/// for display purposes only. `None` means the program is fully reduced.
///
/// Searches straight through any existing highlight (e.g. a green substitution
/// marker left by the last step) rather than avoiding it: a green-wrapped value
/// still counts as a value, so the yellow wrapper for the surrounding redex ends up
/// nested around it — e.g. `{yellow}{green}10{green} + {green}10{green}{yellow}`.
///
/// This mirrors `step`'s own search order exactly, so what gets highlighted here is
/// always exactly what the next `step` call would act on.
pub fn highlight_next(program: &Program) -> Option<Program> {
    let first = program.first()?;

    if let Some(new_expr) = highlight_next_expr(&first.get_val_decl().expr) {
        let mut new_program = program.clone();
        new_program[0] = first.new_expr(new_expr);
        return Some(new_program);
    }

    if program.len() == 1 {
        return None;
    }

    // The first decl is a value and there's more program left: it's about to be
    // substituted into the rest, so it's what's "next".
    let mut new_program = program.clone();
    new_program[0] = first.new_expr(Expr::Highlighted(Box::new(first.get_val_decl().expr.clone()), HighlightColor::Yellow));
    Some(new_program)
}

fn highlight_next_expr(expr: &Expr) -> Option<Expr> {
    match expr {
        Expr::IntConst(_) | Expr::BoolConst(_) | Expr::Lambda(..) => None,
        // `step` strips highlights before searching, so searching has to see
        // through them too — a green marker from the last step sits on plenty of
        // things that still reduce (a substituted recursive name, most of all).
        // The marker is kept and the yellow one nests inside it.
        Expr::Highlighted(inner, color) => highlight_next_expr(inner)
            .map(|new_inner| Expr::Highlighted(Box::new(new_inner), *color)),
        // Mirrors `step_ident`: an identifier is only a redex when it names a
        // recursive function waiting to be unrolled.
        Expr::Ident(name) => lookup_rec(name)
            .map(|_| Expr::Highlighted(Box::new(expr.clone()), HighlightColor::Yellow)),
        Expr::Add(l, r) => highlight_next_binop(expr, l, r, Expr::Add),
        Expr::Sub(l, r) => highlight_next_binop(expr, l, r, Expr::Sub),
        Expr::Mul(l, r) => highlight_next_binop(expr, l, r, Expr::Mul),
        Expr::Div(l, r) => highlight_next_binop(expr, l, r, Expr::Div),
        Expr::Mod(l, r) => highlight_next_binop(expr, l, r, Expr::Mod),
        Expr::Eq(l, r) => highlight_next_binop(expr, l, r, Expr::Eq),
        Expr::Ne(l, r) => highlight_next_binop(expr, l, r, Expr::Ne),
        Expr::Lt(l, r) => highlight_next_binop(expr, l, r, Expr::Lt),
        Expr::Le(l, r) => highlight_next_binop(expr, l, r, Expr::Le),
        Expr::Gt(l, r) => highlight_next_binop(expr, l, r, Expr::Gt),
        Expr::Ge(l, r) => highlight_next_binop(expr, l, r, Expr::Ge),
        Expr::Neg(inner) => highlight_next_neg(expr, inner),
        Expr::AndAlso(l, r) => highlight_next_shortcircuit(expr, l, r, Expr::AndAlso),
        Expr::OrElse(l, r) => highlight_next_shortcircuit(expr, l, r, Expr::OrElse),
        Expr::If(cond, then_branch, else_branch) => {
            highlight_next_if(expr, cond, then_branch, else_branch)
        }
        Expr::Let(decls, body) => highlight_next_let(expr, decls, body),
        Expr::Tuple(items) => highlight_next_tuple(items),
        Expr::Match(scrutinee, arms) => highlight_next_match(expr, scrutinee, arms),
        Expr::App(f, arg) => highlight_next_app(expr, f, arg),
    }
}

/// Mirrors `step_app`'s search: `f` first, then `arg`; once both are values,
/// `whole` (the entire `App`) is the next redex.
fn highlight_next_app(whole: &Expr, f: &Expr, arg: &Expr) -> Option<Expr> {
    if !is_value(f) {
        let new_f = highlight_next_expr(f)?;
        return Some(Expr::App(Box::new(new_f), Box::new(arg.clone())));
    }
    if !is_value(arg) {
        let new_arg = highlight_next_expr(arg)?;
        return Some(Expr::App(Box::new(f.clone()), Box::new(new_arg)));
    }
    Some(Expr::Highlighted(
        Box::new(whole.clone()),
        HighlightColor::Yellow,
    ))
}

/// Mirrors `step_tuple`'s left-to-right search; once every element is a value the
/// tuple already is one too, so (unlike e.g. `highlight_next_binop`) there's no
/// "whole" case that highlights the tuple itself.
fn highlight_next_tuple(items: &[Expr]) -> Option<Expr> {
    for (i, item) in items.iter().enumerate() {
        if !is_value(item) {
            let new_item = highlight_next_expr(item)?;
            let mut new_items = items.to_vec();
            new_items[i] = new_item;
            return Some(Expr::Tuple(new_items));
        }
    }
    None
}

/// `andalso`/`orelse` only need their left operand to be a value before firing
/// (`r` is never inspected here), unlike `highlight_next_binop` which waits for
/// both — matches `step_shortcircuit`'s short-circuit shape.
fn highlight_next_shortcircuit(
    whole: &Expr,
    l: &Expr,
    r: &Expr,
    rebuild: fn(Box<Expr>, Box<Expr>) -> Expr,
) -> Option<Expr> {
    if !is_value(l) {
        let new_l = highlight_next_expr(l)?;
        return Some(rebuild(Box::new(new_l), Box::new(r.clone())));
    }
    Some(Expr::Highlighted(
        Box::new(whole.clone()),
        HighlightColor::Yellow,
    ))
}

fn highlight_next_neg(whole: &Expr, inner: &Expr) -> Option<Expr> {
    if !is_value(inner) {
        let new_inner = highlight_next_expr(inner)?;
        return Some(Expr::Neg(Box::new(new_inner)));
    }
    // The operand is ready: `whole` (the Neg itself) is the next redex. If it's
    // not actually an int, `step` will panic on it when clicked — that's expected
    // (see eval.rs) rather than something this preview needs to pre-empt.
    Some(Expr::Highlighted(
        Box::new(whole.clone()),
        HighlightColor::Yellow,
    ))
}

fn highlight_next_binop(
    whole: &Expr,
    l: &Expr,
    r: &Expr,
    rebuild: fn(Box<Expr>, Box<Expr>) -> Expr,
) -> Option<Expr> {
    if !is_value(l) {
        let new_l = highlight_next_expr(l)?;
        return Some(rebuild(Box::new(new_l), Box::new(r.clone())));
    }
    if !is_value(r) {
        let new_r = highlight_next_expr(r)?;
        return Some(rebuild(Box::new(l.clone()), Box::new(new_r)));
    }
    // Both operands are values: `whole` itself is the next redex.
    Some(Expr::Highlighted(
        Box::new(whole.clone()),
        HighlightColor::Yellow,
    ))
}

fn highlight_next_if(
    whole: &Expr,
    cond: &Expr,
    then_branch: &Expr,
    else_branch: &Expr,
) -> Option<Expr> {
    if !is_value(cond) {
        let new_cond = highlight_next_expr(cond)?;
        return Some(Expr::If(
            Box::new(new_cond),
            Box::new(then_branch.clone()),
            Box::new(else_branch.clone()),
        ));
    }
    // The condition is ready: `whole` (the If itself) is the next redex.
    Some(Expr::Highlighted(
        Box::new(whole.clone()),
        HighlightColor::Yellow,
    ))
}

/// Mirrors `step_match`'s search: only the scrutinee is ever searched into (which
/// arm ends up chosen depends on the scrutinee's *value*, not on anything
/// highlighting can see ahead of time); once it's a value, `whole` (the entire
/// `Match`) is the next redex.
fn highlight_next_match(whole: &Expr, scrutinee: &Expr, arms: &[(Pattern, Expr)]) -> Option<Expr> {
    if !is_value(scrutinee) {
        let new_scrutinee = highlight_next_expr(scrutinee)?;
        return Some(Expr::Match(Box::new(new_scrutinee), arms.to_vec()));
    }
    Some(Expr::Highlighted(
        Box::new(whole.clone()),
        HighlightColor::Yellow,
    ))
}

/// Mirrors `highlight_next`'s own top-level decl-processing shape, generalized
/// with a trailing body: once `decls` runs out, the whole `Let` (about to
/// collapse into `body`) is the next redex.
fn highlight_next_let(whole: &Expr, decls: &[Decl], body: &Expr) -> Option<Expr> {
    let Some(first) = decls.first() else {
        return Some(Expr::Highlighted(
            Box::new(whole.clone()),
            HighlightColor::Yellow,
        ));
    };

    if let Some(new_expr) = highlight_next_expr(&first.get_val_decl().expr) {
        let mut new_decls = decls.to_vec();
        new_decls[0] = first.new_expr(new_expr);
        return Some(Expr::Let(new_decls, Box::new(body.clone())));
    }

    let mut new_decls = decls.to_vec();
    new_decls[0] = first.new_expr(Expr::Highlighted(Box::new(first.get_val_decl().expr.clone()), HighlightColor::Yellow));
    Some(Expr::Let(new_decls, Box::new(body.clone())))
}
