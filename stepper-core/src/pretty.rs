use crate::ast::{Decl, Expr, HighlightColor, Pattern, Type};

/// Sentinel character pairs wrapping a highlighted `Expr::Highlighted` node's text,
/// one pair per color. Chosen from the Unicode Private Use Area, so they can never
/// collide with program text (identifiers/keywords/digits/punctuation are all
/// ASCII). The caller splits on these to build DOM nodes directly, rather than us
/// emitting HTML.
pub const HIGHLIGHT_START: char = '\u{E000}';
pub const HIGHLIGHT_END: char = '\u{E001}';
pub const GREEN_START: char = '\u{E002}';
pub const GREEN_END: char = '\u{E003}';
pub const RED_START: char = '\u{E004}';
pub const RED_END: char = '\u{E005}';

fn sentinels(color: HighlightColor) -> (char, char) {
    match color {
        HighlightColor::Yellow => (HIGHLIGHT_START, HIGHLIGHT_END),
        HighlightColor::Green => (GREEN_START, GREEN_END),
        HighlightColor::Red => (RED_START, RED_END),
    }
}

pub fn pretty_print(program: &[Decl]) -> String {
    program
        .iter()
        .map(pretty_print_decl)
        .collect::<Vec<_>>()
        .join("\n")
}

fn pretty_print_decl(decl: &Decl) -> String {
    match &decl.ty {
        Some(ty) => format!(
            "val {} : {} = {}",
            pretty_print_pattern(&decl.pat),
            pretty_print_type(ty),
            pretty_print_expr(&decl.expr)
        ),
        None => format!(
            "val {} = {}",
            pretty_print_pattern(&decl.pat),
            pretty_print_expr(&decl.expr)
        ),
    }
}

pub(crate) fn pretty_print_pattern(pat: &Pattern) -> String {
    match pat {
        Pattern::Ident(name) => name.clone(),
        Pattern::Wildcard => "_".to_string(),
        Pattern::IntConst(n) => format_int(*n),
        Pattern::BoolConst(b) => b.to_string(),
        Pattern::Tuple(pats) => {
            let items = pats.iter().map(pretty_print_pattern).collect::<Vec<_>>().join(", ");
            format!("({items})")
        }
    }
}

fn pretty_print_type(ty: &Type) -> String {
    match ty {
        Type::Int => "int".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Product(types) => types
            .iter()
            .map(|t| pretty_print_type_atom(t))
            .collect::<Vec<_>>()
            .join(" * "),
    }
}

/// Prints `ty` parenthesized if it wouldn't otherwise round-trip as one element of
/// a `*`-separated list. A nested `Product` needs parens — `(int * bool) * int`
/// printed bare as `int * bool * int` would reparse as one flat 3-tuple instead of
/// a 2-tuple whose first component is itself a 2-tuple. `Int`/`Bool` never need
/// them; this is also where a future paren'd type (e.g. a function type used as a
/// product component) would earn its parens back.
fn pretty_print_type_atom(ty: &Type) -> String {
    match ty {
        Type::Product(_) => format!("({})", pretty_print_type(ty)),
        Type::Int | Type::Bool => pretty_print_type(ty),
    }
}

pub(crate) fn pretty_print_expr(expr: &Expr) -> String {
    pretty_print_expr_prec(expr, 0)
}

/// Prints `expr`, parenthesizing it if its own precedence is lower than `min_prec`
/// (the precedence required by whatever position it's sitting in). Lowest to
/// highest: `if` is 0; `orelse` is 1; `andalso` is 2; comparisons
/// (`=`/`<>`/`</<=/>/>=`) are 3; `+`/`-` are 4; `*`/`div`/`mod` are 5; `~` is 6;
/// atoms are unconditionally high. A left-associative binary op passes its own
/// precedence to its left child and one more than that to its right child, so
/// left-associative same-precedence nesting (`(a + b) + c`) prints bare while the
/// same tree on the right (`a + (b + c)`) — only reachable via explicit parens in
/// the source — gets parens back, since without them it would reparse differently.
/// `andalso`/`orelse` are right-associative, so it's the mirror image (right child
/// gets `own`, left child gets `own+1`). Comparisons are nonassociative (the
/// grammar rejects `a < b < c` outright), so both of their children use own+1: a
/// comparison nested on *either* side of another needs parens to stay meaningful.
fn pretty_print_expr_prec(expr: &Expr, min_prec: u8) -> String {
    match expr {
        Expr::IntConst(n) => format_int(*n),
        Expr::BoolConst(b) => b.to_string(),
        Expr::Ident(name) => name.clone(),
        Expr::Highlighted(inner, color) => {
            let (start, end) = sentinels(*color);
            format!("{start}{}{end}", pretty_print_expr_prec(inner, min_prec))
        }
        Expr::OrElse(l, r) => pretty_print_binop(l, r, "orelse", 1, 2, 1, min_prec),
        Expr::AndAlso(l, r) => pretty_print_binop(l, r, "andalso", 2, 3, 2, min_prec),
        Expr::Eq(l, r) => pretty_print_binop(l, r, "=", 3, 4, 4, min_prec),
        Expr::Ne(l, r) => pretty_print_binop(l, r, "<>", 3, 4, 4, min_prec),
        Expr::Lt(l, r) => pretty_print_binop(l, r, "<", 3, 4, 4, min_prec),
        Expr::Le(l, r) => pretty_print_binop(l, r, "<=", 3, 4, 4, min_prec),
        Expr::Gt(l, r) => pretty_print_binop(l, r, ">", 3, 4, 4, min_prec),
        Expr::Ge(l, r) => pretty_print_binop(l, r, ">=", 3, 4, 4, min_prec),
        Expr::Add(l, r) => pretty_print_binop(l, r, "+", 4, 4, 5, min_prec),
        Expr::Sub(l, r) => pretty_print_binop(l, r, "-", 4, 4, 5, min_prec),
        Expr::Mul(l, r) => pretty_print_binop(l, r, "*", 5, 5, 6, min_prec),
        Expr::Div(l, r) => pretty_print_binop(l, r, "div", 5, 5, 6, min_prec),
        Expr::Mod(l, r) => pretty_print_binop(l, r, "mod", 5, 5, 6, min_prec),
        Expr::Neg(inner) => {
            // Binds tighter than `*`/`div`/`mod`, matching the grammar's
            // `%left '~'` (declared above the `*` line).
            let body = format!("~{}", pretty_print_expr_prec(inner, 6));
            if 6 < min_prec { format!("({body})") } else { body }
        }
        Expr::If(cond, then_branch, else_branch) => {
            // Precedence 0 (lower than everything, matching the grammar's
            // `%nonassoc 'ELSE'` at the lowest level): as soon as an if-expression
            // sits inside any other expression at all, it needs parens to stay
            // unambiguous — the else-branch is otherwise free to keep extending
            // across trailing operators (`if c then a else b + 1` means
            // `if c then a else (b + 1)`), so an `if` used as, say, the left operand
            // of `+` would misparse without them. Its own cond/then/else branches
            // are printed unrestricted (0): the keywords already delimit them
            // unambiguously, which is also why `else if ...` chains stay bare.
            let body = format!(
                "if {} then {} else {}",
                pretty_print_expr_prec(cond, 0),
                pretty_print_expr_prec(then_branch, 0),
                pretty_print_expr_prec(else_branch, 0)
            );
            if 0 < min_prec { format!("({body})") } else { body }
        }
        Expr::Tuple(items) => {
            // Bracketed by its own parens, so — like `Let` — this is atomic from
            // the outside and its elements print unrestricted (0).
            let items_str = items
                .iter()
                .map(|e| pretty_print_expr_prec(e, 0))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({items_str})")
        }
        Expr::Let(decls, body) => {
            // Bracketed by `let`/`end` on both sides, so — unlike `if` — this is
            // atomic from the outside: never needs its own wrapping parens
            // regardless of `min_prec`. Its decls and body are likewise printed
            // unrestricted (0), since the keywords already delimit them.
            let decls_str = pretty_print_decls_inline(decls);
            let body_str = pretty_print_expr_prec(body, 0);
            if decls_str.is_empty() {
                format!("let in {body_str} end")
            } else {
                format!("let {decls_str} in {body_str} end")
            }
        }
    }
}

fn pretty_print_decls_inline(decls: &[Decl]) -> String {
    decls.iter().map(pretty_print_decl).collect::<Vec<_>>().join(" ")
}

/// SML writes negative numbers with `~`, not `-` (which is reserved for the binary
/// subtraction operator). `unsigned_abs` avoids overflow on `i64::MIN`.
fn format_int(n: i64) -> String {
    if n < 0 {
        format!("~{}", n.unsigned_abs())
    } else {
        n.to_string()
    }
}

fn pretty_print_binop(
    l: &Expr,
    r: &Expr,
    op: &str,
    own_prec: u8,
    left_min: u8,
    right_min: u8,
    min_prec: u8,
) -> String {
    let body = format!(
        "{} {op} {}",
        pretty_print_expr_prec(l, left_min),
        pretty_print_expr_prec(r, right_min)
    );
    if own_prec < min_prec {
        format!("({body})")
    } else {
        body
    }
}
