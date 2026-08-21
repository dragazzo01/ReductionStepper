use std::collections::HashMap;
use std::collections::HashSet;

use crate::ast::{Decl, ValDecl, Expr, Pattern, PatternBase, Program, Type};
use crate::pretty::{pretty_print_expr, pretty_print_pattern};

/// Maps declared names to their types. `Let` typechecks into a clone of the outer
/// env (see below), so a later `val x = ...` shadowing an earlier one just means a
/// later `insert` overwriting an earlier one — same result as `subst.rs`'s
/// shadowing, without needing to track insertion order.
type TypeEnv = HashMap<String, Type>;

// this will get more complicated as I have type aliases
fn same_type(typ1: &Type, typ2: &Type) -> bool {
    typ1 == typ2
}

fn lookup_type(env: &TypeEnv, name: &str) -> Option<Type> {
    env.get(name).cloned()
}

fn infer_expr_type(env: &TypeEnv, expr: &Expr) -> Result<Type, String> {
    match expr {
        Expr::IntConst(_) => Ok(Type::Int),
        Expr::BoolConst(_) => Ok(Type::Bool),
        Expr::Ident(name) => {
            lookup_type(env, name).ok_or_else(|| format!("Unbound identifier: {name}"))
        }

        Expr::Add(e1, e2)
        | Expr::Sub(e1, e2)
        | Expr::Mul(e1, e2)
        | Expr::Div(e1, e2)
        | Expr::Mod(e1, e2) => {
            check_expr_type(env, e1, &Type::Int)?;
            check_expr_type(env, e2, &Type::Int)?;
            Ok(Type::Int)
        }
        Expr::Neg(e1) => {
            check_expr_type(env, e1, &Type::Int)?;
            Ok(Type::Int)
        }
        Expr::Eq(e1, e2)
        | Expr::Ne(e1, e2)
        | Expr::Lt(e1, e2)
        | Expr::Le(e1, e2)
        | Expr::Gt(e1, e2)
        | Expr::Ge(e1, e2) => {
            check_expr_type(env, e1, &Type::Int)?;
            check_expr_type(env, e2, &Type::Int)?;
            Ok(Type::Bool)
        }
        Expr::AndAlso(e1, e2) | Expr::OrElse(e1, e2) => {
            check_expr_type(env, e1, &Type::Bool)?;
            check_expr_type(env, e2, &Type::Bool)?;
            Ok(Type::Bool)
        }
        Expr::If(cond, then_branch, else_branch) => {
            check_expr_type(env, cond, &Type::Bool)?;
            let then_ty = infer_expr_type(env, then_branch)?;
            check_expr_type(env, else_branch, &then_ty)?;
            Ok(then_ty)
        }
        Expr::Let(decls, body) => {
            // Bindings only live for the body, so typecheck into a scope local to this
            // call rather than mutating `env` — mirrors how `step_let` substitutes into
            // `body` without touching anything outside the `let`.
            let mut inner_env = env.clone();
            typecheck_decls(&mut inner_env, decls)?;
            infer_expr_type(&inner_env, body)
        }
        Expr::Tuple(exprs) => {
            let types = exprs
                .iter()
                .map(|e| infer_expr_type(env, e))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Type::Product(types.into_iter().map(Box::new).collect()))
        }
        Expr::Highlighted(expr, _) => infer_expr_type(env, expr),
        Expr::Match(scrutinee, arms) => {
            let scrutinee_ty = infer_expr_type(env, scrutinee)?;
            let (first_pat, first_expr) = arms
                .first()
                .expect("grammar: MatchArms always has at least one arm");
            let mut arm_env = env.clone();
            bind_pattern(&mut arm_env, &mut HashSet::new(), first_pat, &scrutinee_ty)?;
            let result_ty = infer_expr_type(&arm_env, first_expr)?;
            for (pat, arm_expr) in &arms[1..] {
                let mut arm_env = env.clone();
                bind_pattern(&mut arm_env, &mut HashSet::new(), pat, &scrutinee_ty)?;
                check_expr_type(&arm_env, arm_expr, &result_ty)?;
            }
            Ok(result_ty)
        }
        Expr::Lambda(cases) => {
            // The parameter type is mandatory in the grammar (no unification engine
            // to infer it from how the lambda is later applied), so binding it is
            // exactly like binding a `val` decl's pattern against its declared type.
            
            let (first_pat, first_expr) = &cases[0];
            let Some(param_ty) = &first_pat.declared_type() else {
                return Err(format!("{} must have an explict param type for functions on first pattern", 
                                    pretty_print_pattern(first_pat)
                        ));
            };
            let mut case_env = env.clone();
            bind_pattern(&mut case_env, &mut HashSet::new(), first_pat, &param_ty)?;
            let result_ty = infer_expr_type(&case_env, first_expr)?;
            for (pat, arm_expr) in &cases[1..] {
                let mut arm_env = env.clone();
                bind_pattern(&mut arm_env, &mut HashSet::new(), pat, &param_ty)?;
                check_expr_type(&arm_env, arm_expr, &result_ty)?;
            }
            Ok(Type::Arrow(
                Box::new(param_ty.clone()),
                Box::new(result_ty),
            ))
        }
        Expr::App(f, arg) => {
            let f_ty = infer_expr_type(env, f)?;
            let Type::Arrow(param_ty, result_ty) = f_ty else {
                return Err(format!(
                    "Applying a non-function: `{}` has type {f_ty:?}",
                    pretty_print_expr(f)
                ));
            };
            check_expr_type(env, arg, &param_ty)?;
            Ok(*result_ty)
        }
    }
}

fn check_expr_type(env: &TypeEnv, expr: &Expr, expected_type: &Type) -> Result<(), String> {
    let actual_type = infer_expr_type(env, expr)?;
    if same_type(&actual_type, expected_type) {
        Ok(())
    } else {
        Err(format!(
            "Expected {expected_type:?} but got type {actual_type:?}"
        ))
    }
}

fn check_pattern_type(pattern: &Pattern, expected_type: &Type) -> Result<(), String> {
    if let Some(typ) = &pattern.typ {
        if !same_type(expected_type, typ) {
            return Err(format!("Expected {expected_type:?} got {typ:?}"));
        }
    }

    match &pattern.pat {
        PatternBase::IntConst(_) => {
            if same_type(expected_type, &Type::Int) {
                Ok(())
            } else {
                Err(format!("Expected {expected_type:?} for int pattern"))
            }
        }
        PatternBase::BoolConst(_) => {
            if same_type(expected_type, &Type::Bool) {
                Ok(())
            } else {
                Err(format!("Expected {expected_type:?} for bool pattern"))
            }
        }
        PatternBase::Ident(_) | PatternBase::Wildcard => Ok(()),
        PatternBase::Tuple(pats) => {
            let Type::Product(expected_typs) = expected_type else {
                return Err(format!(
                    "Pattern {} expects a tuple type but got {expected_type:?}",
                    pretty_print_pattern(pattern)
                ));
            };
            if expected_typs.len() != pats.len() {
                return Err(format!("Tuple of pattern and expected type do not match"));
            }
            for (pat, expected_typ) in pats.iter().zip(expected_typs.iter()) {
                check_pattern_type(pat, expected_typ)?
            }
            Ok(())
        }
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
fn bind_pattern(
    env: &mut TypeEnv,
    seen: &mut HashSet<String>,
    pat: &Pattern,
    ty: &Type,
) -> Result<(), String> {
    check_pattern_type(pat, ty)?;
    match &pat.pat {
        PatternBase::Wildcard | PatternBase::IntConst(_) | PatternBase::BoolConst(_) => Ok(()),
        PatternBase::Ident(name) => {
            if !seen.insert(name.clone()) {
                return Err(format!(
                    "Variable {name} is bound more than once in this pattern"
                ));
            }
            env.insert(name.clone(), ty.clone());
            Ok(())
        }
        PatternBase::Tuple(pats) => {
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
fn typecheck_val_decl(env: &mut TypeEnv, decl: &ValDecl) -> Result<(), String> {
    let ty = match &decl.pat.typ {
        Some(declared) => {
            check_expr_type(env, &decl.expr, declared)?;
            declared.clone()
        }
        None => infer_expr_type(env, &decl.expr)?,
    };
    bind_pattern(env, &mut HashSet::new(), &decl.pat, &ty)
}

fn typecheck_val_rec_decl(env: &mut TypeEnv, decl: &ValDecl) -> Result<(), String> {
    let Some(typ) = &decl.pat.typ else {
        return Err(String::from("val rec requires an explict type annoation"));
    };
    let PatternBase::Ident(_) = &decl.pat.pat else {
        return Err(String::from("must use single identifier for val rec"));
    };
    let Type::Arrow(_, _) = typ else {
        return Err(String::from("val rec requires a function type"));
    };
    let Expr::Lambda(_) = decl.expr else {
        return Err(String::from("val rec must have a fn expression on rhs"));
    };
    bind_pattern(env, &mut HashSet::new(), &decl.pat, &typ)?;
    check_expr_type(env, &decl.expr, typ)
}

fn typecheck_decls(env: &mut TypeEnv, decls: &[Decl]) -> Result<(), String> {
    for decl in decls {
        match decl {
            Decl::ValDecl(decl) => typecheck_val_decl(env, decl)?,
            Decl::ValRecDecl(decl) => typecheck_val_rec_decl(env, decl)?,
        }
    }
    Ok(())
}

pub fn typecheck(prog: &Program) -> Option<String> {
    let mut env = TypeEnv::new();
    typecheck_decls(&mut env, prog).err()
}
