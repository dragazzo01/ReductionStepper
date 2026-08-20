use crate::ast::{Decl, Expr, HighlightColor, Pattern, PatternBase, Type};

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
    match decl {
        Decl::ValDecl(decl) => 
            format!(
                "val {} = {}",
                pretty_print_pattern(&decl.pat),
                pretty_print_expr(&decl.expr)
            ),
        Decl::ValRecDecl(decl) =>
            format!(
                "val rec {} = {}",
                pretty_print_pattern(&decl.pat),
                pretty_print_expr(&decl.expr)
            ),
    }
    
}

fn pretty_print_pattern_base(pat: &PatternBase) -> String {
    match pat {
        PatternBase::Ident(name) => name.clone(),
        PatternBase::Wildcard => "_".to_string(),
        PatternBase::IntConst(n) => format_int(*n),
        PatternBase::BoolConst(b) => b.to_string(),
        PatternBase::Tuple(pats) => {
            let items = pats
                .iter()
                .map(pretty_print_pattern)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({items})")
        }
    }
}

pub(crate) fn pretty_print_pattern(pat: &Pattern) -> String {
    let base = pretty_print_pattern_base(&pat.pat);
    match &pat.typ {
        Some(typ) => format!{"{base} : {}", pretty_print_type(&typ)},
        None => base
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
        // Right-associative, matching the grammar's `%right 'ARROW'`: the left
        // operand needs parens to nest (`(int -> int) -> int` prints bare, but the
        // same tree on the right — `int -> (int -> int)`, only reachable via
        // explicit parens in the source — would reparse as flat right-associative
        // `int -> int -> int` without them, so the right operand always prints
        // bare and the left always goes through the parenthesizing atom form).
        Type::Arrow(param, result) => format!(
            "{} -> {}",
            pretty_print_type_atom(param),
            pretty_print_type(result)
        ),
    }
}

/// Prints `ty` parenthesized if it wouldn't otherwise round-trip as one element of
/// a `*`-separated list or as the left operand of `->`. A nested `Product` needs
/// parens — `(int * bool) * int` printed bare as `int * bool * int` would reparse
/// as one flat 3-tuple instead of a 2-tuple whose first component is itself a
/// 2-tuple. An `Arrow` always needs parens here too: as a product component
/// (`*` binds tighter than `->`) or as `->`'s left operand (right-associative, so
/// only the right side can print bare). `Int`/`Bool` never need them.
fn pretty_print_type_atom(ty: &Type) -> String {
    match ty {
        Type::Product(_) | Type::Arrow(_, _) => format!("({})", pretty_print_type(ty)),
        Type::Int | Type::Bool => pretty_print_type(ty),
    }
}

pub(crate) fn pretty_print_expr(expr: &Expr) -> String {
    pretty_print_expr_prec(expr, 0)
}

/// Prints `expr`, parenthesizing it if its own precedence is lower than `min_prec`
/// (the precedence required by whatever position it's sitting in). Lowest to
/// highest: `if`/`case`/`fn` are 0; `orelse` is 1; `andalso` is 2; comparisons
/// (`=`/`<>`/`</<=/>/>=`) are 3; `+`/`-` are 4; `*`/`div`/`mod` are 5; function
/// application is 6 — above every infix operator, as in SML; `~` is 7; atoms
/// (literals, identifiers, tuples, `let`) are unconditionally high and ignore
/// `min_prec` entirely. A left-associative binary op passes its own
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
            // Atomic, matching the grammar's `AtomicExpr -> '~' AtomicExpr`: its
            // operand is an atom, so anything compound — an application included —
            // takes parens (`~(f x)`), while `~~5` and `~x` stay bare.
            let body = format!("~{}", pretty_print_expr_prec(inner, 7));
            if 7 < min_prec {
                format!("({body})")
            } else {
                body
            }
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
            if 0 < min_prec {
                format!("({body})")
            } else {
                body
            }
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
        Expr::Match(scrutinee, arms) => {
            // Bracketed by `case`/`of` on the left but open-ended on the right (no
            // `end`), matching real SML — so, like `If`, this needs parens as soon
            // as it sits inside anything else. The scrutinee and each arm's body
            // print unrestricted (0), since the keywords/`=>`/`|` already delimit
            // them; a nested `case` as the last arm's body prints bare and simply
            // absorbs any further `|` arms, mirroring the grammar's own shift
            // preference (see grammar.y's `MatchArms`).
            let arms_str = arms
                .iter()
                .map(|(pat, arm_expr)| {
                    format!(
                        "{} => {}",
                        pretty_print_pattern(pat),
                        pretty_print_expr_prec(arm_expr, 0)
                    )
                })
                .collect::<Vec<_>>()
                .join(" | ");
            let body = format!(
                "case {} of {}",
                pretty_print_expr_prec(scrutinee, 0),
                arms_str
            );
            if 0 < min_prec {
                format!("({body})")
            } else {
                body
            }
        }
        Expr::Lambda(cases) => {
            // Open-ended (no closing keyword), same treatment as `If`/`Match`:
            // precedence 0, needs parens the instant it sits inside anything else
            // (e.g. as an application operand — see `Expr::App` below). The body
            // prints unrestricted (0) since `=>` already delimits it unambiguously.
            let cases_str = cases
                .iter()
                .map(|(pat, arm_expr)| {
                    format!(
                        "{} => {}",
                        pretty_print_pattern(pat),
                        pretty_print_expr_prec(arm_expr, 0)
                    )
                })
                .collect::<Vec<_>>()
                .join(" | ");

            let fn_str = format!("fn {cases_str}");
            if 0 < min_prec {
                format!("({fn_str})")
            } else {
                fn_str
            }
        }
        // Binds tighter than every infix operator (own_prec 6, above `*`'s 5), so
        // an application prints bare as any infix operand — `f x + y`. Only `~`
        // (7, atomic) outranks it, which is why `~f x` prints bare too and means
        // `(~f) x`, matching the grammar. Left-associative, so the left child
        // accepts its own precedence (nested application prints bare — `f x y`,
        // not `(f x) y`) while the right child needs strictly higher, since the
        // grammar's `AtomicExpr` never includes a bare `App` — `f (g x)` only
        // round-trips with parens.
        Expr::App(f, arg) => {
            let body = format!(
                "{} {}",
                pretty_print_expr_prec(f, 6),
                pretty_print_expr_prec(arg, 7)
            );
            if 6 < min_prec {
                format!("({body})")
            } else {
                body
            }
        }
        Expr::Let(decls, body) => {
            // Bracketed by `let`/`end` on both sides, so — unlike `if` — this is
            // a true atom, one of SML's `atexp` forms (see grammar.y's
            // `AtomicExpr`): never needs its own wrapping parens regardless of
            // `min_prec`, even as an application argument. Its decls and body are
            // likewise printed
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
    decls
        .iter()
        .map(pretty_print_decl)
        .collect::<Vec<_>>()
        .join(" ")
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
