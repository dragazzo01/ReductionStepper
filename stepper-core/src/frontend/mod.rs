use lrlex::lrlex_mod;
use lrpar::lrpar_mod;
pub mod ast;
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
    let (res, errs) = grammar_y::parse(&lexer);
    if !errs.is_empty() {
        let msg = errs
            .iter()
            .map(|e| e.pp(&lexer, &grammar_y::token_epp))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(msg);
    }
    Ok(res.unwrap())
}
