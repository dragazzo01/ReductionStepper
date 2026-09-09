//! `fun` declarations, turned into the `val`/`val rec` they stand for.
//!
//! `fun` is a *derived form* in the Definition of SML — it isn't a construct of
//! the core language, it's shorthand for a `val rec` bound to a `fn`. It's treated
//! as exactly that here: the grammar builds a `FunClause` per clause, this module
//! rewrites them into an ordinary `Decl`, and nothing downstream of the parser ever
//! learns that `fun` exists. There's no `Decl::FunDecl`, so the typechecker, the
//! pretty-printer and the stepper need no `fun` cases at all — the flip side being
//! that a `fun`-declared function *displays* in its elaborated form, which is
//! arguably the point in a stepper: you get to watch `fun` become `val rec`.
//!
//! The one place this can fail is a malformed `fun` binding (clauses that disagree
//! on the function's name, its arity or its result type, or a recursive function
//! with nothing to build its type annotation from). `frontend::lower` calls
//! `elaborate` while walking the parsed tree and attaches the source range of the
//! `fun` to whatever it reports.

use std::collections::HashSet;

use super::ast::{Binder, Decl, Expr, ExprKind, Pattern, PatternBase, Type, ValDecl};

/// One clause of a `fun` declaration: `name p1 p2 ... : result_ty = body`, with
/// the result type present only if the clause was written with one.
#[derive(Debug, Clone, PartialEq)]
pub struct FunClause {
    pub name: String,
    pub params: Vec<Pattern>,
    pub result_ty: Option<Type>,
    pub body: Expr,
}

/// Elaborates one `fun` declaration's clauses into the `val`/`val rec` they stand
/// for, or says why they don't stand for one.
pub fn elaborate(clauses: Vec<FunClause>) -> Result<Decl, String> {
    let first = clauses
        .first()
        .expect("grammar: FunClauses always has at least one clause");
    let name = first.name.clone();
    let arity = first.params.len();

    for clause in &clauses[1..] {
        if clause.name != name {
            return Err(format!(
                "Every clause of a `fun` declaration must define the same function, but `{name}` has a clause for `{}`",
                clause.name
            ));
        }
        if clause.params.len() != arity {
            return Err(format!(
                "Every clause of `{name}` must take the same number of arguments, but one takes {arity} and another takes {}",
                clause.params.len()
            ));
        }
    }

    // The declared types come from the first clause's parameters — the same clause
    // a `fn` takes its parameter type from (see `typecheck::infer_expr_type`'s
    // `Lambda` case) — plus whichever clauses annotate the result. A literal
    // parameter can carry an annotation like any other pattern (`fun fib (0 : int)
    // = 0 | ...`), so being the first clause is never a reason not to have one.
    let param_tys: Vec<Option<Type>> = first
        .params
        .iter()
        .map(|param| param.declared_type())
        .collect();
    let result_ty = result_type(&clauses)?;
    let fn_ty = function_type(&param_tys, result_ty);
    let lambda = lambda_of(&clauses, &param_tys, &name)?;

    // In real SML `fun` is *always* recursive; here a function that never mentions
    // its own name is elaborated to a plain `val` instead, purely so it doesn't
    // inherit `val rec`'s requirement of a full type annotation. The check is by
    // name only, so a body that merely shadows `name` counts as recursive too —
    // over-reporting just means asking for an annotation that `val rec` can use,
    // never a wrong binding.
    let recursive = clauses.iter().any(|clause| mentions(&clause.body, &name));
    let pat = |typ| Pattern::new(PatternBase::Var(Binder::new(name.clone())), typ);
    if recursive {
        let Some(typ) = fn_ty else {
            return Err(format!(
                "`{name}` is recursive, so it becomes a `val rec`, which needs a complete type: annotate every argument and the result, as in `fun {name} (x : int) : int = ...`"
            ));
        };
        Ok(Decl::ValRecDecl(ValDecl {
            pat: pat(Some(typ)),
            expr: lambda,
        }))
    } else {
        Ok(Decl::ValDecl(ValDecl {
            pat: pat(fn_ty),
            expr: lambda,
        }))
    }
}

/// The result type the clauses agree on, if any of them annotate one. Two clauses
/// annotating *different* result types is a contradiction the typechecker would
/// only report as a confusing mismatch later, so it's rejected here.
fn result_type(clauses: &[FunClause]) -> Result<Option<Type>, String> {
    let mut result: Option<Type> = None;
    for clause in clauses {
        let Some(ty) = &clause.result_ty else { continue };
        match &result {
            Some(previous) if previous != ty => {
                return Err(format!(
                    "Clauses of `{}` disagree about its result type: {previous:?} and {ty:?}",
                    clause.name
                ));
            }
            _ => result = Some(ty.clone()),
        }
    }
    Ok(result)
}

/// `t1 -> t2 -> ... -> result`, or `None` if any part of it went unannotated —
/// which is only fatal for a recursive function (see `elaborate`).
fn function_type(param_tys: &[Option<Type>], result_ty: Option<Type>) -> Option<Type> {
    let mut ty = result_ty?;
    for param_ty in param_tys.iter().rev() {
        ty = Type::Arrow(Box::new(param_ty.clone()?), Box::new(ty));
    }
    Some(ty)
}

/// The `fn` a `fun`'s clauses stand for.
///
/// The Definition's derived form is uniform — n fresh variables, then a `case` on
/// the tuple of them — but two special cases of it read far better in a stepper and
/// mean exactly the same thing, so they're spelled out directly:
///
/// - one argument, any number of clauses: the clauses *are* the `fn`'s cases, since
///   a `fn` already dispatches on a list of patterns (`fn 0 => 1 | n => n * 2`);
/// - one clause, any number of arguments: plain nested `fn`s, one per parameter,
///   with the clause's own patterns as their parameters (`fn a => fn b => body`).
///
/// Only several clauses *and* several arguments needs the general form, because
/// then there's no single pattern position to dispatch on — clause selection
/// depends on all the arguments jointly, so they have to be gathered into a tuple
/// first, which is what the fresh variables are for.
fn lambda_of(clauses: &[FunClause], param_tys: &[Option<Type>], name: &str) -> Result<Expr, String> {
    if param_tys.len() == 1 {
        return Ok(Expr::new(ExprKind::Lambda(
            clauses
                .iter()
                .map(|clause| (clause.params[0].clone(), clause.body.clone()))
                .collect(),
        )));
    }
    if let [only] = clauses {
        return Ok(curry(&only.params, only.body.clone()));
    }

    let mut taken = HashSet::new();
    for clause in clauses {
        clause
            .params
            .iter()
            .for_each(|p| collect_pattern_names(p, &mut taken));
        collect_names(&clause.body, &mut taken);
    }
    let args = param_tys
        .iter()
        .enumerate()
        .map(|(i, param_ty)| {
            let Some(typ) = param_ty.clone() else {
                return Err(format!(
                    "`{name}` has several clauses and several arguments, so it needs each argument of its first clause annotated: `fun {name} (x : int) (y : int) = ...`"
                ));
            };
            Ok(Pattern::new(
                PatternBase::Var(Binder::new(fresh_name(&mut taken, i))),
                Some(typ),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;

    // A fresh `Binder` per use, same as the grammar mints for any other
    // identifier; `frontend::resolve` points these at the parameters above.
    let scrutinee = Expr::new(ExprKind::Tuple(
        args.iter()
            .map(|arg| match &arg.pat {
                PatternBase::Var(binder) => Expr::var(Binder::new(binder.name.clone())),
                _ => unreachable!("fresh arguments are always identifiers"),
            })
            .collect(),
    ));
    let arms = clauses
        .iter()
        .map(|clause| {
            (
                Pattern::untyped(PatternBase::Tuple(clause.params.clone())),
                clause.body.clone(),
            )
        })
        .collect();
    Ok(curry(
        &args,
        Expr::new(ExprKind::Match(Box::new(scrutinee), arms)),
    ))
}

/// `fn p1 => fn p2 => ... => body`: one single-case `fn` per parameter, which is
/// what makes a multi-argument function partially applicable, exactly as in SML.
fn curry(params: &[Pattern], body: Expr) -> Expr {
    params.iter().rev().fold(body, |body, param| {
        Expr::new(ExprKind::Lambda(vec![(param.clone(), body)]))
    })
}

/// A name for the `i`th argument of the general derived form — `argA`, `argB`, ...
/// — lengthened until it collides with nothing `taken` from the clauses. These
/// names are visible in the stepped program and have to be re-enterable as source,
/// so they're letters only.
///
/// Note this runs inside a grammar action, i.e. before `resolve` exists to give
/// anything a binder id, so avoiding a collision here really is a matter of
/// picking an unused *spelling* — unlike the stepper, which used to have to do
/// the same thing and no longer does.
fn fresh_name(taken: &mut HashSet<String>, i: usize) -> String {
    let mut candidate = format!("arg{}", (b'A' + (i % 26) as u8) as char);
    while taken.contains(&candidate) {
        candidate.push('X');
    }
    taken.insert(candidate.clone());
    candidate
}

fn mentions(expr: &Expr, name: &str) -> bool {
    let mut names = HashSet::new();
    collect_names(expr, &mut names);
    names.contains(name)
}

/// Every identifier `expr` mentions, whether used or bound — unlike
/// `stepping::eval`'s deliberately binder-only collector, since the two callers
/// here want opposite things from a *use*: `mentions` is looking for exactly those,
/// and `fresh_name` must avoid capturing one.
fn collect_names(expr: &Expr, out: &mut HashSet<String>) {
    match &expr.kind {
        ExprKind::IntConst(_)
        | ExprKind::RealConst(_)
        | ExprKind::StringConst(_)
        | ExprKind::BoolConst(_)
        | ExprKind::Unit => {}
        ExprKind::Var(binder) => {
            out.insert(binder.name.clone());
        }
        ExprKind::Neg(inner) => collect_names(inner, out),
        ExprKind::BinOp(_, l, r)
        | ExprKind::AndAlso(l, r)
        | ExprKind::OrElse(l, r)
        | ExprKind::App(l, r) => {
            collect_names(l, out);
            collect_names(r, out);
        }
        ExprKind::If(cond, then_branch, else_branch) => {
            collect_names(cond, out);
            collect_names(then_branch, out);
            collect_names(else_branch, out);
        }
        ExprKind::Let(decls, body) => {
            for decl in decls.iter().filter_map(Decl::as_val_decl) {
                collect_pattern_names(&decl.pat, out);
                collect_names(&decl.expr, out);
            }
            collect_names(body, out);
        }
        ExprKind::Tuple(items) => items.iter().for_each(|item| collect_names(item, out)),
        ExprKind::Match(scrutinee, arms) => {
            collect_names(scrutinee, out);
            collect_cases(arms, out);
        }
        ExprKind::Lambda(cases) => collect_cases(cases, out),
    }
}

fn collect_cases(cases: &[(Pattern, Expr)], out: &mut HashSet<String>) {
    for (pat, expr) in cases {
        collect_pattern_names(pat, out);
        collect_names(expr, out);
    }
}

fn collect_pattern_names(pat: &Pattern, out: &mut HashSet<String>) {
    out.extend(pat.binders().into_iter().map(|b| b.name.clone()));
}
