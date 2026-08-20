//! `highlight_next` — the yellow "this is what Step will do next" preview — plus
//! the green markers substitution leaves behind. Between them these are everything
//! the frontend renders beyond the program text itself.
//!
//! `[y..y]` is yellow (next redex), `[g..g]` green (just substituted). The
//! governing invariant is that highlighting mirrors `step`'s search order exactly,
//! so the preview can never point somewhere Step won't go; `run_to_value` asserts
//! the weaker "both agree there's something left to do" half of that on every
//! stepping test in the suite.

mod common;

use common::*;
use stepper_core::stepping;

#[test]
fn highlights_the_innermost_redex_of_an_arithmetic_expression() {
    assert_eq!(render_next(&parse("val x = 1 + 2 * 3")), "val x = 1 + [y2 * 3y]");
    assert_eq!(render_next(&parse("val x = 15 * 10")), "val x = [y15 * 10y]");
}

#[test]
fn highlights_a_finished_decl_that_is_about_to_be_substituted() {
    // The first decl is already a value but the program continues, so what happens
    // next is the substitution — the value itself is what's highlighted.
    assert_eq!(
        render_next(&parse("val x = 150\nval y = x + 1")),
        "val x = [y150y]\nval y = x + 1"
    );
}

#[test]
fn highlights_nothing_once_the_program_is_fully_reduced() {
    let program = parse("val x = 15150");
    assert_eq!(stepping::highlight_next(&program), None);
    assert_eq!(render_next(&program), "val x = 15150");
}

#[test]
fn tracks_a_program_step_by_step() {
    let program = parse("val x = 15 * 10\nval y = x + 15 * 1000");
    assert_eq!(
        render_next(&program),
        "val x = [y15 * 10y]\nval y = x + 15 * 1000"
    );

    let (program, _) = step_once(&program);
    assert_eq!(
        render_next(&program),
        "val x = [y150y]\nval y = x + 15 * 1000"
    );

    let (program, _) = step_once(&program);
    // Green from the substitution is still present, and highlight_next also finds
    // the next redex (15 * 1000) alongside it — the two are disjoint here, so they
    // render as separate, non-nested regions in the same line.
    assert_eq!(
        render_next(&program),
        "val y = [g150g] + [y15 * 1000y]"
    );

    let (program, _) = step_once(&program);
    assert_eq!(render_next(&program), "val y = [y150 + 15000y]");

    let (program, _) = step_once(&program);
    assert_eq!(render_next(&program), "val y = 15150");
}

#[test]
fn yellow_nests_around_a_green_substituted_value() {
    // A green-wrapped value still counts as a value, so the yellow wrapper for the
    // surrounding redex ends up nested around it rather than replacing it.
    let (program, _) = step_once(&parse("val x = 5\nval y = x + x"));
    assert_eq!(show(&program), "val y = [g5g] + [g5g]");
    assert_eq!(render_next(&program), "val y = [y[g5g] + [g5g]y]");
}

#[test]
fn yellow_nests_around_a_green_substituted_condition() {
    let (program, _) = step_once(&parse("val b = true\nval x = if b then 1 else 2"));
    assert_eq!(
        render_next(&program),
        "val x = [yif [gtrueg] then 1 else 2y]"
    );
}

#[test]
fn green_clears_after_the_next_step() {
    let (program, _) = step_once(&parse("val x = 5\nval y = x + x"));
    assert_eq!(show(&program), "val y = [g5g] + [g5g]");
    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 5 + 5 to 10");
    assert_eq!(show(&program), "val y = 10");
}

#[test]
fn finds_the_first_unreduced_tuple_element() {
    assert_eq!(
        render_next(&parse("val x = (1 + 1, 2 + 2)")),
        "val x = ([y1 + 1y], 2 + 2)"
    );
    // Once every element is a value the tuple is one too — there's no "whole
    // tuple" redex to highlight, so the next thing is the decl itself.
    assert_eq!(
        render_next(&parse("val x = (1, 2)\nval y = 1")),
        "val x = [y(1, 2)y]\nval y = 1"
    );
}

#[test]
fn highlights_the_if_only_once_its_condition_is_a_value() {
    assert_eq!(
        render_next(&parse("val x = if 3 < 5 then 1 else 2")),
        "val x = if [y3 < 5y] then 1 else 2"
    );
    assert_eq!(
        render_next(&parse("val x = if true then 1 + 1 else 2")),
        "val x = [yif true then 1 + 1 else 2y]"
    );
}

#[test]
fn shortcircuit_operators_only_wait_on_their_left_operand() {
    // `r` is never searched: `andalso` can fire as soon as `l` is a bool, so the
    // whole expression is the redex even though the right side isn't reduced.
    assert_eq!(
        render_next(&parse("val x = false andalso (1 + 1 = 2)")),
        "val x = [yfalse andalso 1 + 1 = 2y]"
    );
    assert_eq!(
        render_next(&parse("val x = (1 < 2) andalso true")),
        "val x = [y1 < 2y] andalso true"
    );
}

#[test]
fn case_highlights_the_scrutinee_then_the_whole_match() {
    let program = parse("val x = case 1 + 1 of 2 => 10 | _ => 20");
    assert_eq!(
        render_next(&program),
        "val x = case [y1 + 1y] of 2 => 10 | _ => 20"
    );

    // Which arm wins depends on the scrutinee's value, which highlighting can't
    // see ahead of time — so once the scrutinee is a value, the whole `case` is
    // what's next.
    let (program, _) = step_once(&program);
    assert_eq!(
        render_next(&program),
        "val x = [ycase 2 of 2 => 10 | _ => 20y]"
    );
}

#[test]
fn let_highlights_its_first_decl_then_the_collapse() {
    let program = parse("val y = let val x = 1 + 1 in x * 10 end");
    assert_eq!(
        render_next(&program),
        "val y = let val x = [y1 + 1y] in x * 10 end"
    );

    // The decl's value is ready: what happens next is substituting it into the
    // body, so the value is what's marked.
    let (program, _) = step_once(&program);
    assert_eq!(
        render_next(&program),
        "val y = let val x = [y2y] in x * 10 end"
    );
}

#[test]
fn an_empty_let_is_a_redex_in_itself() {
    // Nothing left to bind: the whole `let` is about to collapse into its body.
    assert_eq!(
        render_next(&parse("val y = let in 5 end")),
        "val y = [ylet in 5 endy]"
    );
}

#[test]
fn application_walks_the_function_then_the_argument_then_the_whole_redex() {
    // The argument's own parens are printed inside the sentinels, since
    // `Expr::Highlighted` passes `min_prec` through to whatever it wraps.
    let program = parse("val y = (fn x : int => x) (1 + 1)");
    assert_eq!(
        render_next(&program),
        "val y = (fn x : int => x) [y(1 + 1)y]"
    );

    let (program, _) = step_once(&program);
    assert_eq!(render_next(&program), "val y = [y(fn x : int => x) 2y]");
}

#[test]
fn a_lambda_alone_is_a_value_with_nothing_to_highlight() {
    let program = parse("val f : int -> int = fn x : int => x + 1");
    assert_eq!(stepping::highlight_next(&program), None);
}
