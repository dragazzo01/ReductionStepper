//! A layout tree and the algorithm that turns it into positioned output.
//!
//! This is Wadler's pretty-printer (in Lindig's strict formulation): a document
//! is built out of text, line breaks, indentation and *groups*, and laying it out
//! means deciding, group by group, whether the group's contents fit on the rest
//! of the current line. If they do, its `Line`s render as spaces; if they don't,
//! they render as newlines indented to the enclosing `Nest` level.
//!
//! The output is a flat `Vec<Item>` rather than a `String` so that more than one
//! backend can consume it — a string for the tests and for the messages the
//! stepper builds, a DOM tree for the browser. `Ann` markers survive into that
//! output as balanced `Open`/`Close` pairs, which is what lets the DOM backend
//! build nested `<span>`s and remember which node each one came from.
//!
//! Note what is *not* here: highlight colors. Highlighting a node doesn't change
//! its width, so it can't affect any of these decisions, and baking it in would
//! mean rebuilding and relaying out the whole document every time the next redex
//! moves. Instead the `Ann::Node` markers carry ids, and each backend paints from
//! `ViewState` at render time (the DOM one by toggling classes on elements it
//! already built). Collapsing a lambda *does* change its width, so that one is
//! read while the document is built — see `build.rs`.

use crate::ast::NodeId;

/// What a region of the output means, carried through layout to the backends.
#[derive(Debug, Clone, PartialEq)]
pub enum Ann {
    /// This region is the rendering of one AST node. Backends use it to attach
    /// highlights, to make the region clickable, and to map a click back to a
    /// node. Ids repeat when a subtree was copied (see `NodeId`), so a backend's
    /// id-to-region map is one-to-many.
    Node(NodeId),
    /// A syntactic class for styling: `"kw"`, `"lit"`, `"var"`, `"op"`.
    Class(&'static str),
}

#[derive(Debug, Clone)]
pub enum Doc {
    Nil,
    Text(String),
    /// A space when its group is laid out flat, a newline plus indent when broken.
    Line,
    /// Nothing when flat, a newline plus indent when broken.
    SoftBreak,
    /// Always a newline. Forces every group containing it to break.
    HardLine,
    Concat(Vec<Doc>),
    /// Indents the line breaks inside it by `n` further columns.
    Nest(isize, Box<Doc>),
    /// Indents the line breaks inside it to the column where it *starts*, rather
    /// than to the enclosing indent.
    ///
    /// `Nest` alone can't express SML's `let`/`in`/`end` layout, where the three
    /// keywords line up under each other wherever the `let` happened to land —
    /// that depends on the column reached at the time, which the document doesn't
    /// know until it's being laid out.
    Align(Box<Doc>),
    /// The unit of the fit-or-break decision.
    Group(Box<Doc>),
    Ann(Ann, Box<Doc>),
}

impl Doc {
    pub fn text(s: impl Into<String>) -> Doc {
        Doc::Text(s.into())
    }

    pub fn concat(docs: Vec<Doc>) -> Doc {
        Doc::Concat(docs)
    }

    pub fn nest(n: isize, doc: Doc) -> Doc {
        Doc::Nest(n, Box::new(doc))
    }

    pub fn align(doc: Doc) -> Doc {
        Doc::Align(Box::new(doc))
    }

    pub fn group(doc: Doc) -> Doc {
        Doc::Group(Box::new(doc))
    }

    pub fn ann(ann: Ann, doc: Doc) -> Doc {
        Doc::Ann(ann, Box::new(doc))
    }

    /// `docs` interleaved with `sep`.
    pub fn join(sep: Doc, docs: Vec<Doc>) -> Doc {
        let mut out = Vec::new();
        for (i, doc) in docs.into_iter().enumerate() {
            if i > 0 {
                out.push(sep.clone());
            }
            out.push(doc);
        }
        Doc::Concat(out)
    }
}

/// One piece of laid-out output.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Text(String),
    /// A newline followed by this many spaces of indent.
    Break(usize),
    Open(Ann),
    Close,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Flat,
    Break,
}

/// A layout worklist entry. `Close` is how a balanced `Ann` end marker gets
/// emitted at the right moment: it's pushed *under* the annotated document, so it
/// pops only once that document is fully laid out.
enum Cmd<'a> {
    Doc(usize, Mode, &'a Doc),
    Close,
}

/// Whether `doc`, laid out flat at `indent`, followed by whatever `rest` still
/// holds, reaches the end of the line within `space` columns.
///
/// Looking past `doc` into `rest` is what makes this the "strict" formulation:
/// a group that would fit on its own but is followed by a `then` or a closing
/// paren that pushes the line over still counts as not fitting. The scan stops
/// at the first newline that will actually be emitted, since everything after it
/// starts a fresh line and can't overflow this one.
fn fits(space: isize, indent: usize, doc: &Doc, rest: &[Cmd]) -> bool {
    let mut left = space;
    let mut work: Vec<(usize, Mode, &Doc)> = vec![(indent, Mode::Flat, doc)];
    // `rest` is a stack, so its top — what comes next — is at the end.
    let mut tail = rest.iter().rev();

    loop {
        let Some((indent, mode, doc)) = work.pop() else {
            // Ran out of the group itself; continue into what follows it.
            match tail.next() {
                Some(Cmd::Doc(i, m, d)) => {
                    work.push((*i, *m, d));
                    continue;
                }
                Some(Cmd::Close) => continue,
                None => return left >= 0,
            }
        };
        if left < 0 {
            return false;
        }
        match doc {
            Doc::Nil => {}
            Doc::Text(s) => left -= s.chars().count() as isize,
            Doc::Line => match mode {
                Mode::Flat => left -= 1,
                Mode::Break => return true,
            },
            Doc::SoftBreak => match mode {
                Mode::Flat => {}
                Mode::Break => return true,
            },
            // Inside the group being flattened (`Flat`), a hard newline is
            // un-flattenable, so the group can't stay on one line. Reached in
            // `Break` mode it belongs to the enclosing context instead — it's
            // where this line ends, so everything measured so far did fit. The
            // top-level `HardLine` between decls is exactly this second case.
            Doc::HardLine => return mode == Mode::Break,
            Doc::Concat(docs) => {
                for d in docs.iter().rev() {
                    work.push((indent, mode, d));
                }
            }
            Doc::Nest(n, d) => work.push((offset(indent, *n), mode, d)),
            // The indent an `Align` would set is the column it starts at, which
            // isn't tracked here. It can't change the answer: the scan stops at
            // the first break, and indentation only ever applies *after* one.
            Doc::Align(d) => work.push((indent, mode, d)),
            Doc::Group(d) => work.push((indent, Mode::Flat, d)),
            Doc::Ann(_, d) => work.push((indent, mode, d)),
        }
    }
}

fn offset(indent: usize, n: isize) -> usize {
    (indent as isize + n).max(0) as usize
}

/// Lays `doc` out to `width` columns.
pub fn layout(doc: &Doc, width: usize) -> Vec<Item> {
    let mut out = Vec::new();
    let mut col = 0usize;
    let mut work: Vec<Cmd> = vec![Cmd::Doc(0, Mode::Break, doc)];

    while let Some(cmd) = work.pop() {
        let (indent, mode, doc) = match cmd {
            Cmd::Close => {
                out.push(Item::Close);
                continue;
            }
            Cmd::Doc(i, m, d) => (i, m, d),
        };
        match doc {
            Doc::Nil => {}
            Doc::Text(s) => {
                col += s.chars().count();
                out.push(Item::Text(s.clone()));
            }
            Doc::Line => match mode {
                Mode::Flat => {
                    col += 1;
                    out.push(Item::Text(" ".to_string()));
                }
                Mode::Break => {
                    col = indent;
                    out.push(Item::Break(indent));
                }
            },
            Doc::SoftBreak => match mode {
                Mode::Flat => {}
                Mode::Break => {
                    col = indent;
                    out.push(Item::Break(indent));
                }
            },
            Doc::HardLine => {
                col = indent;
                out.push(Item::Break(indent));
            }
            Doc::Concat(docs) => {
                for d in docs.iter().rev() {
                    work.push(Cmd::Doc(indent, mode, d));
                }
            }
            Doc::Nest(n, d) => work.push(Cmd::Doc(offset(indent, *n), mode, d)),
            Doc::Align(d) => work.push(Cmd::Doc(col, mode, d)),
            Doc::Group(d) => {
                // `width` is `usize::MAX` for "never break", which would come out
                // of the cast as -1 and break *everything* — clamp before it does.
                let space = width.saturating_sub(col).min(isize::MAX as usize) as isize;
                let mode = if fits(space, indent, d, &work) {
                    Mode::Flat
                } else {
                    Mode::Break
                };
                work.push(Cmd::Doc(indent, mode, d));
            }
            Doc::Ann(ann, d) => {
                out.push(Item::Open(ann.clone()));
                work.push(Cmd::Close);
                work.push(Cmd::Doc(indent, mode, d));
            }
        }
    }
    out
}
