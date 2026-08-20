//! `val rec`, which is the one construct that can't be modelled by substitution
//! alone: inlining a recursive lambda at each use would leave its own name
//! dangling and grow the displayed program by a copy of the function per
//! unrolling. Instead the lambda is parked in `stepping`'s recursive-value env
//! under a name unique to this program, uses are substituted with that *name*, and
//! stepping the name unrolls one copy — which is what keeps `fact (n - 1)`
//! readable on screen.
//!
//! That env is thread-local and outlives a single `step`, so every test here
//! starts by clearing it, exactly as `enter_formula` does for a new program.

mod common;

use common::*;
use stepper_core::{parse_program, stepping, typecheck};

/// Clears the recursive-value env, then steps `src` to completion and returns the
/// final rendering. Also insists the program typechecks: `val rec`'s restrictions
/// (annotated identifier, function type, `fn` right-hand side) are what let
/// stepping assume the shape it does.
fn run_rec(src: &str) -> String {
    stepping::reset_rec_env();
    let program = parse(src);
    assert_eq!(typecheck(&program), None, "type error in `{src}`");
    run_to_value(program)
}

#[test]
fn a_recursive_decl_is_already_a_value() {
    // Nothing to reduce: a lone `val rec` is a finished program.
    assert_eq!(
        run_rec("val rec f : int -> int = fn n : int => f n"),
        "val rec f : int -> int = fn n : int => f n"
    );
}

#[test]
fn binding_a_recursive_function_substitutes_its_name_not_its_body() {
    stepping::reset_rec_env();
    let program = parse(
        "val rec fact : int -> int = fn n : int => if n = 0 then 1 else n * fact (n - 1)\nval x = fact 3",
    );

    let (program, msg) = step_once(&program);
    assert_eq!(
        msg,
        "Bound recursive fact = fn n : int => if n = 0 then 1 else n * fact (n - 1)"
    );
    // What the frontend actually receives: the green marker the substitution left
    // on the recursive name, with the yellow "next redex" marker nested inside it
    // (the name is a redex — stepping it unrolls the lambda).
    assert_eq!(show(&program), "val x = [gfactg] 3");
    assert_eq!(render_next(&program), "val x = [g[yfacty]g] 3");

    let (program, msg) = step_once(&program);
    assert_eq!(
        msg,
        "Unrolled fact to fn n : int => if n = 0 then 1 else n * fact (n - 1)"
    );
    assert_eq!(
        show(&program),
        "val x = [g(fn n : int => if n = 0 then 1 else n * fact (n - 1))g] 3"
    );
}

#[test]
fn factorial() {
    assert_eq!(
        run_rec(
            "val rec fact : int -> int = fn n : int => if n = 0 then 1 else n * fact (n - 1)\nval x = fact 3"
        ),
        "val x = 6"
    );
}

#[test]
fn a_let_bound_recursive_function() {
    assert_eq!(
        run_rec(
            "val y = let val rec countdown : int -> int = fn n : int => if n <= 0 then 0 else countdown (n - 1) in countdown 2 end"
        ),
        "val y = 0"
    );
}

#[test]
fn two_recursive_functions_can_share_a_name() {
    // Two different `val rec f`s in one program are two different functions, and
    // the first one's references must survive the second one's binding — which is
    // what the rename in `bind_rec` is for.
    assert_eq!(
        run_rec(
            "val rec f : int -> int = fn n : int => n + 1\
             \nval a : int -> int = fn z : int => f z\
             \nval rec f : int -> int = fn n : int => n * 10\
             \nval b = (a 1, f 1)"
        ),
        "val b = (2, 10)"
    );
}

#[test]
fn a_plain_val_may_shadow_a_recursive_name_afterwards() {
    // The green on `100` is just the last step's substitution marker: nothing
    // steps after it, so nothing clears it.
    assert_eq!(
        run_rec(
            "val rec f : int -> int = fn n : int => n + 1\nval a = f 1\nval f = 100\nval b = (a, f)"
        ),
        "val b = (2, [g100g])"
    );
}

#[test]
fn a_recursive_function_can_be_passed_around_as_a_value() {
    assert_eq!(
        run_rec(
            "val rec f : int -> int = fn n : int => n + 1\nval g : int -> int = f\nval h : int = g 1"
        ),
        "val h : int = 2"
    );
}

#[test]
fn recursion_through_a_curried_function() {
    // The unrolled name shows up under an application that isn't yet saturated.
    assert_eq!(
        run_rec(
            "val rec add : int -> (int -> int) = fn a : int => fn b : int => if a = 0 then b else add (a - 1) (b + 1)\nval s = add 2 3"
        ),
        "val s = 5"
    );
}

#[test]
fn a_recursive_call_inside_a_case() {
    // A recursive call in a `case` scrutinee, and a recursive function whose body
    // is itself a `case`.
    assert_eq!(
        run_rec(
            "val rec even : int -> bool = fn n : int => case n of 0 => true | _ => (case n - 1 of 0 => false | m => even (m - 1))\nval c = (even 4, even 3)"
        ),
        "val c = (true, false)"
    );
}

#[test]
fn a_recursive_function_defined_inside_a_recursive_function() {
    assert_eq!(
        run_rec(
            "val rec outer : int -> int = fn n : int => let val rec inner : int -> int = fn m : int => if m = 0 then 0 else m + inner (m - 1) in if n = 0 then 0 else inner n end\nval z = outer 3"
        ),
        "val z = 6"
    );
}

#[test]
fn re_running_the_same_program_does_not_accumulate_renames() {
    // `bind_rec` reuses a name that already holds an identical lambda, so the
    // second run must show `fact`, not `factA`.
    let src =
        "val rec fact : int -> int = fn n : int => if n = 0 then 1 else n * fact (n - 1)\nval x = fact 2";
    assert_eq!(run_rec(src), "val x = 2");
    assert_eq!(run_rec(src), "val x = 2");

    // ...and without the reset `run_rec` does, a *different* function under the
    // same name has to be renamed out of the way instead.
    stepping::reset_rec_env();
    let first = parse_program("val rec g : int -> int = fn n : int => n + 1\nval a = g 1").unwrap();
    let (_, msg) = step_once(&first);
    assert_eq!(msg, "Bound recursive g = fn n : int => n + 1");

    let second = parse_program("val rec g : int -> int = fn n : int => n * 2\nval a = g 1").unwrap();
    let (program, msg) = step_once(&second);
    assert_eq!(
        msg,
        "Bound recursive g = fn n : int => n * 2, shown as gA since g is taken"
    );
    assert_eq!(show(&program), "val a = [ggAg] 1");
    assert_eq!(run_to_value(program), "val a = 2");
}
