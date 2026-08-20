//! Shared helpers for the integration test suite.
//!
//! Every test file here drives the crate through its public API only
//! (`parse_program`, `typecheck`, `pretty`, `stepping`), the same surface the WASM
//! frontend uses. The helpers below exist to keep the tests themselves readable:
//! short AST constructors (`val`, `pvar`, ...) for the parser tests, and
//! marker-aware rendering (`show`, `render_next`) for the stepping/highlight ones.

#![allow(dead_code)]

use stepper_core::ast::{Decl, Expr, Pattern, PatternBase, Program, Type, ValDecl};
use stepper_core::{parse_program, pretty, stepping};

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parses `src`, panicking with the parse error if it doesn't.
pub fn parse(src: &str) -> Program {
    parse_program(src).unwrap_or_else(|e| panic!("failed to parse `{src}`:\n{e}"))
}

/// The `i`th declaration's right-hand side.
pub fn expr_at(program: &Program, i: usize) -> &Expr {
    &program[i].get_val_decl().expr
}

/// The `i`th declaration's pattern, type annotation included.
pub fn pattern_at(program: &Program, i: usize) -> &Pattern {
    &program[i].get_val_decl().pat
}

/// The `i`th declaration's pattern with its annotation dropped — what the old
/// `Decl.pat` field held before the annotation moved onto `Pattern` itself.
pub fn pattern_base_at(program: &Program, i: usize) -> &PatternBase {
    &pattern_at(program, i).pat
}

/// The `i`th declaration's type annotation, if it has one.
pub fn type_at(program: &Program, i: usize) -> Option<&Type> {
    pattern_at(program, i).typ.as_ref()
}

/// Parses a single-`val` program and returns just its right-hand-side expression.
pub fn expr_of(src: &str) -> Expr {
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
// ---------------------------------------------------------------------------

pub fn val(pat: Pattern, expr: Expr) -> Decl {
    Decl::ValDecl(ValDecl { pat, expr })
}

pub fn val_rec(pat: Pattern, expr: Expr) -> Decl {
    Decl::ValRecDecl(ValDecl { pat, expr })
}

/// An unannotated pattern.
pub fn pat(base: PatternBase) -> Pattern {
    Pattern {
        pat: base,
        typ: None,
    }
}

/// A pattern carrying a `: type` annotation.
pub fn pat_typed(base: PatternBase, typ: Type) -> Pattern {
    Pattern {
        pat: base,
        typ: Some(typ),
    }
}

/// The unannotated variable pattern `name`.
pub fn pvar(name: &str) -> Pattern {
    pat(PatternBase::Ident(name.to_string()))
}

/// The variable pattern `name : typ`.
pub fn pvar_typed(name: &str, typ: Type) -> Pattern {
    pat_typed(PatternBase::Ident(name.to_string()), typ)
}

pub fn ident(name: &str) -> Expr {
    Expr::Ident(name.to_string())
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Rewrites `pretty`'s Private Use Area sentinels into readable markers, so
/// expected strings can be written literally: `[y...y]` is the yellow "next redex"
/// highlight, `[g...g]` the green "just substituted" one, `[r...r]` red. Square
/// brackets never occur in program text, so this stays unambiguous.
pub fn markers(rendered: &str) -> String {
    rendered
        .replace(pretty::HIGHLIGHT_START, "[y")
        .replace(pretty::HIGHLIGHT_END, "y]")
        .replace(pretty::GREEN_START, "[g")
        .replace(pretty::GREEN_END, "g]")
        .replace(pretty::RED_START, "[r")
        .replace(pretty::RED_END, "r]")
}

/// Drops every highlight sentinel, leaving the plain program text.
pub fn strip_markers(rendered: &str) -> String {
    rendered
        .chars()
        .filter(|c| !('\u{E000}'..='\u{F8FF}').contains(c))
        .collect()
}

/// `program` pretty-printed, with any highlights it carries shown as `[y..y]` /
/// `[g..g]` markers.
pub fn show(program: &Program) -> String {
    markers(&pretty::pretty_print(program))
}

/// `program` pretty-printed with the *next* step's target marked yellow — exactly
/// what the frontend displays (see `stepper_core::current_render`).
pub fn render_next(program: &Program) -> String {
    match stepping::highlight_next(program) {
        Some(highlighted) => show(&highlighted),
        None => show(program),
    }
}

// ---------------------------------------------------------------------------
// Stepping
// ---------------------------------------------------------------------------

/// Performs one step, returning the new program and the message describing it.
/// Panics if the program was already fully reduced.
pub fn step_once(program: &Program) -> (Program, String) {
    let outcome = stepping::step(program)
        .unwrap_or_else(|| panic!("expected a step, but this is fully reduced:\n{}", show(program)));
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
            Some(outcome) => {
                assert!(
                    highlighted.is_some(),
                    "step found a redex but highlight_next didn't:\n{}",
                    show(&program)
                );
                program = outcome.program;
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
