use std::collections::HashMap;
use std::collections::HashSet;

use crate::ast::{Program, Decl, Expr, Pattern, Type};
use crate::pretty::pretty_print_pattern;

/// Maps declared names to their types. `Let` typechecks into a clone of the outer
/// env (see below), so a later `val x = ...` shadowing an earlier one just means a
/// later `insert` overwriting an earlier one — same result as `subst.rs`'s
/// shadowing, without needing to track insertion order.
type TypeEnv = HashMap<String, Type>;

// this will get more complicated as I have type aliases
fn same_type(typ1 : &Type, typ2 : &Type) -> bool {
    typ1 == typ2
}

fn lookup_type(env: &TypeEnv, name: &str) -> Option<Type> {
    env.get(name).cloned()
}

fn infer_expr_type(env: &TypeEnv, expr: &Expr) -> Result<Type, String> {
    match expr {
    Expr::IntConst(_) => Ok(Type::Int),
    Expr::BoolConst(_) => Ok(Type::Bool),
    Expr::Ident(name) => lookup_type(env, name)
        .ok_or_else(|| format!("Unbound identifier: {name}")),

    Expr::Add(e1, e2) |
    Expr::Sub(e1, e2) |
    Expr::Mul(e1, e2) |
    Expr::Div(e1, e2) |
    Expr::Mod(e1, e2) => {
        check_expr_type(env, e1, &Type::Int)?;
        check_expr_type(env, e2, &Type::Int)?;
        Ok(Type::Int)
    },
    Expr::Neg(e1) => {
        check_expr_type(env, e1, &Type::Int)?;
        Ok(Type::Int)
    },
    Expr::Eq(e1, e2) |
    Expr::Ne(e1, e2) |
    Expr::Lt(e1, e2) |
    Expr::Le(e1, e2) |
    Expr::Gt(e1, e2) |
    Expr::Ge(e1, e2) => {
        check_expr_type(env, e1, &Type::Int)?;
        check_expr_type(env, e2, &Type::Int)?;
        Ok(Type::Bool)
    },
    Expr::AndAlso(e1, e2) |
    Expr::OrElse(e1, e2) => {
        check_expr_type(env, e1, &Type::Bool)?;
        check_expr_type(env, e2, &Type::Bool)?;
        Ok(Type::Bool)
    },
    Expr::If(cond, then_branch, else_branch) => {
        check_expr_type(env, cond, &Type::Bool)?;
        let then_ty = infer_expr_type(env, then_branch)?;
        check_expr_type(env, else_branch, &then_ty)?;
        Ok(then_ty)
    },
    Expr::Let(decls, body) => {
        // Bindings only live for the body, so typecheck into a scope local to this
        // call rather than mutating `env` — mirrors how `step_let` substitutes into
        // `body` without touching anything outside the `let`.
        let mut inner_env = env.clone();
        typecheck_decls(&mut inner_env, decls)?;
        infer_expr_type(&inner_env, body)
    },
    Expr::Tuple(exprs) => {
        let types = exprs.iter()
            .map(|e| infer_expr_type(env, e))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Type::Product(types.into_iter().map(Box::new).collect()))
    },
    Expr::Highlighted(expr, _) => infer_expr_type(env, expr),
    }
}

fn check_expr_type(env: &TypeEnv, expr: &Expr, expected_type: &Type) -> Result<(), String> {
    let actual_type = infer_expr_type(env, expr)?;
    if same_type(&actual_type, expected_type) {
        Ok(())
    } else {
        Err(format!("Expected {expected_type:?} but got type {actual_type:?}"))
    }
}

/// A literal pattern (`IntConst`/`BoolConst`) requires `ty` to be exactly the
/// literal's own type — same "Expected .. but got .." phrasing as `check_expr_type`,
/// since this is the same kind of mismatch, just checked against a pattern instead
/// of an expression.
fn check_pattern_type(ty: &Type, expected: &Type) -> Result<(), String> {
    if same_type(ty, expected) {
        Ok(())
    } else {
        Err(format!("Expected {expected:?} but got type {ty:?}"))
    }
}

/// Matches `pat` against `ty`, inserting a binding into `env` for every identifier
/// `pat` contains. `Wildcard` matches anything and binds nothing; `Tuple` requires
/// a `Product` of the same arity and recurses componentwise — a shape or arity
/// mismatch is a type error, same as any other `Expected .. but got ..` case.
/// `seen` tracks names already bound by *this* pattern (not `env`, which may
/// legitimately already hold a same-named binding from an earlier decl being
/// shadowed) so a non-linear pattern like `val (x, x) = ...` is rejected — SML
/// disallows this too, since it'd otherwise be ambiguous which binding a later use
/// of `x` refers to.
fn bind_pattern(env: &mut TypeEnv, seen: &mut HashSet<String>, pat: &Pattern, ty: &Type) -> Result<(), String> {
    match pat {
        Pattern::Wildcard => Ok(()),
        Pattern::Ident(name) => {
            if !seen.insert(name.clone()) {
                return Err(format!("Variable {name} is bound more than once in this pattern"));
            }
            env.insert(name.clone(), ty.clone());
            Ok(())
        }
        Pattern::IntConst(_) => check_pattern_type(ty, &Type::Int),
        Pattern::BoolConst(_) => check_pattern_type(ty, &Type::Bool),
        Pattern::Tuple(pats) => {
            let Type::Product(types) = ty else {
                return Err(format!(
                    "Pattern {} expects a tuple type but got {ty:?}",
                    pretty_print_pattern(pat)
                ));
            };
            if pats.len() != types.len() {
                return Err(format!(
                    "Pattern {} has {} components but its type has {}",
                    pretty_print_pattern(pat),
                    pats.len(),
                    types.len()
                ));
            }
            for (p, t) in pats.iter().zip(types.iter()) {
                bind_pattern(env, seen, p, t)?;
            }
            Ok(())
        }
    }
}

/// Typechecks `decl` against the bindings seen so far and, on success, binds its
/// pattern into `env` so later decls in the same list (or the enclosing `let`'s
/// body) can see it.
fn typecheck_decl(env: &mut TypeEnv, decl: &Decl) -> Result<(), String> {
    let ty = match &decl.ty {
        Some(declared) => {
            check_expr_type(env, &decl.expr, declared)?;
            declared.clone()
        }
        None => infer_expr_type(env, &decl.expr)?,
    };
    bind_pattern(env, &mut HashSet::new(), &decl.pat, &ty)
}

fn typecheck_decls(env: &mut TypeEnv, decls: &[Decl]) -> Result<(), String> {
    for decl in decls {
        typecheck_decl(env, decl)?;
    }
    Ok(())
}

pub fn typecheck(prog: &Program) -> Option<String> {
    let mut env = TypeEnv::new();
    typecheck_decls(&mut env, prog).err()
}
