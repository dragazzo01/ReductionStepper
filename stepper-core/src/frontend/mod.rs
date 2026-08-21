use std::cell::RefCell;

use lrlex::lrlex_mod;
use lrpar::lrpar_mod;
pub mod ast;
mod elaborate;
pub mod typecheck;

use ast::Program;

lrlex_mod!("frontend/grammar.l");
lrpar_mod!("frontend/grammar.y");

/// Parses `input` into a `Program`. Purely syntactic — a result may still be
/// ill-typed; callers that care (see `crate::enter_formula`) run
/// `typecheck::typecheck` separately afterwards.
pub fn parse_program(input: &str) -> Result<Program, String> {
    let lexerdef = grammar_l::lexerdef();
    let lexer = lexerdef.lexer(input);
    // `fun` declarations are elaborated into `val`/`val rec` by the grammar's own
    // actions (see `elaborate`), and an action has no way to fail the parse — so a
    // malformed `fun` binding reports itself through this side channel (the
    // grammar's `%parse-param`) and is checked here, before the placeholder decl it
    // left behind can escape.
    let errors = RefCell::new(Vec::new());
    let (res, errs) = grammar_y::parse(&lexer, &errors);
    if !errs.is_empty() {
        let msg = errs
            .iter()
            .map(|e| e.pp(&lexer, &grammar_y::token_epp))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(msg);
    }
    let errors = errors.into_inner();
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    Ok(res.unwrap())
}
