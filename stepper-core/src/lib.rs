use std::cell::RefCell;

use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
pub mod dom;
mod frontend;
pub mod pretty;
pub mod stepping;
pub mod view;

pub use frontend::ast;
pub use frontend::parse_program;
pub use frontend::typecheck::typecheck;
use view::{HighlightColor, ViewState};

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

/// The one program the UI has loaded, and how it's currently being displayed.
///
/// These are deliberately two separate things. `program` is pure semantics —
/// stepping it never consults `view`, and it holds no highlight or fold state to
/// be cleaned up between steps. `view` is pure display — changing it (moving a
/// highlight, folding a lambda) never touches `program`. The only coupling is
/// that both talk about the same `NodeId`s.
struct Session {
    program: Option<ast::Program>,
    view: ViewState,
}

thread_local! {
    static SESSION: RefCell<Session> = RefCell::new(Session {
        program: None,
        view: ViewState::default(),
    });
}

/// Points the yellow highlight at whatever `step` would do next. Called after
/// every change to the program, so the display always shows what Step is *about
/// to* do rather than what it just did.
fn refresh_next_redex(session: &mut Session) {
    let next = session
        .program
        .as_ref()
        .and_then(stepping::highlight_next);
    session
        .view
        .set_highlights(HighlightColor::Yellow, next);
}

/// Parse `formula`, store it as the current program, and return an error message
/// if it doesn't parse or typecheck (empty string on success). Call
/// `render_program` afterwards for the display.
#[wasm_bindgen]
pub fn enter_formula(formula: &str) -> String {
    // Recursive functions parked by the previous program are dead now.
    stepping::reset_rec_env();
    SESSION.with(|s| {
        let mut session = s.borrow_mut();
        // A new program means new nodes, so nothing the old view state named
        // still exists — start its display from scratch, keeping only the width,
        // which is a user preference rather than a fact about the program.
        let width = session.view.width;
        session.view = ViewState {
            width,
            ..ViewState::default()
        };

        match parse_program(formula) {
            Ok(program) => {
                if let Some(e) = typecheck(&program) {
                    session.program = None;
                    return format!("Type error:\n{e}");
                }
                session.program = Some(program);
                refresh_next_redex(&mut session);
                String::new()
            }
            Err(e) => {
                session.program = None;
                format!("Parse error:\n{e}")
            }
        }
    })
}

/// Advance the stored program by one reduction step. Returns a description of what
/// happened; call `render_program` afterwards to get the updated display.
#[wasm_bindgen]
pub fn step_formula() -> String {
    SESSION.with(|s| {
        let mut session = s.borrow_mut();
        let Some(current) = session.program.as_ref() else {
            return "Enter a formula first.".to_string();
        };
        match stepping::step(current) {
            Some(outcome) => {
                session.program = Some(outcome.program);
                // Green marks what this step just placed, replacing whatever the
                // previous step marked. Nothing in the tree had to be un-marked
                // first — this *is* the un-marking.
                session
                    .view
                    .set_highlights(HighlightColor::Green, outcome.green);
                refresh_next_redex(&mut session);
                outcome.message
            }
            None => "No more steps.".to_string(),
        }
    })
}

/// Folds or unfolds the lambda `node_id`, and reports whether it's now folded.
///
/// Ids repeat across copies of a subtree (see `ast::NodeId`), so folding a
/// recursive function folds every unrolling of it at once — which is the useful
/// behavior while watching one unroll.
#[wasm_bindgen]
pub fn toggle_collapse(node_id: u32) -> bool {
    SESSION.with(|s| s.borrow_mut().view.toggle_collapsed(ast::NodeId(node_id)))
}

/// The node ids of every lambda in the current program, outermost-first.
///
/// The browser doesn't need this to handle a click — each foldable span carries
/// its own id in `data-collapse` — but a "fold everything" control would, and it
/// gives callers with no DOM (the tests) a way to name a lambda.
#[wasm_bindgen]
pub fn foldable_ids() -> Vec<u32> {
    with_session(|program, _| {
        program
            .map(|p| ast::lambda_ids(p).iter().map(|id| id.0).collect())
            .unwrap_or_default()
    })
}

/// Sets the column width the display wraps at.
#[wasm_bindgen]
pub fn set_width(width: usize) {
    SESSION.with(|s| s.borrow_mut().view.width = width.max(1));
}

/// The current program's rendered text, highlights shown as markers. Kept for
/// non-DOM callers and for debugging; the browser uses `dom::render_program`.
#[wasm_bindgen]
pub fn current_render() -> String {
    SESSION.with(|s| {
        let session = s.borrow();
        match session.program.as_ref() {
            Some(program) => pretty::pretty_print_marked(program, &session.view),
            None => String::new(),
        }
    })
}

/// Runs `f` on the current program and view — how the DOM backend gets at them
/// without `SESSION` having to be public.
pub(crate) fn with_session<T>(f: impl FnOnce(Option<&ast::Program>, &ViewState) -> T) -> T {
    SESSION.with(|s| {
        let session = s.borrow();
        f(session.program.as_ref(), &session.view)
    })
}
