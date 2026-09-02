//! Display state, held *beside* the program rather than inside it.
//!
//! Everything here answers a question about how the program should look right
//! now, never about what it means. Keeping it out of the AST is what lets
//! highlights move and lambdas fold without a single tree rewrite — and, going
//! the other way, what lets a reduction happen without disturbing the display
//! state that should outlive it.
//!
//! The two kinds of state behave differently on purpose:
//!
//! - **Highlights** don't change any node's width, so moving one never changes
//!   where lines break. The renderer can repaint them by toggling a class on
//!   elements it already built (see `crate::dom`), no relayout needed.
//! - **Collapse** does change a node's width, and therefore the line-breaking of
//!   every group enclosing it, so it's read while the `Doc` is *built* and a
//!   toggle means a rebuild. Cheap at the size of program this tool displays.

use std::collections::{HashMap, HashSet};

use crate::ast::NodeId;

/// Why a node is marked, and which color it renders as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HighlightColor {
    /// The redex `step` would act on next.
    Yellow,
    /// Text the last step just placed here (a substituted value, an unrolled
    /// recursive function).
    Green,
    /// Reserved for a future exception indicator — nothing sets this yet.
    Red,
}

impl HighlightColor {
    /// The CSS class the DOM renderer tags this color's `<span>` with, and the
    /// name `www/index.html`'s stylesheet expects.
    pub fn css_class(self) -> &'static str {
        match self {
            HighlightColor::Yellow => "highlight-yellow",
            HighlightColor::Green => "highlight-green",
            HighlightColor::Red => "highlight-red",
        }
    }
}

/// The width the pretty-printer lays out to when nothing says otherwise. Wide
/// enough that short programs stay on one line, narrow enough that a nested
/// `let` or a multi-clause `fn` actually breaks.
pub const DEFAULT_WIDTH: usize = 60;

/// A width that no line can exceed, i.e. "never break a group". Laying out at
/// this width reproduces the single-line output the printer produced before it
/// knew how to break at all, which is what keeps the parse/print round-trip
/// tests meaningful.
pub const UNLIMITED_WIDTH: usize = usize::MAX;

#[derive(Debug, Clone)]
pub struct ViewState {
    /// Lambdas the user has folded to `fn <pat> => ...`.
    pub collapsed: HashSet<NodeId>,
    /// What to paint, and in what color.
    pub highlights: HashMap<NodeId, HighlightColor>,
    /// Column the layout tries to keep lines within.
    pub width: usize,
}

impl Default for ViewState {
    fn default() -> Self {
        ViewState {
            collapsed: HashSet::new(),
            highlights: HashMap::new(),
            width: DEFAULT_WIDTH,
        }
    }
}

impl ViewState {
    /// A view that never breaks a line and shows nothing highlighted or folded —
    /// the plain single-line rendering of a program.
    pub fn plain() -> Self {
        ViewState {
            width: UNLIMITED_WIDTH,
            ..ViewState::default()
        }
    }

    pub fn is_collapsed(&self, id: NodeId) -> bool {
        self.collapsed.contains(&id)
    }

    /// Folds or unfolds the lambda `id`, returning its new state.
    pub fn toggle_collapsed(&mut self, id: NodeId) -> bool {
        if !self.collapsed.remove(&id) {
            self.collapsed.insert(id);
            return true;
        }
        false
    }

    pub fn highlight_of(&self, id: NodeId) -> Option<HighlightColor> {
        self.highlights.get(&id).copied()
    }

    /// Replaces every highlight of `color` with the given ids, leaving the other
    /// colors alone. Each color has exactly one producer — yellow is
    /// `highlight_next`, green is whatever the last step placed — so each can be
    /// refreshed without the others needing to know.
    pub fn set_highlights(&mut self, color: HighlightColor, ids: impl IntoIterator<Item = NodeId>) {
        self.highlights.retain(|_, c| *c != color);
        for id in ids {
            self.highlights.insert(id, color);
        }
    }
}
