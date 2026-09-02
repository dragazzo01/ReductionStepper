//! Typechecking: scope and shadowing rules, and the errors reported when a program
//! doesn't check. Error strings are asserted verbatim where they're stable and
//! user-facing, and loosely (`starts_with`) where only the rejection matters.

mod common;

use common::*;
use stepper_core::typecheck;

/// Typechecks `src`, returning the error message if it has one.
fn check(src: &str) -> Option<String> {
    typecheck(&parse(src))
}

/// Asserts `src` typechecks cleanly.
fn accepts(src: &str) {
    assert_eq!(check(src), None, "expected `{src}` to typecheck");
}

/// Asserts `src` is rejected, with `message` as the reported error.
fn rejects(src: &str, message: &str) {
    assert_eq!(check(src), Some(message.to_string()), "typechecking `{src}`");
}

/// Asserts `src` is rejected, without pinning the exact wording.
fn rejects_somehow(src: &str) {
    assert!(check(src).is_some(), "expected `{src}` to be rejected");
}

#[test]
fn accepts_a_well_typed_program() {
    accepts("val x = 1\nval y = x + 1");
}

#[test]
fn rejects_a_declared_type_mismatch() {
    rejects("val x : bool = 1 + 2", "Expected Bool but got type Int");
}

#[test]
fn rejects_an_unbound_identifier() {
    rejects("val x = y + 1", "Unbound identifier: y");
}

#[test]
fn rejects_a_chained_comparison() {
    // SML's comparisons are left-associative rather than nonassociative, so
    // `1 < 2 < 3` parses (see `grammar.rs`'s
    // `chained_comparisons_group_to_the_left`) and is caught here instead: the
    // left operand of the outer `<` is a bool.
    rejects("val x = 1 < 2 < 3", "Expected Int but got type Bool");
}

#[test]
fn later_decls_see_earlier_bindings_but_not_the_reverse() {
    // `x` is visible to `y`, matching evaluation order, but declaring them in the
    // other order should fail: `y` isn't in scope yet when `x` is checked.
    accepts("val x = 1\nval y = x + 1");
    rejects("val y = x + 1\nval x = 1", "Unbound identifier: x");
}

#[test]
fn top_level_redeclaration_shadows_for_later_decls() {
    // `y` should see the second (bool) `x`, not the first (int) one.
    accepts("val x = 1\nval x = true\nval y = x");
}

// ---------------------------------------------------------------------------
// if / let
// ---------------------------------------------------------------------------

#[test]
fn if_requires_a_bool_condition() {
    rejects("val x = if 1 then 2 else 3", "Expected Bool but got type Int");
}

#[test]
fn if_requires_both_branches_to_match() {
    rejects(
        "val x = if true then 1 else false",
        "Expected Int but got type Bool",
    );
    // Exercises `Type::Product` comparison the same way, with two
    // differently-shaped tuples meeting at an `if`.
    rejects(
        "val x = if true then (1, true) else (1, 2)",
        "Expected Product([Int, Bool]) but got type Product([Int, Int])",
    );
}

#[test]
fn if_of_matching_branches_infers_their_type() {
    accepts("val x : int = if true then 1 else 2");
}

#[test]
fn let_bindings_are_visible_in_the_body_but_not_after() {
    accepts("val x = let val y = 1 in y + 1 end");
    rejects(
        "val x = (let val y = 1 in y end) + y",
        "Unbound identifier: y",
    );
}

#[test]
fn a_let_binding_can_shadow_an_outer_one_with_a_different_type() {
    accepts("val x = true\nval y = (let val x = 1 in x + 1 end)");
}

// ---------------------------------------------------------------------------
// Tuples and patterns
// ---------------------------------------------------------------------------

#[test]
fn infers_a_tuple_as_a_product_type() {
    accepts("val x = (1, true, 2 + 3)");
    accepts("val x : int * bool * int = (1, true, 2 + 3)");
}

#[test]
fn rejects_a_tuple_arity_mismatch_against_its_annotation() {
    rejects(
        "val x : int * bool = (1, true, 2)",
        "Expected Product([Int, Bool]) but got type Product([Int, Bool, Int])",
    );
}

#[test]
fn a_tuple_pattern_binds_each_component_its_own_type() {
    accepts("val (x, y) = (1, true)\nval z = y");
    rejects(
        "val (x, y) = (1, true)\nval z = y + 1",
        "Expected Int but got type Bool",
    );
}

#[test]
fn a_wildcard_matches_anything_and_binds_nothing() {
    accepts("val _ = true\nval x = 1");
    // `_` is a pattern form only — it's not an expression, so there's nothing a
    // later decl could refer to it by.
    assert!(stepper_core::parse_program("val _ = true\nval x = _ + 1").is_err());
}

#[test]
fn rejects_a_tuple_pattern_against_a_non_tuple_type() {
    rejects(
        "val (x, y) = 5",
        "Pattern (x, y) expects a tuple type but got Int",
    );
}

#[test]
fn rejects_a_tuple_pattern_arity_mismatch() {
    rejects(
        "val (x, y) = (1, 2, 3)",
        "Tuple of pattern and expected type do not match",
    );
}

#[test]
fn rejects_a_non_linear_pattern() {
    rejects(
        "val (x, x) = (1, 2)",
        "Variable x is bound more than once in this pattern",
    );
}

#[test]
fn checks_an_annotation_on_a_pattern_component() {
    accepts("val (x : int, y) = (1, true)");
    rejects("val (x : bool, y) = (1, true)", "Expected Int got Bool");
}

// ---------------------------------------------------------------------------
// case
// ---------------------------------------------------------------------------

#[test]
fn case_requires_every_arm_to_produce_the_same_type() {
    rejects(
        "val x = case 1 of 0 => true | _ => 1",
        "Expected Bool but got type Int",
    );
}

#[test]
fn case_binds_pattern_variables_within_their_own_arm_only() {
    accepts("val x = case (1, 2) of (a, b) => a + b");
    rejects(
        "val x = (case (1, 2) of (a, b) => a + b) + a",
        "Unbound identifier: a",
    );
}

#[test]
fn case_scrutinee_and_patterns_must_agree_in_type() {
    rejects_somehow("val x = case true of 0 => 1 | _ => 2");
    rejects_somehow("val x = case 1 of true => 1 | _ => 2");
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

#[test]
fn a_lambda_infers_an_arrow_type_from_its_parameter_annotation() {
    accepts("val f : int -> int = fn x : int => x + 1");
    accepts("val f : int -> bool = fn x : int => x < 1");
    // The lambda's inferred arrow type is what gets compared against the
    // annotation, so a wrong *result* type is reported as a whole-function
    // mismatch rather than as an error inside the body.
    rejects(
        "val f : int -> bool = fn x : int => x + 1",
        "Expected Arrow(Int, Bool) but got type Arrow(Int, Int)",
    );
}

#[test]
fn a_lambda_needs_an_annotation_on_its_first_parameter() {
    // There's no unification engine to recover the parameter type from how the
    // lambda is later applied, so the first arm has to say it outright.
    rejects(
        "val f = fn x => x + 1",
        "x must have an explict param type for functions on first pattern",
    );
    // Later arms take their type from the first one, so only that one is required.
    accepts("val f = fn n : int => n | 0 => 1");
    rejects(
        "val f = fn 0 => 1 | n : int => n",
        "0 must have an explict param type for functions on first pattern",
    );
}

#[test]
fn a_lambda_parameter_may_be_annotated_component_by_component() {
    // Annotating every component of a tuple pattern says the same thing as
    // annotating the whole pattern, so it satisfies the "first parameter must
    // declare its type" rule too.
    accepts("val f = fn (x : int, y : int) => x + y");
    accepts("val f : int * bool -> int = fn (x : int, b : bool) => x");
    // One component short of a complete type is still no type.
    rejects(
        "val f = fn (x : int, y) => x",
        "(x : int, y) must have an explict param type for functions on first pattern",
    );
}

#[test]
fn every_lambda_arm_must_return_the_same_type() {
    accepts("val f = fn n : int => n | 0 => 1");
    rejects(
        "val f = fn n : int => n | 0 => true",
        "Expected Int but got type Bool",
    );
}

#[test]
fn application_checks_the_argument_against_the_parameter_type() {
    accepts("val f : int -> int = fn x : int => x + 1 val y = f 2");
    rejects(
        "val f : int -> int = fn x : int => x + 1 val y = f true",
        "Expected Int but got type Bool",
    );
}

#[test]
fn rejects_applying_a_non_function() {
    rejects("val x = 1 val y = x 2", "Applying a non-function: `x` has type Int");
}

#[test]
fn rejects_negating_a_function() {
    // `~ f x` parses as `(~f) x`, so it fails the same way SML's does.
    rejects(
        "val f : int -> int = fn x : int => x val z = ~ f 1",
        "Expected Int but got type Arrow(Int, Int)",
    );
}

#[test]
fn a_curried_function_applies_one_argument_at_a_time() {
    accepts(
        "val add : int -> int -> int = fn a : int => fn b : int => a + b\nval s : int = add 1 2",
    );
    // Partially applied, it's still a function.
    accepts(
        "val add : int -> int -> int = fn a : int => fn b : int => a + b\nval inc : int -> int = add 1",
    );
}

// ---------------------------------------------------------------------------
// val rec
// ---------------------------------------------------------------------------

#[test]
fn val_rec_sees_its_own_name_in_its_body() {
    // The whole point of `rec`: the name is in scope on the right-hand side, which
    // a plain `val` doesn't give you.
    accepts("val rec f : int -> int = fn n : int => if n = 0 then 1 else n * f (n - 1)");
    rejects(
        "val f : int -> int = fn n : int => if n = 0 then 1 else n * f (n - 1)",
        "Unbound identifier: f",
    );
}

#[test]
fn val_rec_requires_an_annotated_identifier_bound_to_a_lambda() {
    // Without inference, the declared type is the only thing that can type the
    // recursive occurrences inside the body — hence the four restrictions.
    rejects(
        "val rec f = fn n : int => n",
        "val rec requires an explict type annoation",
    );
    rejects(
        "val rec (f, g) : int -> int = fn n : int => n",
        "must use single identifier for val rec",
    );
    rejects("val rec f : int = 5", "val rec requires a function type");
    rejects(
        "val rec f : int -> int = 5",
        "val rec must have a fn expression on rhs",
    );
}

#[test]
fn val_rec_checks_its_body_against_the_declared_type() {
    rejects(
        "val rec f : int -> bool = fn n : int => n",
        "Expected Arrow(Int, Bool) but got type Arrow(Int, Int)",
    );
}

// ---------------------------------------------------------------------------
// `fun` declarations
// ---------------------------------------------------------------------------
//
// A `fun` is a `val`/`val rec` by the time the typechecker sees it (see
// `frontend::elaborate`), so these are really about the types elaboration hands
// it — and about the errors that only make sense in terms of what the user wrote.

#[test]
fn checks_a_fun_like_the_val_it_elaborates_to() {
    accepts("fun f (x : int) = x + 1\nval y = f 2");
    accepts("fun f (x : int) : bool = x < 1\nval b = f 2");
    rejects(
        "fun f (x : int) : bool = x + 1",
        "Expected Arrow(Int, Bool) but got type Arrow(Int, Int)",
    );
    rejects(
        "fun f (x : int) = x + 1\nval y = f true",
        "Expected Int but got type Bool",
    );
}

#[test]
fn every_clause_of_a_fun_is_checked_against_the_first_clauses_types() {
    accepts("fun fib (0 : int) = 0 | fib 1 = 1 | fib (n : int) : int = fib (n - 1) + fib (n - 2)");
    // A later clause annotating a parameter differently contradicts the type the
    // function already has.
    rejects("fun f (x : int) = 0 | f (b : bool) = 1", "Expected Int got Bool");
    // Every clause has to return the same type, exactly as every `fn` arm does.
    rejects("fun f (x : int) = 0 | f 1 = true", "Expected Int but got type Bool");
}

#[test]
fn a_fun_may_take_a_tuple_apart_in_its_parameter() {
    // Annotating the components is annotating the parameter, so a tuple-taking
    // `fun` needs nothing further — the same rule
    // `a_lambda_parameter_may_be_annotated_component_by_component` shows for `fn`.
    accepts("fun add (x : int, y : int) = x + y\nval s = add (1, 2)");
    accepts(
        "fun sum (0 : int, acc : int) : int = acc | sum (n : int, acc : int) = sum (n - 1, acc + n)",
    );
}

#[test]
fn a_curried_fun_is_a_function_returning_a_function() {
    accepts("fun add (x : int) (y : int) = x + y\nval inc : int -> int = add 1");
    rejects(
        "fun add (x : int) (y : int) : int = x + y\nval n : int = add 1",
        "Expected Int but got type Arrow(Int, Int)",
    );
}
