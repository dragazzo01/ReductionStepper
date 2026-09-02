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

/// What a binary operator produces when both its operands have type `operand`,
/// or why it doesn't accept that type at all.
///
/// Every `BinOp` takes two operands of *one* type — which one is the whole
/// question, since most of them are overloaded (see `ast::BinOp`). SML resolves
/// overloading by unification and a default; with no unification engine here,
/// the left operand's inferred type simply decides, and the right is then checked
/// against it (see the `BinOp` arm of `infer_expr_type`). That's the same answer
/// for every program a full SML compiler wouldn't need the default rule for.
fn binop_result_type(op: BinOp, operand: &Type) -> Result<Type, String> {
    // `=` and `<>` are the polymorphic pair, admitting a whole class of types
    // rather than a listed few, so they get their own answer and their own way of
    // saying no.
    if let BinOp::Eq | BinOp::Ne = op {
        return match is_equality_type(operand) {
            true => Ok(Type::Bool),
            false => Err(format!(
                "`{}` needs an equality type, but {operand:?} is not one",
                op.symbol()
            )),
        };
    }
    let result = match (op, operand) {
        // Arithmetic is overloaded over the two numeric types and gives back
        // whichever it was handed; `div`/`mod` and `/` are each specific to one.
        (BinOp::Add | BinOp::Sub | BinOp::Mul, Type::Int | Type::Real) => Some(operand.clone()),
        (BinOp::Div | BinOp::Mod, Type::Int) => Some(Type::Int),
        (BinOp::RealDiv, Type::Real) => Some(Type::Real),
        (BinOp::Concat, Type::String) => Some(Type::String),
        // The ordered types are the numeric ones plus `string`; `bool` and `unit`
        // are comparable with `=` but have no order in SML.
        (
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge,
            Type::Int | Type::Real | Type::String,
        ) => Some(Type::Bool),
        _ => None,
    };
    result.ok_or_else(|| format!("`{}` cannot be applied to {operand:?}", op.symbol()))
}

/// Whether values of `ty` can be compared with `=` and `<>`.
///
/// SML calls these the *equality types*, and admits them structurally: most base
/// types are comparable, a tuple is comparable when every component is, and a
/// function never is — there's no way to decide whether two of them agree on
/// every argument. `real` is excluded for a reason of its own: `nan` isn't equal
/// to itself, so `=` on reals wouldn't be an equality at all.
fn is_equality_type(ty: &Type) -> bool {
    match ty {
        Type::Int | Type::String | Type::Bool | Type::Unit => true,
        Type::Real => false,
        Type::Product(parts) => parts.iter().all(|part| is_equality_type(part)),
        Type::Arrow(..) => false,
    }
}

fn infer_expr_type(env: &TypeEnv, expr: &Expr) -> Result<Type, String> {
    match &expr.kind {
        ExprKind::IntConst(_) => Ok(Type::Int),
        ExprKind::RealConst(_) => Ok(Type::Real),
        ExprKind::StringConst(_) => Ok(Type::String),
        ExprKind::BoolConst(_) => Ok(Type::Bool),
        ExprKind::Unit => Ok(Type::Unit),
        // A use `resolve` found no binder for keeps the id it was minted with,
        // which nothing else shares — so it misses here, and this is where an
        // unbound identifier is reported.
        ExprKind::Var(binder) => env
            .get(&binder.id)
            .cloned()
            .ok_or_else(|| format!("Unbound identifier: {}", binder.name)),

        // Both operands of a `BinOp` have the same type, whichever type that is,
        // so the left one's is inferred and settles the overloading; the operator
        // then gets to reject it, and the right one is checked against it.
        ExprKind::BinOp(op, e1, e2) => {
            let operand_type = infer_expr_type(env, e1)?;
            let result_type = binop_result_type(*op, &operand_type)?;
            check_expr_type(env, e2, &operand_type)?;
            Ok(result_type)
        }
        // `~` is overloaded over the numeric types, and like a `BinOp` it hands
        // back whichever one it was given.
        ExprKind::Neg(e1) => {
            let operand_type = infer_expr_type(env, e1)?;
            match operand_type {
                Type::Int | Type::Real => Ok(operand_type),
                _ => Err(format!("`~` cannot be applied to {operand_type:?}")),
            }
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
        PatternBase::StringConst(_) => {
            if same_type(expected_type, &Type::String) {
                Ok(())
            } else {
                Err(format!("Expected {expected_type:?} for string pattern"))
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
        | PatternBase::StringConst(_)
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
