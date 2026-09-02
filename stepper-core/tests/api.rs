//! The `wasm_bindgen` boundary (`lib.rs`): the functions `www/index.html`
//! actually calls, and the session state they share. Everything here is a native
//! call — the same functions the WASM build exports, minus `dom::render_program`,
//! which needs a browser.
//!
//! `enter_formula` reports *problems* rather than returning a rendering: the
//! browser gets its display from `dom::render_program`, which builds the DOM
//! itself, so there's no longer a string for the boundary to hand back. An empty
//! return means the program was accepted.

mod common;

use stepper_core::{
    current_render, enter_formula, foldable_ids, set_width, step_formula, toggle_collapse,
};

#[test]
fn entering_a_formula_shows_what_step_will_do() {
    assert_eq!(enter_formula("val x = 15 * 10"), "");
    assert_eq!(current_render(), "val x = [y15 * 10y]");
    assert_eq!(step_formula(), "Evaluated 15 * 10 to 150");
    // Fully reduced: nothing left to highlight.
    assert_eq!(current_render(), "val x = 150");
    assert_eq!(step_formula(), "No more steps.");
}

#[test]
fn a_step_marks_what_it_placed_and_unmarks_what_the_last_one_did() {
    enter_formula("val x = 5\nval y = x + x");
    step_formula();
    // The substitution's copies are green, and the next redex is yellow around
    // them.
    assert_eq!(current_render(), "val y = [y[g5g] + [g5g]y]");
    step_formula();
    // Nothing had to clear the green: the next step simply named a different set
    // (here, none at all).
    assert_eq!(current_render(), "val y = 10");
}

#[test]
fn folding_a_lambda_changes_the_display_and_not_the_program() {
    enter_formula("val f = fn x : int => x + 1\nval y = f 2");
    assert!(current_render().contains("fn x : int => x + 1"));

    // The browser reads this id off the clicked element's `data-collapse`; with
    // no DOM here it comes from `foldable_ids` instead.
    let [id] = foldable_ids()[..] else {
        panic!("expected exactly one lambda");
    };
    assert!(toggle_collapse(id), "first toggle folds");
    assert!(current_render().contains("fn x => ..."), "folded");

    // The fold is display-only: the program underneath is untouched, so the step
    // reports the whole lambda and the result is the same either way.
    assert_eq!(step_formula(), "Substituted f = fn x : int => x + 1");
    assert!(!toggle_collapse(id), "second toggle unfolds");
    assert_eq!(step_formula(), "Substituted x : int = 2");
}

#[test]
fn setting_the_width_changes_where_lines_break() {
    let src = "val x = let val a = 1 val b = 2 in a + b + a + b end";
    enter_formula(src);
    set_width(stepper_core::view::UNLIMITED_WIDTH);
    assert!(!current_render().contains('\n'), "unlimited width: one line");

    set_width(20);
    assert!(current_render().contains('\n'), "narrow width: broken");

    // Width is a preference about the display, so it outlives the program.
    enter_formula("val y = let val a = 1 val b = 2 in a + b + a + b end");
    assert!(current_render().contains('\n'), "width survived re-entry");
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
    assert_eq!(current_render(), "val x = 2");
    assert_eq!(
        messages.first().map(String::as_str),
        Some("Bound recursive fact = fn n : int => if n = 0 then 1 else n * fact (n - 1)")
    );
}
