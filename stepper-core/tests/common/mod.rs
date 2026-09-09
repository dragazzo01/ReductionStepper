//! Shared helpers for the integration test suite.
//!
//! Every test file here drives the crate through its public API only
//! (`parse_program`, `typecheck`, `pretty`, `stepping`, `view`), the same surface
//! the WASM frontend uses. The helpers below exist to keep the tests themselves
//! readable: short AST constructors (`val`, `pvar`, ...) for the parser tests, and
//! marker-aware rendering (`show`, `render_next`) for the stepping ones.
//!
//! ## Why there's a thread-local here
//!
//! Highlights aren't part of the program any more — they're `ViewState` keyed by
//! `NodeId`, held beside it (see `stepper_core::view`). So "what does the program
//! look like after that step" is a question about two things, not one, and a test
//! written as `let (program, msg) = step_once(&program); assert_eq!(show(&program),
//! "...[g5g]...")` has only threaded one of them through.
//!
//! Rather than make every test carry a view around, this module keeps one per
//! test thread and has `step_once` update it — exactly the arrangement `lib.rs`
//! keeps for the browser, where a `thread_local!` session holds the program and
//! its view together. `parse` resets it, so each test starts clean, and libtest
//! gives each test its own thread.

#![allow(dead_code)]

use std::cell::RefCell;

use stepper_core::ast::{
    BinOp, Binder, Decl, Expr as AstExpr, ExprKind, NodeId, Pattern, PatternBase, Program, Type,
    ValDecl,
};
use stepper_core::view::{HighlightColor, ViewState, UNLIMITED_WIDTH};
use stepper_core::{parse_program, pretty, stepping};

/// Builders that spell the old `Expr` enum's variants.
///
/// `Expr` is a struct now — an id plus an [`ExprKind`] — so `Expr::Mul(a, b)`
/// stopped being a thing you can write. The parser tests are entirely about
/// *shapes*, though, and threading a fresh id through a hundred-odd expected
/// trees by hand would say nothing and read worse, so these put the old spelling
/// back: each one mints an id and wraps the corresponding `ExprKind`. Equality
/// ignores ids (see `ast.rs`), so the minted ones never matter.
///
/// Test files get this instead of `stepper_core::ast::Expr` — it deliberately
/// shadows that import, and nothing in the tests uses `Expr` as a type.
#[allow(non_snake_case)]
pub mod expr_builders {
    use super::{AstExpr, BinOp, Decl, ExprKind, Pattern};

    pub struct Expr;

    /// The operators that share `ExprKind::BinOp` — spelled here as if each still
    /// had a variant of its own.
    macro_rules! binop {
        ($($name:ident),* $(,)?) => {
            $(pub fn $name(l: Box<AstExpr>, r: Box<AstExpr>) -> AstExpr {
                AstExpr::new(ExprKind::BinOp(BinOp::$name, l, r))
            })*
        };
    }

    /// The two-operand kinds that kept a variant of their own.
    macro_rules! binary {
        ($($name:ident),* $(,)?) => {
            $(pub fn $name(l: Box<AstExpr>, r: Box<AstExpr>) -> AstExpr {
                AstExpr::new(ExprKind::$name(l, r))
            })*
        };
    }

    impl Expr {
        binop!(Add, Sub, Mul, RealDiv, Div, Mod, Concat, Eq, Ne, Lt, Le, Gt, Ge);
        binary!(AndAlso, OrElse, App);

        pub fn IntConst(n: i64) -> AstExpr {
            AstExpr::new(ExprKind::IntConst(n))
        }
        pub fn RealConst(x: f64) -> AstExpr {
            AstExpr::new(ExprKind::RealConst(x))
        }
        pub fn StringConst(s: &str) -> AstExpr {
            AstExpr::new(ExprKind::StringConst(s.to_string()))
        }
        pub fn BoolConst(b: bool) -> AstExpr {
            AstExpr::new(ExprKind::BoolConst(b))
        }
        pub fn Unit() -> AstExpr {
            AstExpr::new(ExprKind::Unit)
        }
        pub fn Neg(inner: Box<AstExpr>) -> AstExpr {
            AstExpr::new(ExprKind::Neg(inner))
        }
        pub fn If(c: Box<AstExpr>, t: Box<AstExpr>, e: Box<AstExpr>) -> AstExpr {
            AstExpr::new(ExprKind::If(c, t, e))
        }
        pub fn Let(decls: Vec<Decl>, body: Box<AstExpr>) -> AstExpr {
            AstExpr::new(ExprKind::Let(decls, body))
        }
        pub fn Tuple(items: Vec<AstExpr>) -> AstExpr {
            AstExpr::new(ExprKind::Tuple(items))
        }
        pub fn Match(scrutinee: Box<AstExpr>, arms: Vec<(Pattern, AstExpr)>) -> AstExpr {
            AstExpr::new(ExprKind::Match(scrutinee, arms))
        }
        pub fn Lambda(cases: Vec<(Pattern, AstExpr)>) -> AstExpr {
            AstExpr::new(ExprKind::Lambda(cases))
        }
    }
}

thread_local! {
    /// The display state that goes with whatever program the current test is
    /// holding. Only ever carries green markers: yellow is computed on demand by
    /// `render_next`, since it's a function of the program alone.
    static VIEW: RefCell<ViewState> = RefCell::new(ViewState::plain());
}

fn with_view<T>(f: impl FnOnce(&ViewState) -> T) -> T {
    VIEW.with(|v| f(&v.borrow()))
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parses `src`, panicking with the parse error if it doesn't.
///
/// Also clears any markers left from an earlier program in the same test, which
/// is what `enter_formula` does for the same reason: a new program is made of new
/// nodes, so nothing the old view named still exists.
pub fn parse(src: &str) -> Program {
    VIEW.with(|v| *v.borrow_mut() = ViewState::plain());
    parse_program(src).unwrap_or_else(|e| panic!("failed to parse `{src}`:\n{e}"))
}

/// The `i`th declaration's right-hand side. Panics on a `type` decl, which has
/// none — a test that wants one asks for `&program[i]` itself.
pub fn expr_at(program: &Program, i: usize) -> &AstExpr {
    &val_decl_at(program, i).expr
}

/// The `i`th declaration's pattern, type annotation included.
pub fn pattern_at(program: &Program, i: usize) -> &Pattern {
    &val_decl_at(program, i).pat
}

fn val_decl_at(program: &Program, i: usize) -> &ValDecl {
    program[i]
        .as_val_decl()
        .unwrap_or_else(|| panic!("decl {i} binds no value"))
}

/// The `i`th declaration's pattern with its annotation dropped.
pub fn pattern_base_at(program: &Program, i: usize) -> &PatternBase {
    &pattern_at(program, i).pat
}

/// The `i`th declaration's type annotation, if it has one.
pub fn type_at(program: &Program, i: usize) -> Option<&Type> {
    pattern_at(program, i).typ.as_ref()
}

/// Parses a single-`val` program and returns just its right-hand-side expression.
pub fn expr_of(src: &str) -> AstExpr {
    let program = parse(src);
    assert_eq!(program.len(), 1, "expected exactly one decl in `{src}`");
    expr_at(&program, 0).clone()
}

/// Parses a single-`val` program and returns just its pattern.
pub fn pattern_of(src: &str) -> Pattern {
    let program = parse(src);
    assert_eq!(program.len(), 1, "expected exactly one decl in `{src}`");
    pattern_at(&program, 0).clone()
}

/// Parses a single-`val` program and returns just its type annotation.
pub fn type_of(src: &str) -> Option<Type> {
    pattern_of(src).typ
}

// ---------------------------------------------------------------------------
// AST constructors
//
// These build trees to compare parser output against. Node ids and binder ids
// don't have to line up with the parsed program's: `Expr`/`Pattern` compare by
// shape and `Binder` by name (see `ast.rs`), precisely so these stay this short.
// ---------------------------------------------------------------------------

pub fn val(pat: Pattern, expr: AstExpr) -> Decl {
    Decl::ValDecl(ValDecl { pat, expr })
}

pub fn val_rec(pat: Pattern, expr: AstExpr) -> Decl {
    Decl::ValRecDecl(ValDecl { pat, expr })
}

/// An unannotated pattern.
pub fn pat(base: PatternBase) -> Pattern {
    Pattern::new(base, None)
}

/// A pattern carrying a `: type` annotation.
pub fn pat_typed(base: PatternBase, typ: Type) -> Pattern {
    Pattern::new(base, Some(typ))
}

/// The unit pattern `()`.
pub fn punit() -> Pattern {
    pat(PatternBase::Unit)
}

/// The unannotated variable pattern `name`.
pub fn pvar(name: &str) -> Pattern {
    pat(PatternBase::Var(Binder::new(name)))
}

/// The variable pattern `name : typ`.
pub fn pvar_typed(name: &str, typ: Type) -> Pattern {
    pat_typed(PatternBase::Var(Binder::new(name)), typ)
}

/// A use of the variable `name`.
pub fn ident(name: &str) -> AstExpr {
    AstExpr::var(Binder::new(name))
}

/// An expression of the given shape, with a fresh node id.
pub fn expr(kind: ExprKind) -> AstExpr {
    AstExpr::new(kind)
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// `program` pretty-printed, with the markers the last step left shown as
/// `[g..g]` (green, "just substituted here"). Everything stays on one line —
/// these tests are about content, not layout; see `renders_at_a_narrow_width` in
/// `pretty.rs` for the line-breaking ones.
pub fn show(program: &Program) -> String {
    with_view(|view| pretty::pretty_print_marked(program, view))
}

/// `program` pretty-printed with the *next* step's target marked `[y..y]` on top
/// of whatever `show` would display — exactly what the frontend renders.
pub fn render_next(program: &Program) -> String {
    with_view(|view| {
        let mut view = view.clone();
        view.set_highlights(HighlightColor::Yellow, stepping::highlight_next(program));
        pretty::pretty_print_marked(program, &view)
    })
}

/// `program` laid out to `width` columns, with no markers — for the tests that
/// are specifically about where lines break.
pub fn show_at_width(program: &Program, width: usize) -> String {
    let view = ViewState {
        width,
        ..ViewState::plain()
    };
    pretty::pretty_print_marked(program, &view)
}

/// `program` with the node `id` marked in `color` — highlighting one specific
/// node without going through `highlight_next`.
pub fn show_highlighted(program: &Program, id: NodeId, color: HighlightColor) -> String {
    let mut view = ViewState::plain();
    view.set_highlights(color, [id]);
    pretty::pretty_print_marked(program, &view)
}

/// `program` with the lambdas at `ids` folded to `fn <pat> => ...`.
pub fn show_folded(program: &Program, ids: &[NodeId]) -> String {
    let mut view = ViewState::plain();
    view.collapsed = ids.iter().copied().collect();
    pretty::pretty_print_marked(program, &view)
}

/// Drops every marker, leaving the plain program text.
pub fn strip_markers(rendered: &str) -> String {
    rendered
        .replace("[y", "")
        .replace("y]", "")
        .replace("[g", "")
        .replace("g]", "")
        .replace("[r", "")
        .replace("r]", "")
}

// ---------------------------------------------------------------------------
// Stepping
// ---------------------------------------------------------------------------

/// Performs one step, returning the new program and the message describing it,
/// and recording what the step placed so `show` can mark it green.
/// Panics if the program was already fully reduced.
pub fn step_once(program: &Program) -> (Program, String) {
    let outcome = stepping::step(program).unwrap_or_else(|| {
        panic!(
            "expected a step, but this is fully reduced:\n{}",
            show(program)
        )
    });
    VIEW.with(|v| {
        v.borrow_mut()
            .set_highlights(HighlightColor::Green, outcome.green)
    });
    (outcome.program, outcome.message)
}

/// Cap on `run_to_value`'s step count — high enough for the recursive programs in
/// `recursion.rs`, low enough to fail fast on a non-terminating reduction.
const STEP_LIMIT: usize = 200;

/// Steps `program` until nothing is left to reduce and returns its rendered text.
///
/// Also checks, at every step, that `highlight_next` and `step` agree on whether
/// there's anything left to do — the invariant that keeps the displayed
/// "next redex" from drifting out of sync with what Step actually performs.
pub fn run_to_value(mut program: Program) -> String {
    for _ in 0..STEP_LIMIT {
        let highlighted = stepping::highlight_next(&program);
        match stepping::step(&program) {
            Some(_) => {
                assert!(
                    highlighted.is_some(),
                    "step found a redex but highlight_next didn't:\n{}",
                    show(&program)
                );
                let (next, _) = step_once(&program);
                program = next;
            }
            None => {
                assert!(
                    highlighted.is_none(),
                    "highlight_next found a redex but step didn't:\n{}",
                    show(&program)
                );
                return show(&program);
            }
        }
    }
    panic!("step limit reached, still reducing:\n{}", show(&program));
}

/// `run_to_value` straight from source.
pub fn run(src: &str) -> String {
    run_to_value(parse(src))
}

/// The width `show`/`run` render at — everything on one line.
pub const TEST_WIDTH: usize = UNLIMITED_WIDTH;
