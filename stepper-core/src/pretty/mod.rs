//! Printing a program: precedence-aware, width-aware, and backend-agnostic.
//!
//! The pipeline is `Program -> Doc -> Vec<Item> -> output`:
//!
//! - [`build`] turns the tree into a [`Doc`](doc::Doc), deciding parentheses
//!   (from precedence) and where lines are *allowed* to break (from grouping);
//! - [`doc::layout`] decides which of those breaks actually happen, at a given
//!   width, and flattens the result to [`Item`](doc::Item)s;
//! - a backend turns those items into something — [`render`] into a string,
//!   `crate::dom` into DOM nodes.
//!
//! The round-trip property the tests lean on — parse, print, parse again, get the
//! same tree — holds at `UNLIMITED_WIDTH`, where no group ever breaks and the
//! output is the single-line form. It holds at finite widths too, since a break
//! only ever replaces a space with a newline and the lexer treats both as
//! whitespace. The one exception is a *folded* lambda, which deliberately prints
//! an un-reparseable `...` (see `build::collapsed_lambda_doc`).

mod build;
pub mod doc;
mod render;

pub use doc::{Ann, Item};
pub use render::{render_marked, render_string};

use crate::ast::{Expr, Pattern, Program, Type};
use crate::view::ViewState;

/// Lays `program` out under `view`, ready for a backend to consume.
pub fn layout(program: &Program, view: &ViewState) -> Vec<Item> {
    doc::layout(&build::program_doc(program, view), view.width)
}

/// `program` as plain single-line-per-decl text: nothing highlighted, nothing
/// folded, no line ever broken. This is the canonical form the parser round-trips
/// against, and what the stepper's messages are built out of.
pub fn pretty_print(program: &Program) -> String {
    render_string(&layout(program, &ViewState::plain()))
}

/// `program` printed under `view`, with highlights shown as `[y..y]`/`[g..g]`
/// markers. Used by the tests, which need to assert on where the highlights are.
pub fn pretty_print_marked(program: &Program, view: &ViewState) -> String {
    render_marked(&layout(program, view), view)
}

/// One expression, on one line — for embedding in a stepper message.
pub fn pretty_print_expr(expr: &Expr) -> String {
    let view = ViewState::plain();
    let doc = build::expr_doc(expr, 0, &view);
    render_string(&doc::layout(&doc, view.width))
}

/// One pattern, on one line — for embedding in a stepper message.
pub fn pretty_print_pattern(pat: &Pattern) -> String {
    let view = ViewState::plain();
    let doc = build::pattern_doc(pat, &view);
    render_string(&doc::layout(&doc, view.width))
}

/// One type. Types never break, so this needs no width.
pub fn pretty_print_type(ty: &Type) -> String {
    build::type_string(ty)
}
