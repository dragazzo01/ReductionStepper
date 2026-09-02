//! Scope resolution: points every variable *use* at the binding site it refers to.
//!
//! The parser mints a fresh [`BinderId`] at every occurrence of an identifier,
//! binding or not, because a grammar action has no idea what's in scope. This
//! pass fixes that up: it walks the program with a scope stack and rewrites each
//! use's id to its binder's, leaving binding sites with the id they were minted
//! with. Afterwards `BinderId` means exactly "which variable", which is what
//! `subst.rs` and `typecheck.rs` rely on.
//!
//! Purely syntactic, and infallible. A use with no binder in scope keeps the id
//! it was minted with — an id nothing else shares — so it simply fails to match
//! anything, and `typecheck` reports it as the unbound identifier it is. Doing it
//! that way keeps one error message in one place instead of splitting "unbound"
//! between two passes.
//!
//! The scoping implemented here is the same scoping `typecheck` walks and
//! `stepping` substitutes under; the three have to agree, and this is the pass
//! that decides it:
//!
//! - decls in a list (a program, or a `let`) take effect in order, so a decl's
//!   right-hand side sees the ones before it but not itself — except `val rec`,
//!   whose whole point is that its right-hand side *does* see its own binding;
//! - a later decl rebinding a name shadows the earlier one from that point on,
//!   which here is just a later `insert` overwriting an earlier one;
//! - each `fn` case and each `case` arm is its own scope: its pattern binds for
//!   that arm's body alone, and never for its siblings or the scrutinee.

use std::collections::HashMap;

use super::ast::{Decl, Expr, ExprKind, Pattern, Program, ValDecl};
use crate::ast::BinderId;

/// Innermost-last stack of scopes. A lookup walks it backwards; a binding always
/// lands in the top frame.
struct Scopes {
    frames: Vec<HashMap<String, BinderId>>,
}

impl Scopes {
    fn new() -> Self {
        Scopes {
            frames: vec![HashMap::new()],
        }
    }

    fn push(&mut self) {
        self.frames.push(HashMap::new());
    }

    fn pop(&mut self) {
        self.frames.pop();
    }

    fn lookup(&self, name: &str) -> Option<BinderId> {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| frame.get(name).copied())
    }

    /// Registers every variable `pat` binds, so uses after this point resolve to
    /// them. The ids come from the pattern itself — binding sites keep what the
    /// parser gave them.
    fn bind(&mut self, pat: &Pattern) {
        let frame = self
            .frames
            .last_mut()
            .expect("scope stack is never empty: `new` seeds one frame");
        for binder in pat.binders() {
            frame.insert(binder.name.clone(), binder.id);
        }
    }
}

/// Repoints every use in `program` at its binding site, in place.
pub fn resolve(program: &mut Program) {
    let mut scopes = Scopes::new();
    resolve_decls(&mut scopes, program);
}

/// Resolves a decl list into the *current* frame, so the bindings outlive the
/// list — which is what a trailing `let` body or the rest of the program needs.
/// Callers that want the bindings confined push a frame first.
fn resolve_decls(scopes: &mut Scopes, decls: &mut [Decl]) {
    for decl in decls {
        match decl {
            // A plain `val`'s right-hand side is evaluated in the scope *before*
            // its own binding takes effect, so `val x = x + 1` refers to an outer
            // `x` (or to nothing at all).
            Decl::ValDecl(ValDecl { pat, expr }) => {
                resolve_expr(scopes, expr);
                scopes.bind(pat);
            }
            // `val rec` is the opposite, and that's the whole of what `rec` means
            // here: bind first, so the function's own name resolves to itself
            // inside its body.
            Decl::ValRecDecl(ValDecl { pat, expr }) => {
                scopes.bind(pat);
                resolve_expr(scopes, expr);
            }
        }
    }
}

/// Resolves each `(pattern, body)` case as its own scope — the shape `fn` cases
/// and `case` arms share.
fn resolve_cases(scopes: &mut Scopes, cases: &mut [(Pattern, Expr)]) {
    for (pat, body) in cases {
        scopes.push();
        scopes.bind(pat);
        resolve_expr(scopes, body);
        scopes.pop();
    }
}

fn resolve_expr(scopes: &mut Scopes, expr: &mut Expr) {
    match &mut expr.kind {
        ExprKind::IntConst(_) | ExprKind::BoolConst(_) | ExprKind::Unit => {}
        ExprKind::Var(binder) => {
            if let Some(id) = scopes.lookup(&binder.name) {
                binder.id = id;
            }
        }
        ExprKind::Neg(inner) => resolve_expr(scopes, inner),
        ExprKind::BinOp(_, l, r)
        | ExprKind::AndAlso(l, r)
        | ExprKind::OrElse(l, r)
        | ExprKind::App(l, r) => {
            resolve_expr(scopes, l);
            resolve_expr(scopes, r);
        }
        ExprKind::If(cond, then_branch, else_branch) => {
            resolve_expr(scopes, cond);
            resolve_expr(scopes, then_branch);
            resolve_expr(scopes, else_branch);
        }
        ExprKind::Tuple(items) => items.iter_mut().for_each(|i| resolve_expr(scopes, i)),
        ExprKind::Let(decls, body) => {
            // The decls bind for the body and for each other, and for nothing
            // outside — hence a frame of its own.
            scopes.push();
            resolve_decls(scopes, decls);
            resolve_expr(scopes, body);
            scopes.pop();
        }
        // The scrutinee is outside every arm's scope, so it resolves before the
        // arms push theirs.
        ExprKind::Match(scrutinee, arms) => {
            resolve_expr(scopes, scrutinee);
            resolve_cases(scopes, arms);
        }
        ExprKind::Lambda(cases) => resolve_cases(scopes, cases),
    }
}
