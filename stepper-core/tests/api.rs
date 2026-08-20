//! The `wasm_bindgen` boundary (`lib.rs`): the three functions `www/index.html`
//! actually calls, and the stored-program state they share. Everything here is a
//! native call — the same functions the WASM build exports.

mod common;

use common::*;
use stepper_core::{current_render, enter_formula, step_formula};

#[test]
fn entering_a_formula_shows_what_step_will_do() {
    assert_eq!(markers(&enter_formula("val x = 15 * 10")), "val x = [y15 * 10y]");
    assert_eq!(step_formula(), "Evaluated 15 * 10 to 150");
    // Fully reduced: nothing left to highlight.
    assert_eq!(markers(&current_render()), "val x = 150");
    assert_eq!(step_formula(), "No more steps.");
}

#[test]
fn current_render_repeats_the_stored_program_without_advancing_it() {
    enter_formula("val x = 1 + 2 + 3");
    let first = current_render();
    assert_eq!(current_render(), first);
    step_formula();
    assert_ne!(current_render(), first);
}

#[test]
fn reports_parse_errors_and_forgets_the_program() {
    assert!(enter_formula("val = 1").starts_with("Parse error:"));
    // Nothing was stored, so there's nothing to step or render.
    assert_eq!(current_render(), "");
    assert_eq!(step_formula(), "Enter a formula first.");
}

#[test]
fn reports_type_errors_and_forgets_the_program() {
    assert_eq!(
        enter_formula("val x : bool = 1 + 2"),
        "Type error:\nExpected Bool but got type Int"
    );
    assert_eq!(current_render(), "");
    assert_eq!(step_formula(), "Enter a formula first.");
}

#[test]
fn a_bad_program_does_not_disturb_a_good_one_already_entered() {
    enter_formula("val x = 1 + 2");
    enter_formula("val = 1");
    // `enter_formula` clears the stored program on failure rather than leaving the
    // previous one behind for Step to keep advancing.
    assert_eq!(current_render(), "");
}

#[test]
fn stepping_before_entering_anything_says_so() {
    // A fresh thread starts with no stored program (it's thread-local state).
    std::thread::spawn(|| {
        assert_eq!(step_formula(), "Enter a formula first.");
        assert_eq!(current_render(), "");
    })
    .join()
    .unwrap();
}

#[test]
fn drives_a_recursive_program_to_completion() {
    let mut messages = Vec::new();
    enter_formula(
        "val rec fact : int -> int = fn n : int => if n = 0 then 1 else n * fact (n - 1)\nval x = fact 2",
    );
    loop {
        let message = step_formula();
        if message == "No more steps." {
            break;
        }
        messages.push(message);
        assert!(messages.len() < 100, "not terminating: {messages:?}");
    }
    assert_eq!(markers(&current_render()), "val x = 2");
    assert_eq!(
        messages.first().map(String::as_str),
        Some("Bound recursive fact = fn n : int => if n = 0 then 1 else n * fact (n - 1)")
    );
}
