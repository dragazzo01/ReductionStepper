//! The small-step semantics: which redex `step` picks, what message it reports,
//! and what the program looks like afterwards. `[g..g]` in an expected rendering
//! is the green "this was just substituted here" marker (see `highlight.rs` for
//! the marker rules themselves).
//!
//! Recursion lives in `recursion.rs`; everything else is here.

mod common;

use common::*;
use stepper_core::stepping;

#[test]
fn reduces_arithmetic_left_to_right() {
    let program = parse("val x = 1 + 2 * 3");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 2 * 3 to 6");
    assert_eq!(show(&program), "val x = 1 + 6");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 1 + 6 to 7");
    assert_eq!(show(&program), "val x = 7");

    assert_eq!(stepping::step(&program), None);
}

#[test]
fn uses_sml_floor_division_not_truncating_division() {
    // Floor semantics: ~7 div 2 = ~4 (not ~3, which truncating division gives),
    // and ~7 mod 2 = 1 (always same sign as the divisor).
    assert_eq!(run("val x = ~7 div 2"), "val x = ~4");
    assert_eq!(run("val x = ~7 mod 2"), "val x = 1");
    assert_eq!(run("val x = 7 div 2"), "val x = 3");
    assert_eq!(run("val x = 7 mod ~2"), "val x = ~1");
}

#[test]
fn unit_is_already_a_value() {
    assert_eq!(run("val x = ()"), "val x = ()");
    // Nothing to step inside a tuple of values either.
    assert_eq!(run("val p = ((), 1)"), "val p = ((), 1)");
}

#[test]
fn applies_a_function_to_unit() {
    assert_eq!(run("val f = fn () => 1 + 1\nval y = f ()"), "val y = 2");
}

#[test]
fn a_unit_pattern_matches_and_binds_nothing() {
    // One value inhabits the type, so the match always succeeds.
    assert_eq!(run("val () = ()\nval x = 5"), "val x = 5");
    assert_eq!(run("val x = case () of () => 7"), "val x = 7");
}

#[test]
fn compares_values_of_any_equality_type() {
    assert_eq!(run("val b = true = true"), "val b = true");
    assert_eq!(run("val b = true = false"), "val b = false");
    assert_eq!(run("val b = true <> false"), "val b = true");
    // One value inhabits unit, so these are equal by construction.
    assert_eq!(run("val b = () = ()"), "val b = true");
    assert_eq!(run("val b = () <> ()"), "val b = false");
}

#[test]
fn compares_tuples_componentwise() {
    assert_eq!(run("val b = (1, true) = (1, true)"), "val b = true");
    assert_eq!(run("val b = (1, true) = (1, false)"), "val b = false");
    assert_eq!(run("val b = (1, (2, ())) = (1, (2, ()))"), "val b = true");
    assert_eq!(run("val b = (1, (2, ())) <> (1, (3, ()))"), "val b = true");
}

#[test]
fn a_comparison_reduces_both_sides_first() {
    // Same left-then-right order as arithmetic: three steps, not one.
    let program = parse("val b = (1 + 1, true) = (2, 1 < 2)");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 1 + 1 to 2");
    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 1 < 2 to true");
    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated (2, true) = (2, true) to true");
    assert_eq!(show(&program), "val b = true");
}

#[test]
fn reduces_unary_minus() {
    // A negative literal is already a value: nothing to step.
    assert_eq!(run("val x = ~5"), "val x = ~5");
    // `~ ~5` needs the space, or the two `~`s lex as one identifier.
    assert_eq!(run("val x = ~ ~5"), "val x = 5");
    assert_eq!(run("val x = ~(2 + 3)"), "val x = ~5");
}

// ---------------------------------------------------------------------------
// if
// ---------------------------------------------------------------------------

#[test]
fn reduces_the_condition_before_selecting_a_branch() {
    // The condition is itself a not-yet-reduced if-expression, so it must be
    // stepped to a bool before either branch can be selected.
    let program = parse("val x = if (if true then true else false) then 1 else 2");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated if true then true else false to true");
    assert_eq!(show(&program), "val x = if true then 1 else 2");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated if true then 1 else 2 to 1");
    assert_eq!(show(&program), "val x = 1");
}

#[test]
fn discards_the_untaken_branch_unevaluated() {
    // If the else-branch were mistakenly evaluated first, this would take two
    // steps (reduce 2 + 3, then select the then-branch) instead of one.
    let (program, _) = step_once(&parse("val x = if true then 1 else 2 + 3"));
    assert_eq!(show(&program), "val x = 1");
    assert_eq!(stepping::step(&program), None);
}

#[test]
fn if_substitutes_through_to_its_condition() {
    let program = parse("val b = true\nval x = if b then 1 else 2");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Substituted b = true");
    assert_eq!(show(&program), "val x = if [gtrueg] then 1 else 2");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated if true then 1 else 2 to 1");
    assert_eq!(show(&program), "val x = 1");
}

// ---------------------------------------------------------------------------
// Comparisons, andalso/orelse
// ---------------------------------------------------------------------------

#[test]
fn reduces_each_comparison_operator() {
    let cases = [
        ("val x = 1 = 1", "true"),
        ("val x = 1 = 2", "false"),
        ("val x = 1 <> 2", "true"),
        ("val x = 2 < 3", "true"),
        ("val x = 3 < 3", "false"),
        ("val x = 3 <= 3", "true"),
        ("val x = 4 > 3", "true"),
        ("val x = 3 >= 3", "true"),
    ];
    for (src, expected) in cases {
        assert_eq!(run(src), format!("val x = {expected}"), "stepping {src:?}");
    }
}

#[test]
fn comparison_operands_reduce_before_comparing() {
    let program = parse("val x = 1 + 2 < 10 div 2");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 1 + 2 to 3");
    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 10 div 2 to 5");
    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 3 < 5 to true");
    assert_eq!(show(&program), "val x = true");
}

#[test]
fn a_comparison_feeds_an_if_condition() {
    let program = parse("val x = if 3 < 5 then 1 else 2");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 3 < 5 to true");
    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated if true then 1 else 2 to 1");
    assert_eq!(show(&program), "val x = 1");
}

#[test]
fn andalso_short_circuits_false_without_evaluating_the_right() {
    // If the right side were mistakenly evaluated, this would take two steps
    // (reduce 1 + 1 = 2, then short-circuit) instead of one.
    let (program, msg) = step_once(&parse("val x = false andalso (1 + 1 = 2)"));
    assert_eq!(msg, "Evaluated false andalso 1 + 1 = 2 to false");
    assert_eq!(show(&program), "val x = false");
    assert_eq!(stepping::step(&program), None);
}

#[test]
fn orelse_short_circuits_true_without_evaluating_the_right() {
    let (program, msg) = step_once(&parse("val x = true orelse (1 + 1 = 2)"));
    assert_eq!(msg, "Evaluated true orelse 1 + 1 = 2 to true");
    assert_eq!(show(&program), "val x = true");
    assert_eq!(stepping::step(&program), None);
}

#[test]
fn andalso_and_orelse_evaluate_the_right_when_they_must() {
    assert_eq!(run("val x = true andalso (1 < 2)"), "val x = true");
    assert_eq!(run("val x = false orelse (1 < 2)"), "val x = true");
}

// ---------------------------------------------------------------------------
// let and substitution
// ---------------------------------------------------------------------------

#[test]
fn let_reduces_its_decl_substitutes_then_collapses() {
    let program = parse("val y = let val x = 1 + 1 in x * 10 end");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 1 + 1 to 2");
    assert_eq!(show(&program), "val y = let val x = 2 in x * 10 end");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Substituted x = 2");
    assert_eq!(show(&program), "val y = [g2g] * 10");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 2 * 10 to 20");
    assert_eq!(show(&program), "val y = 20");

    assert_eq!(stepping::step(&program), None);
}

#[test]
fn let_shadowing_protects_an_inner_rebinding_from_an_outer_substitution() {
    // If a substitution from outside the let naively touched the let's own body
    // (ignoring that its `val x` rebinds the name), this would wrongly reach 6
    // (1 + 5, using the *outer* x) instead of 11 (10 + 1, using the *inner* x).
    let program = parse("val x = 5\nval y = let val x = 10 in x + 1 end");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Substituted x = 5");
    // Nothing to substitute: the let's own `x` shadows the outer one throughout.
    assert_eq!(show(&program), "val y = let val x = 10 in x + 1 end");

    assert_eq!(run_to_value(program), "val y = 11");
}

#[test]
fn top_level_redeclaration_shadows_an_earlier_binding() {
    // Same shadowing rule as the let case above, at the top level: y's `x` must
    // refer to the *second* (nearer) declaration, giving 11, not 6.
    assert_eq!(run("val x = 5\nval x = 10\nval y = x + 1"), "val y = 11");
}

#[test]
fn substitution_reaches_every_occurrence_of_the_name() {
    let (program, msg) = step_once(&parse("val x = 5\nval y = x + x"));
    assert_eq!(msg, "Substituted x = 5");
    assert_eq!(show(&program), "val y = [g5g] + [g5g]");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 5 + 5 to 10");
    assert_eq!(show(&program), "val y = 10");
}

#[test]
fn the_full_worked_example_from_the_project_origin() {
    // The exact example that originally motivated this project: nested let with
    // shadowing, substitution across several decls, and a final comparison.
    assert_eq!(
        run("val x = 15 * 10
             val y = x + 15 * 1000
             val z =
                 let
                     val x = 10
                 in
                     y < x
                 end"),
        "val z = false"
    );
}

#[test]
fn walks_a_two_decl_program_end_to_end() {
    let program = parse("val x = 15 * 10\nval y = x + 15 * 1000");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 15 * 10 to 150");
    assert_eq!(show(&program), "val x = 150\nval y = x + 15 * 1000");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Substituted x = 150");
    assert_eq!(show(&program), "val y = [g150g] + 15 * 1000");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 15 * 1000 to 15000");
    // The green marker from the previous step is gone: `step` strips leftover
    // highlights before reducing, which is what makes green fade after one step.
    assert_eq!(show(&program), "val y = 150 + 15000");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 150 + 15000 to 15150");
    assert_eq!(show(&program), "val y = 15150");

    assert_eq!(stepping::step(&program), None);
}

// ---------------------------------------------------------------------------
// Tuples and patterns
// ---------------------------------------------------------------------------

#[test]
fn reduces_tuple_elements_left_to_right_then_stops() {
    let program = parse("val x = (1 + 1, 2 + 2)");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 1 + 1 to 2");
    assert_eq!(show(&program), "val x = (2, 2 + 2)");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 2 + 2 to 4");
    assert_eq!(show(&program), "val x = (2, 4)");

    // A tuple of values is itself a value: nothing left to do.
    assert_eq!(stepping::step(&program), None);
}

#[test]
fn destructures_a_tuple_pattern_into_multiple_bindings() {
    let (program, msg) = step_once(&parse("val (x, y) = (1, 2)\nval z = x + y"));
    assert_eq!(msg, "Substituted (x, y) = (1, 2)");
    assert_eq!(show(&program), "val z = [g1g] + [g2g]");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 1 + 2 to 3");
    assert_eq!(show(&program), "val z = 3");
}

#[test]
fn a_wildcard_decl_is_consumed_without_substituting_anything() {
    let (program, msg) = step_once(&parse("val _ = 5\nval x = 1"));
    assert_eq!(msg, "Substituted _ = 5");
    assert_eq!(show(&program), "val x = 1");
}

#[test]
fn tuple_pattern_bindings_shadow_independently() {
    // The tuple decl binds both x and y; the later `val x = 10` shadows only x,
    // not y, so z should end up seeing the *second* x (still free, pending its own
    // step) but the *first* y (already substituted in).
    let (program, msg) = step_once(&parse("val (x, y) = (1, 2)\nval x = 10\nval z = x + y"));
    assert_eq!(msg, "Substituted (x, y) = (1, 2)");
    assert_eq!(show(&program), "val x = 10\nval z = x + [g2g]");
    assert_eq!(run_to_value(program), "val z = 12");
}

#[test]
fn let_destructures_a_tuple_pattern() {
    let (program, msg) = step_once(&parse("val z = let val (x, y) = (1, 2) in x + y end"));
    assert_eq!(msg, "Substituted (x, y) = (1, 2)");
    assert_eq!(show(&program), "val z = [g1g] + [g2g]");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 1 + 2 to 3");
    assert_eq!(show(&program), "val z = 3");
}

#[test]
#[should_panic(expected = "Bind failure")]
fn a_literal_val_pattern_that_does_not_match_is_fatal() {
    // Real SML raises `Bind` here; until this project has exceptions that's a
    // panic (see `HighlightColor::Red`). The mismatch only surfaces once the decl
    // is actually consumed, i.e. when there's a later decl to substitute into.
    run("val 5 = 2 + 2\nval x = 1");
}

// ---------------------------------------------------------------------------
// case
// ---------------------------------------------------------------------------

#[test]
fn case_reduces_the_scrutinee_before_selecting_an_arm() {
    let program = parse("val x = case 1 + 1 of 2 => 10 | _ => 20");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 1 + 1 to 2");
    assert_eq!(show(&program), "val x = case 2 of 2 => 10 | _ => 20");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Substituted 2 = 2");
    assert_eq!(show(&program), "val x = 10");
}

#[test]
fn case_tries_arms_in_order_and_stops_at_the_first_match() {
    assert_eq!(run("val x = case 5 of 0 => 1 | _ => 2"), "val x = 2");
    assert_eq!(run("val x = case 5 of 5 => 1 | _ => 2"), "val x = 1");
}

#[test]
fn case_binds_pattern_variables_into_the_chosen_arm() {
    let (program, msg) = step_once(&parse("val x = case (1, 2) of (a, b) => a + b"));
    assert_eq!(msg, "Substituted (a, b) = (1, 2)");
    assert_eq!(show(&program), "val x = [g1g] + [g2g]");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 1 + 2 to 3");
    assert_eq!(show(&program), "val x = 3");
}

#[test]
fn an_unmatched_arms_variables_are_not_substituted() {
    // Only the chosen arm's bindings should ever reach substitution; a variable
    // with the same name in a discarded arm must not leak in.
    assert_eq!(run("val x = case 1 of 1 => 10 | y => y"), "val x = 10");
}

#[test]
#[should_panic(expected = "Match failure")]
fn case_panics_when_no_arm_matches() {
    // SML's `Match` exception, as a panic for now.
    run("val x = case 1 of 0 => 10");
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

#[test]
fn application_reduces_the_function_then_the_argument_then_substitutes() {
    let program = parse("val y = (fn x : int => x + 1) (2 * 3)");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 2 * 3 to 6");
    assert_eq!(show(&program), "val y = (fn x : int => x + 1) 6");

    // Applying a lambda is the same pattern-substitution a `case` arm gets — the
    // message names the parameter pattern, annotation included.
    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Substituted x : int = 6");
    assert_eq!(show(&program), "val y = [g6g] + 1");

    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Evaluated 6 + 1 to 7");
    assert_eq!(show(&program), "val y = 7");
}

#[test]
fn application_binds_tighter_than_infix_when_stepping_too() {
    // `f 2 + 1` is `(f 2) + 1` = 21, not `f (2 + 1)` = 30.
    assert_eq!(
        run("val f : int -> int = fn x : int => x * 10 val y = f 2 + 1"),
        "val y = 21"
    );
}

#[test]
fn a_lambda_is_a_value_and_gets_substituted_whole() {
    let (program, msg) = step_once(&parse("val f : int -> int = fn x : int => x * 10\nval y = f 2"));
    assert_eq!(msg, "Substituted f : int -> int = fn x : int => x * 10");
    // Substituted into an application's function position, it needs its parens
    // back — inside the green marker, since that's the position it occupies.
    assert_eq!(show(&program), "val y = [g(fn x : int => x * 10)g] 2");
    assert_eq!(run_to_value(program), "val y = 20");
}

#[test]
fn a_multi_arm_lambda_matches_its_argument_against_the_arms_in_order() {
    assert_eq!(run("val f = fn n : int => n * 2 | 0 => 100 val y = f 3"), "val y = 6");
    // The first arm is a variable pattern, so it always matches — the literal arm
    // after it is unreachable, exactly as SML's redundant-match warning would say.
    assert_eq!(run("val f = fn n : int => n * 2 | 0 => 100 val y = f 0"), "val y = 0");
    // With the literal first (and the annotation moved onto it), the literal wins.
    assert_eq!(run("val f = fn 0 : int => 100 | n => n * 2 val y = f 0"), "val y = 100");
    assert_eq!(run("val f = fn 0 : int => 100 | n => n * 2 val y = f 3"), "val y = 6");
}

#[test]
fn a_curried_function_applies_one_argument_per_step_pair() {
    assert_eq!(
        run("val add : int -> int -> int = fn a : int => fn b : int => a + b\nval s = add 1 2"),
        "val s = 3"
    );
}

#[test]
fn a_function_passed_as_an_argument_is_applied_when_it_lands() {
    assert_eq!(
        run("val twice : (int -> int) -> int = fn g : int -> int => g (g 1)\
             \nval y = twice (fn x : int => x * 3)"),
        "val y = 9"
    );
}

#[test]
fn a_fun_declared_function_steps_like_the_lambda_it_is() {
    let program = parse("fun double (n : int) = n * 2\nval y = double 4");

    // Nothing about the first step is `fun`-specific: it's a `val` bound to a
    // lambda, so it's already a value and gets substituted whole.
    let (program, msg) = step_once(&program);
    assert_eq!(msg, "Substituted double = fn n : int => n * 2");
    assert_eq!(show(&program), "val y = [g(fn n : int => n * 2)g] 4");

    assert_eq!(run_to_value(program), "val y = 8");
}

#[test]
fn a_multi_clause_fun_picks_the_first_clause_that_matches() {
    let src = "fun f (0 : int) = 100 | f n = n * 2\n";
    assert_eq!(run(&format!("{src}val y = f 0")), "val y = 100");
    assert_eq!(run(&format!("{src}val y = f 3")), "val y = 6");
}

#[test]
fn a_fun_that_takes_a_tuple_apart_binds_its_components() {
    assert_eq!(
        run("fun add (x : int, y : int) = x + y\nval s = add (1, 2)"),
        "val s = 3"
    );
}
