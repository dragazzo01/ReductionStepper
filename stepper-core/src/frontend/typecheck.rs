use std::collections::HashMap;
use std::collections::HashSet;

use crate::ast::{
    BinOp, BinderId, Decl, Expr, ExprKind, Pattern, PatternBase, Program, Type, ValDecl,
};
use crate::pretty::{pretty_print_expr, pretty_print_pattern};

/// Maps each variable — identified by its binding site, not its name — to its
/// type.
///
/// Keying by `BinderId` means shadowing needs no handling at all: an inner
/// `val x` is a different key from an outer one, so neither can overwrite or hide
/// the other, and `frontend::resolve` has already decided which one any given use
/// refers to. The scope-shaped `env.clone()`s below are kept because they say
/// plainly what scopes what, but with unique ids they no longer carry weight.
type TypeEnv = HashMap<BinderId, Type>;

// this will get more complicated as I have type aliases
fn same_type(typ1: &Type, typ2: &Type) -> bool {
    typ1 == typ2
}

/// What a binary operator produces. Its *operands* are two ints for all of these
/// but `=` and `<>`, which take any one equality type — see `is_equality_type`
/// and the `BinOp` arms of `infer_expr_type`.
fn binop_result_type(op: BinOp) -> Type {
    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => Type::Int,
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => Type::Bool,
    }
}

/// Whether values of `ty` can be compared with `=` and `<>`.
///
/// SML calls these the *equality types*, and admits them structurally: the base
/// types are comparable, a tuple is comparable when every component is, and a
/// function never is — there's no way to decide whether two of them agree on
/// every argument. (Real SML also excludes `real`, for a different reason: `nan`
/// isn't equal to itself. This subset has no reals to exclude.)
fn is_equality_type(ty: &Type) -> bool {
    match ty {
        Type::Int | Type::Bool | Type::Unit => true,
        Type::Product(parts) => parts.iter().all(|part| is_equality_type(part)),
        Type::Arrow(..) => false,
    }
}

fn infer_expr_type(env: &TypeEnv, expr: &Expr) -> Result<Type, String> {
    match &expr.kind {
        ExprKind::IntConst(_) => Ok(Type::Int),
        ExprKind::BoolConst(_) => Ok(Type::Bool),
        ExprKind::Unit => Ok(Type::Unit),
        // A use `resolve` found no binder for keeps the id it was minted with,
        // which nothing else shares — so it misses here, and this is where an
        // unbound identifier is reported.
        ExprKind::Var(binder) => env
            .get(&binder.id)
            .cloned()
            .ok_or_else(|| format!("Unbound identifier: {}", binder.name)),

        // `=` and `<>` are the polymorphic pair: their operands may be of any one
        // equality type, so the left one's type is inferred and the right is
        // checked against it. Every other operator is arithmetic on two ints.
        ExprKind::BinOp(op @ (BinOp::Eq | BinOp::Ne), e1, e2) => {
            let operand_type = infer_expr_type(env, e1)?;
            check_expr_type(env, e2, &operand_type)?;
            if !is_equality_type(&operand_type) {
                return Err(format!(
                    "`{}` needs an equality type, but {operand_type:?} is not one",
                    op.symbol()
                ));
            }
            Ok(binop_result_type(*op))
        }
        ExprKind::BinOp(op, e1, e2) => {
            check_expr_type(env, e1, &Type::Int)?;
            check_expr_type(env, e2, &Type::Int)?;
            Ok(binop_result_type(*op))
        }
        ExprKind::Neg(e1) => {
            check_expr_type(env, e1, &Type::Int)?;
            Ok(Type::Int)
        }
        ExprKind::AndAlso(e1, e2) | ExprKind::OrElse(e1, e2) => {
            check_expr_type(env, e1, &Type::Bool)?;
            check_expr_type(env, e2, &Type::Bool)?;
            Ok(Type::Bool)
        }
        ExprKind::If(cond, then_branch, else_branch) => {
            check_expr_type(env, cond, &Type::Bool)?;
            let then_ty = infer_expr_type(env, then_branch)?;
            check_expr_type(env, else_branch, &then_ty)?;
            Ok(then_ty)
        }
        ExprKind::Let(decls, body) => {
            // Bindings only live for the body, so typecheck into a scope local to this
            // call rather than mutating `env` — mirrors how `step_let` substitutes into
            // `body` without touching anything outside the `let`.
            let mut inner_env = env.clone();
            typecheck_decls(&mut inner_env, decls)?;
            infer_expr_type(&inner_env, body)
        }
        ExprKind::Tuple(exprs) => {
            let types = exprs
                .iter()
                .map(|e| infer_expr_type(env, e))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Type::Product(types.into_iter().map(Box::new).collect()))
        }
        ExprKind::Match(scrutinee, arms) => {
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
        ExprKind::Lambda(cases) => {
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
        ExprKind::App(f, arg) => {
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
        PatternBase::Unit => {
            if same_type(expected_type, &Type::Unit) {
                Ok(())
            } else {
                Err(format!("Expected {expected_type:?} for unit pattern"))
            }
        }
        PatternBase::Var(_) | PatternBase::Wildcard => Ok(()),
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

/// Matches `pat` against `ty`, inserting a binding into `env` for every variable
/// `pat` contains. `Wildcard` matches anything and binds nothing; `Tuple` requires
/// a `Product` of the same arity and recurses componentwise — a shape or arity
/// mismatch is a type error, same as any other `Expected .. but got ..` case.
///
/// `seen` tracks *names* already bound by this pattern, so a non-linear pattern
/// like `val (x, x) = ...` is rejected — SML disallows it too, since it'd
/// otherwise be ambiguous which binding a later use of `x` refers to. Names, not
/// ids, because the two `x`s here are two separate binding sites with two
/// distinct ids: being distinct is exactly what makes them a problem.
fn bind_pattern(
    env: &mut TypeEnv,
    seen: &mut HashSet<String>,
    pat: &Pattern,
    ty: &Type,
) -> Result<(), String> {
    check_pattern_type(pat, ty)?;
    match &pat.pat {
        PatternBase::Wildcard
        | PatternBase::IntConst(_)
        | PatternBase::BoolConst(_)
        | PatternBase::Unit => Ok(()),
        PatternBase::Var(binder) => {
            if !seen.insert(binder.name.clone()) {
                return Err(format!(
                    "Variable {} is bound more than once in this pattern",
                    binder.name
                ));
            }
            env.insert(binder.id, ty.clone());
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
    let PatternBase::Var(_) = &decl.pat.pat else {
        return Err(String::from("must use single identifier for val rec"));
    };
    let Type::Arrow(_, _) = typ else {
        return Err(String::from("val rec requires a function type"));
    };
    let ExprKind::Lambda(_) = decl.expr.kind else {
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
