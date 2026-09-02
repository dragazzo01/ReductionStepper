//! Laid-out [`Item`]s to text.
//!
//! Highlights are resolved here rather than being built into the document,
//! because they can't affect layout: an `Ann::Node` region is painted by looking
//! its id up in the `ViewState` at the moment it's rendered. The DOM backend does
//! the same thing with CSS classes instead of markers.

use crate::view::{HighlightColor, ViewState};

use super::doc::{Ann, Item};

/// Plain text, with every annotation dropped.
pub fn render_string(items: &[Item]) -> String {
    render(items, |_| None)
}

/// Text with each highlighted region wrapped in readable markers — `[y..y]`
/// yellow, `[g..g]` green, `[r..r]` red. Square brackets never occur in program
/// text, so this stays unambiguous, and it's what the tests assert against.
pub fn render_marked(items: &[Item], view: &ViewState) -> String {
    render(items, |id| view.highlight_of(id).map(marker_pair))
}

fn marker_pair(color: HighlightColor) -> (&'static str, &'static str) {
    match color {
        HighlightColor::Yellow => ("[y", "y]"),
        HighlightColor::Green => ("[g", "g]"),
        HighlightColor::Red => ("[r", "r]"),
    }
}

/// Walks `items`, asking `wrap` what (if anything) should bracket each node
/// region. The stack mirrors the balanced `Open`/`Close` pairs `layout` emits, so
/// a region's closing marker lands exactly where its content ends.
fn render(
    items: &[Item],
    wrap: impl Fn(crate::ast::NodeId) -> Option<(&'static str, &'static str)>,
) -> String {
    let mut out = String::new();
    let mut closers: Vec<Option<&'static str>> = Vec::new();
    for item in items {
        match item {
            Item::Text(s) => out.push_str(s),
            Item::Break(indent) => {
                out.push('\n');
                out.extend(std::iter::repeat_n(' ', *indent));
            }
            Item::Open(Ann::Node(id)) => match wrap(*id) {
                Some((open, close)) => {
                    out.push_str(open);
                    closers.push(Some(close));
                }
                None => closers.push(None),
            },
            Item::Open(Ann::Class(_)) => closers.push(None),
            Item::Close => {
                if let Some(Some(close)) = closers.pop() {
                    out.push_str(close);
                }
            }
        }
    }
    out
}
