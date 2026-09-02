//! Building the program's DOM directly, instead of handing JavaScript a string
//! to pick apart.
//!
//! `render_program` returns a real `<pre>` element, so `www/index.html` is a
//! `output.replaceChildren(render_program())` away from being done. Everything
//! about *how* the program looks — where lines break, what's clickable, which
//! spans get which classes — is decided here in Rust.
//!
//! Every region carries its `NodeId` in `data-node`, which is what makes
//! highlighting separable from layout: a highlight can't change any node's width,
//! so moving one is a class swap on spans that already exist, with nothing
//! measured or rebuilt. The buttons here don't exercise that — each of them
//! changes the *program*, and a changed program needs a real re-render anyway —
//! but a control that only moves the marker (stepping backwards through a
//! recorded history, say, or highlighting every use of a hovered variable) would
//! need no more than those attributes and a pass over them.
//!
//! Folding a lambda is the opposite case: it changes the node's width and
//! therefore the line-breaking of everything around it, so
//! [`toggle_collapse`](crate::toggle_collapse) is followed by a fresh
//! `render_program`.
//!
//! Clicks are handled by delegation rather than a listener per node: every
//! foldable region carries a `data-collapse` attribute holding its `NodeId`, and
//! `index.html` puts one listener on the container. That keeps this module free
//! of `Closure` lifetime management entirely — nothing here has to outlive the
//! call that created it.

use wasm_bindgen::prelude::wasm_bindgen;
use web_sys::{Document, Element};

use std::collections::HashSet;

use crate::ast::{self, NodeId};
use crate::pretty::{self, Ann, Item};
use crate::view::ViewState;

/// The attribute a foldable region carries, holding the `NodeId` to send back to
/// [`crate::toggle_collapse`]. `index.html` reads this in its click handler.
const COLLAPSE_ATTR: &str = "data-collapse";

/// The attribute every node region carries, so highlights can be repainted by
/// lookup rather than by rebuilding.
const NODE_ATTR: &str = "data-node";

fn document() -> Document {
    web_sys::window()
        .expect("no window: this runs in a browser")
        .document()
        .expect("window has no document")
}

/// The current program as a `<pre>` element, laid out and styled.
#[wasm_bindgen]
pub fn render_program() -> Element {
    crate::with_session(|program, view| {
        let doc = document();
        let root = doc
            .create_element("pre")
            .expect("`pre` is a valid tag name");
        root.set_class_name("program");
        let Some(program) = program else {
            return root;
        };
        // Gathered once, rather than asked per node: `Ann::Node` deliberately
        // carries nothing but an id (the layout has no reason to know what kind
        // of node it came from), so without this the "is this foldable?" question
        // would be a fresh tree walk for every span on screen.
        let foldable: HashSet<NodeId> = ast::lambda_ids(program).into_iter().collect();
        build_into(&doc, &root, &pretty::layout(program, view), &foldable, view);
        root
    })
}

/// Walks the laid-out items, opening a `<span>` for each annotated region and
/// closing it again at its matching `Close`.
///
/// `stack` holds the elements currently open; text and line breaks always land in
/// whatever is on top, which is what makes nested annotations come out as nested
/// elements.
fn build_into(
    doc: &Document,
    root: &Element,
    items: &[Item],
    foldable: &HashSet<NodeId>,
    view: &ViewState,
) {
    let mut stack: Vec<Element> = vec![root.clone()];
    for item in items {
        let top = stack
            .last()
            .expect("stack is never empty: `root` is at the bottom")
            .clone();
        match item {
            Item::Text(s) => append_text(doc, &top, s),
            Item::Break(indent) => {
                append_text(doc, &top, "\n");
                append_text(doc, &top, &" ".repeat(*indent));
            }
            Item::Open(ann) => {
                let span = doc
                    .create_element("span")
                    .expect("`span` is a valid tag name");
                match ann {
                    Ann::Class(class) => span.set_class_name(class),
                    Ann::Node(id) => decorate_node(&span, *id, foldable, view),
                }
                top.append_child(&span).expect("span is a fresh node");
                stack.push(span);
            }
            Item::Close => {
                stack.pop();
            }
        }
    }
}

/// Tags a node's span with its id, its highlight (if any) and — for a lambda —
/// the attribute that makes it foldable.
fn decorate_node(span: &Element, id: NodeId, foldable: &HashSet<NodeId>, view: &ViewState) {
    span.set_attribute(NODE_ATTR, &id.0.to_string())
        .expect("data-node is a valid attribute name");

    let mut classes = vec!["node".to_string()];
    if let Some(color) = view.highlight_of(id) {
        classes.push(color.css_class().to_string());
    }
    if foldable.contains(&id) {
        classes.push("foldable".to_string());
        if view.is_collapsed(id) {
            classes.push("folded".to_string());
        }
        span.set_attribute(COLLAPSE_ATTR, &id.0.to_string())
            .expect("data-collapse is a valid attribute name");
    }
    span.set_class_name(&classes.join(" "));
}

fn append_text(doc: &Document, parent: &Element, text: &str) {
    let node = doc.create_text_node(text);
    parent
        .append_child(&node)
        .expect("text node is a fresh node");
}
