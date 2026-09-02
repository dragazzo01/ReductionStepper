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
    // Binding a `val rec` places no text at all, so nothing is green: `fact` was
    // already written there, and it already referred to this binder. (It used to
    // be marked, back when binding one meant substituting every use with a
    // freshly-invented unique spelling of its name.) It is still a redex, though —
    // stepping the name unrolls the lambda — so yellow points at it.
    assert_eq!(show(&program), "val x = fact 3");
    assert_eq!(render_next(&program), "val x = [yfacty] 3");

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
fn two_different_functions_may_share_a_name_without_being_renamed() {
    // Re-entering the same program is unremarkable: each run's `fact` is its own
    // binder, parked under its own id.
    let src =
        "val rec fact : int -> int = fn n : int => if n = 0 then 1 else n * fact (n - 1)\nval x = fact 2";
    assert_eq!(run_rec(src), "val x = 2");
    assert_eq!(run_rec(src), "val x = 2");

    // The interesting case is two *different* functions called `g`, both live in
    // the env at once, with no reset in between. Because the env is keyed by
    // binding site rather than by name, neither can shadow the other and neither
    // needs renaming: this used to report ", shown as gA since g is taken" and
    // display the second one as `gA` forever after.
    stepping::reset_rec_env();
    let first = parse_program("val rec g : int -> int = fn n : int => n + 1\nval a = g 1").unwrap();
    let (first, msg) = step_once(&first);
    assert_eq!(msg, "Bound recursive g = fn n : int => n + 1");

    let second = parse_program("val rec g : int -> int = fn n : int => n * 2\nval a = g 1").unwrap();
    let (second, msg) = step_once(&second);
    assert_eq!(msg, "Bound recursive g = fn n : int => n * 2");
    assert_eq!(show(&second), "val a = g 1");

    // Both still run, and each unrolls its own lambda rather than the other's.
    assert_eq!(run_to_value(second), "val a = 2");
    assert_eq!(run_to_value(first), "val a = 2");
}

#[test]
fn a_fun_that_calls_itself_is_a_recursive_binding() {
    // `fun` elaborates to the same `val rec` (see `frontend::elaborate`), so it
    // gets the same parked-lambda treatment — the message names the function, not
    // the `fun` it was written as.
    stepping::reset_rec_env();
    let program = parse("fun fact (n : int) : int = if n = 0 then 1 else n * fact (n - 1)\nval x = fact 3");

    let (program, msg) = step_once(&program);
    assert_eq!(
        msg,
        "Bound recursive fact = fn n : int => if n = 0 then 1 else n * fact (n - 1)"
    );
    assert_eq!(show(&program), "val x = fact 3");
    assert_eq!(run_to_value(program), "val x = 6");
}

#[test]
fn a_fun_declared_inside_a_let_is_recursive_too() {
    assert_eq!(
        run_rec("val y = let fun countdown (n : int) : int = if n <= 0 then 0 else countdown (n - 1) in countdown 2 end"),
        "val y = 0"
    );
}

#[test]
fn psuedo_cps_test() {
    // Continuation-passing style is what forces the stepper to rename bound
    // variables when it duplicates a subtree (`subst::refresh_binders`): every
    // unrolling builds another `fn res => k (x * res)`, and `k` is then
    // substituted with a previous one, so without renaming the two `res`
    // parameters would be the same variable and the outer one would capture the
    // inner. This came out as 3 rather than 6 before that was fixed.
    //
    // The green on the result is the last step's substitution marker — the final
    // `k 1` is a substitution, and nothing steps after it to clear the mark. Same
    // as `recursion.rs`'s `a_plain_val_may_shadow_a_recursive_name_afterwards`.
    assert_eq!(
        run_rec("fun factCPS (0 : int) (k : int -> int) : int = k 1
            | factCPS x k = factCPS (x-1) (fn res : int => k (x * res))
            val _ = factCPS 3 (fn x : int => x)"),
        "val _ = [g6g]"
    )
}
