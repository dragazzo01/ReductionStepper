use std::cell::RefCell;

use wasm_bindgen::prelude::*;

mod frontend;
pub mod pretty;
pub mod stepping;

pub use frontend::ast;
pub use frontend::parse_program;
pub use frontend::typecheck::typecheck;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

#[cfg(target_arch = "wasm32")]
#[allow(unused_macros)]
macro_rules! console_log {
    ($($t:tt)*) => (log(&format!($($t)*)))
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(unused_macros)]
macro_rules! console_log {
    ($($t:tt)*) => {println!($($t)*)};
}

#[wasm_bindgen(start)]
fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

thread_local! {
    // Always holds a clean, highlight-free program. Highlighting is computed fresh
    // at render time (see `render` below), never stored here.
    static PROGRAM: RefCell<Option<ast::Program>> = const { RefCell::new(None) };
}

/// Render `program` with the *next* reduction step's target highlighted, so the
/// display always shows what Step is about to do rather than what it just did.
fn render(program: &ast::Program) -> String {
    match stepping::highlight_next(program) {
        Some(highlighted) => pretty::pretty_print(&highlighted),
        None => pretty::pretty_print(program),
    }
}

/// Parse `formula`, store it as the current program, and return its rendered text
/// (or a parse-error message, leaving the stored program unset).
#[wasm_bindgen]
pub fn enter_formula(formula: &str) -> String {
    // Recursive functions parked by the previous program are dead now, and keeping
    // them would only push this program's `val rec` names into renames.
    stepping::reset_rec_env();
    match parse_program(formula) {
        Ok(program) => {
            if let Some(e) = typecheck(&program) {
                PROGRAM.with(|p| *p.borrow_mut() = None);
                return format!("Type error:\n{e}");
            }
            let rendered = render(&program);
            PROGRAM.with(|p| *p.borrow_mut() = Some(program));
            rendered
        }
        Err(e) => {
            PROGRAM.with(|p| *p.borrow_mut() = None);
            format!("Parse error:\n{e}")
        }
    }
}

/// Advance the stored program by one reduction step. Returns a description of what
/// happened; call `current_render` afterwards to get the updated, highlighted text.
#[wasm_bindgen]
pub fn step_formula() -> String {
    PROGRAM.with(|p| {
        let mut program = p.borrow_mut();
        match program.as_ref() {
            None => "Enter a formula first.".to_string(),
            Some(current) => match stepping::step(current) {
                Some(outcome) => {
                    *program = Some(outcome.program);
                    outcome.message
                }
                None => "No more steps.".to_string(),
            },
        }
    })
}

/// The current stored program's rendered text (empty if none is set), with the next
/// step's target wrapped in `pretty::HIGHLIGHT_START`/`HIGHLIGHT_END`.
#[wasm_bindgen]
pub fn current_render() -> String {
    PROGRAM.with(|p| match p.borrow().as_ref() {
        Some(program) => render(program),
        None => String::new(),
    })
}
