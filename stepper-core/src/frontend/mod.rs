pub mod ast;
mod elaborate;
mod lower;
mod resolve;
pub mod typecheck;

use ast::Program;

/// Parses `input` into a `Program` and resolves its variables to their binding
/// sites. Purely syntactic — a result may still be ill-typed; callers that care
/// (see `crate::enter_formula`) run `typecheck::typecheck` separately afterwards.
///
/// Resolution is part of parsing rather than a step callers take themselves
/// because an unresolved tree is a trap: `BinderId`s would still be distinct per
/// occurrence, so substitution would silently match nothing. Nothing outside this
/// module ever holds a `Program` that hasn't been through `resolve`.
///
/// The three stages are millet's lexer, millet's parser, and [`lower`], and they
/// fail for different reasons: the first two reject what isn't Standard ML at all,
/// and the third rejects the SML this crate hasn't implemented yet. Only the first
/// error of a stage is reported — millet's parser recovers and carries on, so one
/// mistake can produce a cascade, and the first of them is the honest one.
pub fn parse_program(input: &str) -> Result<Program, String> {
    let lexed = sml_lex::get(input, false);
    if let Some(error) = lexed.errors.first() {
        return Err(describe(input, error.range().start().into(), &error.to_string()));
    }

    // A mutable copy of the standard basis' fixities, because an SML program may
    // extend it: `infix 6 ++` changes how the *rest* of the file parses. `lower`
    // rejects the declaration itself, having nowhere to put it, but by then the
    // parser has already done the part no fixed grammar could.
    let mut fixity = sml_fixity::STD_BASIS.clone();
    let parsed = sml_parse::get(&lexed.tokens, &mut fixity);
    if let Some(error) = parsed.errors.first() {
        return Err(describe(input, error.range().start().into(), &error.to_string()));
    }

    let mut program = lower::lower(&parsed.root)
        .map_err(|error| describe(input, error.start, &error.message))?;
    resolve::resolve(&mut program);
    Ok(program)
}

/// Millet reports positions as byte offsets; a person reads line and column.
fn describe(input: &str, offset: usize, message: &str) -> String {
    let before = &input[..offset.min(input.len())];
    let line = before.matches('\n').count() + 1;
    let column = before.len() - before.rfind('\n').map_or(0, |i| i + 1) + 1;
    format!("{message}, at line {line} column {column}")
}
