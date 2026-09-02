//! The sample programs the frontend offers in its dropdown (`www/examples.txt`).
//!
//! They're the first thing anyone runs, so they're worth holding to the same
//! standard as the test suite: every one must parse, typecheck, and reduce to a
//! value in a sane number of steps. Compiled in with `include_str!`, so editing
//! the file the page reads is what runs these — the two can't drift apart.

mod common;

use stepper_core::{parse_program, stepping, typecheck};

const EXAMPLES: &str = include_str!("../../www/examples.txt");

/// Generous next to `common`'s limit of 200: these are whole programs rather
/// than single reductions, and `fib 5` alone is a few hundred steps. Still low
/// enough to catch an example that doesn't terminate.
const STEP_LIMIT: usize = 20_000;

/// Splits the file into `(name, source)` pairs — the same rule `index.html`
/// applies: a `#` line names the example that follows it.
fn examples() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in EXAMPLES.lines() {
        match line.strip_prefix('#') {
            Some(name) => out.push((name.trim().to_string(), String::new())),
            None => {
                if let Some((_, source)) = out.last_mut() {
                    source.push_str(line);
                    source.push('\n');
                }
            }
        }
    }
    // The leading comment block is a run of `#` lines, so it leaves behind
    // entries with nothing under them. Real examples always have a program.
    out.into_iter()
        .filter(|(_, source)| !source.trim().is_empty())
        .collect()
}

#[test]
fn the_file_holds_the_examples_the_dropdown_expects() {
    let examples = examples();
    assert!(
        examples.len() >= 8,
        "expected the dropdown to have something to offer, got {}",
        examples.len()
    );
    for (name, _) in &examples {
        assert!(!name.is_empty(), "every example needs a name for the dropdown");
    }
}

#[test]
fn every_example_parses_typechecks_and_runs_to_a_value() {
    for (name, source) in examples() {
        stepping::reset_rec_env();

        let program = parse_program(&source)
            .unwrap_or_else(|e| panic!("example `{name}` does not parse:\n{source}\n{e}"));
        assert_eq!(
            typecheck(&program),
            None,
            "example `{name}` does not typecheck:\n{source}"
        );

        let mut program = program;
        let mut steps = 0;
        loop {
            // The same agreement `common::run_to_value` checks: whatever the
            // display says is next must be what Step actually does.
            let highlighted = stepping::highlight_next(&program);
            match stepping::step(&program) {
                Some(outcome) => {
                    assert!(
                        highlighted.is_some(),
                        "example `{name}`: step found a redex but highlight_next didn't"
                    );
                    program = outcome.program;
                }
                None => {
                    assert!(
                        highlighted.is_none(),
                        "example `{name}`: highlight_next found a redex but step didn't"
                    );
                    break;
                }
            }
            steps += 1;
            assert!(steps < STEP_LIMIT, "example `{name}` does not terminate");
        }
    }
}
