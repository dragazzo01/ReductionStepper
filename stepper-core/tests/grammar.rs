//! Parsing: what each construct turns into, and how the precedence table and the
//! `atexp`/`exp` split resolve everything in between. Nothing here evaluates or
//! typechecks — see `stepping.rs` and `typecheck.rs` for those.

mod common;

use common::*;
use stepper_core::ast::{Decl, Expr, PatternBase, Type};
use stepper_core::parse_program;

#[test]
fn parses_a_simple_decl() {
    assert_eq!(
        parse("val x = 15 * 10"),
        vec![val(
            pvar("x"),
            Expr::Mul(
                Box::new(Expr::IntConst(15)),
                Box::new(Expr::IntConst(10)),
            ),
        )]
    );
}

#[test]
fn parses_minus_div_mod_at_expected_precedence() {
    // div/mod bind as tightly as *, above the +/- level.
    assert_eq!(
        parse("val x = 10 - 6 div 2 mod 4"),
        vec![val(
            pvar("x"),
            Expr::Sub(
                Box::new(Expr::IntConst(10)),
                Box::new(Expr::Mod(
                    Box::new(Expr::Div(
                        Box::new(Expr::IntConst(6)),
                        Box::new(Expr::IntConst(2)),
                    )),
                    Box::new(Expr::IntConst(4)),
                )),
            ),
        )]
    );
}

#[test]
fn parses_bool_literals_and_type_annotation() {
    assert_eq!(
        parse("val b : bool = true"),
        vec![val(pvar_typed("b", Type::Bool), Expr::BoolConst(true))]
    );
    assert_eq!(expr_of("val b = false"), Expr::BoolConst(false));
}

#[test]
fn parses_if_then_else() {
    assert_eq!(
        expr_of("val x = if true then 1 else 2"),
        Expr::If(
            Box::new(Expr::BoolConst(true)),
            Box::new(Expr::IntConst(1)),
            Box::new(Expr::IntConst(2)),
        )
    );
}

#[test]
fn else_branch_greedily_extends_across_operators() {
    // The else-branch should keep consuming "+ 1" rather than the whole
    // if-expression becoming the left operand of +.
    assert_eq!(
        expr_of("val x = if true then 1 else 2 + 1"),
        Expr::If(
            Box::new(Expr::BoolConst(true)),
            Box::new(Expr::IntConst(1)),
            Box::new(Expr::Add(
                Box::new(Expr::IntConst(2)),
                Box::new(Expr::IntConst(1)),
            )),
        )
    );
}

#[test]
fn parses_unary_minus_tighter_than_mul() {
    // ~2 * 3 should be (~2) * 3, not ~(2 * 3) -- unary minus binds tighter than *.
    assert_eq!(
        expr_of("val x = ~2 * 3"),
        Expr::Mul(
            Box::new(Expr::Neg(Box::new(Expr::IntConst(2)))),
            Box::new(Expr::IntConst(3)),
        )
    );
}

#[test]
fn parses_unary_minus_before_binary_add() {
    // ~1 + 2 should be (~1) + 2, not ~(1 + 2).
    assert_eq!(
        expr_of("val x = ~1 + 2"),
        Expr::Add(
            Box::new(Expr::Neg(Box::new(Expr::IntConst(1)))),
            Box::new(Expr::IntConst(2)),
        )
    );
}

#[test]
fn parens_override_precedence() {
    assert_eq!(
        expr_of("val x = (1 + 2) * 3"),
        Expr::Mul(
            Box::new(Expr::Add(
                Box::new(Expr::IntConst(1)),
                Box::new(Expr::IntConst(2)),
            )),
            Box::new(Expr::IntConst(3)),
        )
    );
}

#[test]
fn parens_are_redundant_when_they_match_precedence() {
    assert_eq!(parse("val x = 1 + (2 * 3)"), parse("val x = 1 + 2 * 3"));
}

#[test]
fn respects_precedence_and_type_annotations() {
    assert_eq!(
        parse("val y : int = x + 15 * 1000"),
        vec![val(
            pvar_typed("y", Type::Int),
            Expr::Add(
                Box::new(ident("x")),
                Box::new(Expr::Mul(
                    Box::new(Expr::IntConst(15)),
                    Box::new(Expr::IntConst(1000)),
                )),
            ),
        )]
    );
}

#[test]
fn parses_multiple_decls() {
    assert_eq!(parse("val x = 1\nval y = x + 1").len(), 2);
}

#[test]
fn rejects_garbage() {
    assert!(parse_program("val = 1").is_err());
}

// ---------------------------------------------------------------------------
// Comparisons
// ---------------------------------------------------------------------------

#[test]
fn parses_all_comparison_operators() {
    let cases = [
        ("val x = 1 = 2", Expr::Eq as fn(_, _) -> _),
        ("val x = 1 <> 2", Expr::Ne),
        ("val x = 1 < 2", Expr::Lt),
        ("val x = 1 <= 2", Expr::Le),
        ("val x = 1 > 2", Expr::Gt),
        ("val x = 1 >= 2", Expr::Ge),
    ];
    for (src, ctor) in cases {
        assert_eq!(
            expr_of(src),
            ctor(Box::new(Expr::IntConst(1)), Box::new(Expr::IntConst(2))),
            "parsing {src:?}"
        );
    }
}

#[test]
fn comparisons_bind_looser_than_arithmetic() {
    // 1 + 2 < 3 * 4 should compare the two arithmetic results, not e.g. try to add
    // 2 to a bool.
    assert_eq!(
        expr_of("val x = 1 + 2 < 3 * 4"),
        Expr::Lt(
            Box::new(Expr::Add(
                Box::new(Expr::IntConst(1)),
                Box::new(Expr::IntConst(2)),
            )),
            Box::new(Expr::Mul(
                Box::new(Expr::IntConst(3)),
                Box::new(Expr::IntConst(4)),
            )),
        )
    );
}

#[test]
fn rejects_chained_comparisons() {
    // Nonassociative: "a < b < c" isn't meaningful (a < b is a bool, < wants ints),
    // so the grammar should reject it outright rather than silently pick a grouping.
    assert!(parse_program("val x = 1 < 2 < 3").is_err());
}

#[test]
fn parenthesized_comparisons_can_still_nest() {
    // Parens sidestep the nonassoc restriction; nothing stops this syntactically
    // even though it wouldn't type-check in real SML.
    assert_eq!(
        expr_of("val x = (1 < 2) = (3 < 4)"),
        Expr::Eq(
            Box::new(Expr::Lt(
                Box::new(Expr::IntConst(1)),
                Box::new(Expr::IntConst(2)),
            )),
            Box::new(Expr::Lt(
                Box::new(Expr::IntConst(3)),
                Box::new(Expr::IntConst(4)),
            )),
        )
    );
}

// ---------------------------------------------------------------------------
// andalso / orelse
// ---------------------------------------------------------------------------

#[test]
fn andalso_binds_tighter_than_orelse() {
    assert_eq!(
        expr_of("val x = true orelse false andalso false"),
        Expr::OrElse(
            Box::new(Expr::BoolConst(true)),
            Box::new(Expr::AndAlso(
                Box::new(Expr::BoolConst(false)),
                Box::new(Expr::BoolConst(false)),
            )),
        )
    );
}

#[test]
fn andalso_orelse_are_right_associative() {
    assert_eq!(
        expr_of("val x = true andalso true andalso false"),
        Expr::AndAlso(
            Box::new(Expr::BoolConst(true)),
            Box::new(Expr::AndAlso(
                Box::new(Expr::BoolConst(true)),
                Box::new(Expr::BoolConst(false)),
            )),
        )
    );
}

#[test]
fn andalso_binds_looser_than_comparisons() {
    assert_eq!(
        expr_of("val x = 1 < 2 andalso 3 < 4"),
        Expr::AndAlso(
            Box::new(Expr::Lt(
                Box::new(Expr::IntConst(1)),
                Box::new(Expr::IntConst(2)),
            )),
            Box::new(Expr::Lt(
                Box::new(Expr::IntConst(3)),
                Box::new(Expr::IntConst(4)),
            )),
        )
    );
}

// ---------------------------------------------------------------------------
// let / tuples
// ---------------------------------------------------------------------------

#[test]
fn parses_let_in_end() {
    assert_eq!(
        expr_of("val y = let val x = 1 in x + 1 end"),
        Expr::Let(
            vec![val(pvar("x"), Expr::IntConst(1))],
            Box::new(Expr::Add(
                Box::new(ident("x")),
                Box::new(Expr::IntConst(1)),
            )),
        )
    );
}

#[test]
fn let_supports_multiple_decls() {
    let Expr::Let(decls, _) = expr_of("val y = let val x = 1 val z = 2 in x + z end") else {
        panic!("expected a Let");
    };
    assert_eq!(decls.len(), 2);
}

#[test]
fn parses_tuples() {
    assert_eq!(
        expr_of("val x = (1, true, 2 + 3)"),
        Expr::Tuple(vec![
            Expr::IntConst(1),
            Expr::BoolConst(true),
            Expr::Add(Box::new(Expr::IntConst(2)), Box::new(Expr::IntConst(3))),
        ])
    );
}

#[test]
fn single_parenthesized_expr_is_grouping_not_a_tuple() {
    assert_eq!(
        expr_of("val x = (1 + 2)"),
        Expr::Add(Box::new(Expr::IntConst(1)), Box::new(Expr::IntConst(2)))
    );
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[test]
fn parses_a_product_type_annotation() {
    assert_eq!(
        type_of("val x : int * bool = (1, true)"),
        Some(Type::Product(vec![
            Box::new(Type::Int),
            Box::new(Type::Bool),
        ]))
    );
}

#[test]
fn parses_a_product_type_with_more_than_two_elements() {
    // Left-recursive `TypeProduct`, mirroring `TupleItems`: each `*` should push
    // onto the same flat `Vec` rather than nesting.
    assert_eq!(
        type_of("val x : int * bool * int = (1, true, 2)"),
        Some(Type::Product(vec![
            Box::new(Type::Int),
            Box::new(Type::Bool),
            Box::new(Type::Int),
        ]))
    );
}

#[test]
fn parenthesized_type_is_just_grouping() {
    assert_eq!(type_of("val x : (int) = 1"), Some(Type::Int));
}

#[test]
fn parens_let_a_type_nest_inside_a_product() {
    assert_eq!(
        type_of("val x : (int * bool) * int = ((1, true), 2)"),
        Some(Type::Product(vec![
            Box::new(Type::Product(vec![
                Box::new(Type::Int),
                Box::new(Type::Bool),
            ])),
            Box::new(Type::Int),
        ]))
    );
}

#[test]
fn arrow_types_are_right_associative() {
    // `int -> int -> int` is `int -> (int -> int)`: a function returning a
    // function, matching SML.
    assert_eq!(
        type_of("val f : int -> int -> int = fn x : int => fn y : int => x"),
        Some(Type::Arrow(
            Box::new(Type::Int),
            Box::new(Type::Arrow(Box::new(Type::Int), Box::new(Type::Int))),
        ))
    );
    // Parens are what make it a function *taking* a function instead.
    assert_eq!(
        type_of("val f : (int -> int) -> int = fn g : int -> int => g 1"),
        Some(Type::Arrow(
            Box::new(Type::Arrow(Box::new(Type::Int), Box::new(Type::Int))),
            Box::new(Type::Int),
        ))
    );
}

#[test]
fn arrow_binds_looser_than_product() {
    // `int * int -> bool` is `(int * int) -> bool`, not `int * (int -> bool)`.
    assert_eq!(
        type_of("val f : int * int -> bool = fn p : int * int => true"),
        Some(Type::Arrow(
            Box::new(Type::Product(vec![
                Box::new(Type::Int),
                Box::new(Type::Int),
            ])),
            Box::new(Type::Bool),
        ))
    );
}

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

#[test]
fn parses_a_wildcard_pattern() {
    assert_eq!(pattern_of("val _ = 5"), pat(PatternBase::Wildcard));
}

#[test]
fn parses_a_tuple_pattern() {
    assert_eq!(
        pattern_of("val (x, y) = (1, 2)"),
        pat(PatternBase::Tuple(vec![pvar("x"), pvar("y")]))
    );
}

#[test]
fn parenthesized_pattern_is_just_grouping() {
    assert_eq!(pattern_of("val (x) = 1"), pvar("x"));
}

#[test]
fn parses_a_nested_tuple_pattern_with_a_wildcard() {
    assert_eq!(
        pattern_of("val (x, (y, _)) = (1, (2, 3))"),
        pat(PatternBase::Tuple(vec![
            pvar("x"),
            pat(PatternBase::Tuple(vec![
                pvar("y"),
                pat(PatternBase::Wildcard),
            ])),
        ]))
    );
}

#[test]
fn a_type_annotation_belongs_to_the_pattern_it_follows() {
    // The annotation hangs off the `Pattern`, not the decl, so it works on any
    // pattern form — a tuple pattern included.
    assert_eq!(
        pattern_of("val (x, y) : int * bool = (1, true)"),
        pat_typed(
            PatternBase::Tuple(vec![pvar("x"), pvar("y")]),
            Type::Product(vec![Box::new(Type::Int), Box::new(Type::Bool)]),
        )
    );
    // ...and on the components of one, independently.
    assert_eq!(
        pattern_of("val (x : int, y) = (1, true)"),
        pat(PatternBase::Tuple(vec![
            pvar_typed("x", Type::Int),
            pvar("y"),
        ]))
    );
}

#[test]
fn parses_literal_patterns_including_negative_ints() {
    // `~` in a pattern is part of the literal (there's no negation to evaluate in
    // a pattern), so `~1` is one `IntConst(-1)` rather than a `Neg` node.
    let Expr::Match(_, arms) = expr_of("val x = case n of ~1 => 0 | true => 1 | 2 => 2") else {
        panic!("expected a Match");
    };
    let patterns: Vec<_> = arms.into_iter().map(|(p, _)| p).collect();
    assert_eq!(
        patterns,
        vec![
            pat(PatternBase::IntConst(-1)),
            pat(PatternBase::BoolConst(true)),
            pat(PatternBase::IntConst(2)),
        ]
    );
}

// ---------------------------------------------------------------------------
// case
// ---------------------------------------------------------------------------

#[test]
fn parses_a_case_expression() {
    assert_eq!(
        expr_of("val x = case 1 of 0 => true | _ => false"),
        Expr::Match(
            Box::new(Expr::IntConst(1)),
            vec![
                (pat(PatternBase::IntConst(0)), Expr::BoolConst(true)),
                (pat(PatternBase::Wildcard), Expr::BoolConst(false)),
            ],
        )
    );
}

#[test]
fn case_arm_body_extends_greedily_across_operators() {
    // Like the else-branch of `if`, an arm's body should keep consuming "+ 1"
    // rather than the whole case-expression becoming the left operand of +.
    assert_eq!(
        expr_of("val x = case 1 of _ => 2 + 1"),
        Expr::Match(
            Box::new(Expr::IntConst(1)),
            vec![(
                pat(PatternBase::Wildcard),
                Expr::Add(Box::new(Expr::IntConst(2)), Box::new(Expr::IntConst(1))),
            )],
        )
    );
}

#[test]
fn nested_case_dangling_bar_attaches_to_the_innermost_case() {
    // Without parens, a trailing "| p3 => c" after a nested case should attach to
    // the *inner* case (shift-preferred, same "nearest wins" rule real SML uses
    // for this — mirrors dangling-else), not float out to the outer one.
    assert_eq!(
        expr_of("val x = case 1 of a => case 2 of b => 10 | c => 20 | d => 30"),
        Expr::Match(
            Box::new(Expr::IntConst(1)),
            vec![(
                pvar("a"),
                Expr::Match(
                    Box::new(Expr::IntConst(2)),
                    vec![
                        (pvar("b"), Expr::IntConst(10)),
                        (pvar("c"), Expr::IntConst(20)),
                        (pvar("d"), Expr::IntConst(30)),
                    ],
                ),
            )],
        )
    );
}

#[test]
fn parens_let_a_trailing_bar_attach_to_the_outer_case() {
    // Explicit parens around the inner case override the default "nearest wins"
    // attachment, letting "| c => 20" belong to the outer case instead.
    assert_eq!(
        expr_of("val x = case 1 of a => (case 2 of b => 10) | c => 20"),
        Expr::Match(
            Box::new(Expr::IntConst(1)),
            vec![
                (
                    pvar("a"),
                    Expr::Match(
                        Box::new(Expr::IntConst(2)),
                        vec![(pvar("b"), Expr::IntConst(10))],
                    ),
                ),
                (pvar("c"), Expr::IntConst(20)),
            ],
        )
    );
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

#[test]
fn parses_a_lambda_as_a_one_armed_match() {
    // `fn` shares `MatchArms` with `case`, so even a single-parameter lambda is a
    // one-element arm list — its parameter is a pattern like any other.
    assert_eq!(
        expr_of("val f = fn x : int => x + 1"),
        Expr::Lambda(vec![(
            pvar_typed("x", Type::Int),
            Expr::Add(Box::new(ident("x")), Box::new(Expr::IntConst(1))),
        )])
    );
}

#[test]
fn parses_a_multi_arm_lambda() {
    // `fn p1 => e1 | p2 => e2` — the argument is matched against the arms in
    // order, exactly as a `case` on it would be.
    assert_eq!(
        expr_of("val f = fn n : int => n | 0 => 1"),
        Expr::Lambda(vec![
            (pvar_typed("n", Type::Int), ident("n")),
            (pat(PatternBase::IntConst(0)), Expr::IntConst(1)),
        ])
    );
}

#[test]
fn parses_a_val_rec_decl() {
    // `val rec` is its own `Decl` variant carrying the same `ValDecl` payload as a
    // plain `val`; only the variant distinguishes them.
    assert_eq!(
        parse("val rec f : int -> int = fn n : int => f n"),
        vec![val_rec(
            pvar_typed("f", Type::Arrow(Box::new(Type::Int), Box::new(Type::Int))),
            Expr::Lambda(vec![(
                pvar_typed("n", Type::Int),
                Expr::App(Box::new(ident("f")), Box::new(ident("n"))),
            )]),
        )]
    );
}

#[test]
fn val_and_val_rec_are_distinct_decls() {
    let plain = parse("val f : int -> int = fn n : int => n");
    let recursive = parse("val rec f : int -> int = fn n : int => n");
    assert!(matches!(plain[0], Decl::ValDecl(_)));
    assert!(matches!(recursive[0], Decl::ValRecDecl(_)));
    // Same `ValDecl` payload underneath — only the variant differs, which is what
    // `get_val_decl` exists to see past.
    assert_eq!(plain[0].get_val_decl(), recursive[0].get_val_decl());
    assert_ne!(plain, recursive);
}

#[test]
fn application_binds_tighter_than_every_infix_operator() {
    let id = |n: &str| Box::new(ident(n));
    let app = |f: &str, x: &str| Box::new(Expr::App(id(f), id(x)));

    // "Function calls have very high precedence, higher than any infix operator."
    assert_eq!(expr_of("val z = f x + y"), Expr::Add(app("f", "x"), id("y")));
    assert_eq!(expr_of("val z = y + f x"), Expr::Add(id("y"), app("f", "x")));
    assert_eq!(
        expr_of("val z = f x * g y"),
        Expr::Mul(app("f", "x"), app("g", "y"))
    );
    assert_eq!(
        expr_of("val z = f x div g y"),
        Expr::Div(app("f", "x"), app("g", "y"))
    );
    assert_eq!(
        expr_of("val z = f x < g y"),
        Expr::Lt(app("f", "x"), app("g", "y"))
    );
    assert_eq!(
        expr_of("val z = f x andalso g y"),
        Expr::AndAlso(app("f", "x"), app("g", "y"))
    );
    assert_eq!(
        expr_of("val z = f x orelse g y"),
        Expr::OrElse(app("f", "x"), app("g", "y"))
    );
}

#[test]
fn application_is_left_associative() {
    let id = |n: &str| Box::new(ident(n));
    // `f x y` is `(f x) y`, and `f (g x)` needs its parens to mean anything else.
    assert_eq!(
        expr_of("val z = f x y"),
        Expr::App(Box::new(Expr::App(id("f"), id("x"))), id("y"))
    );
    assert_eq!(
        expr_of("val z = f (g x)"),
        Expr::App(id("f"), Box::new(Expr::App(id("g"), id("x"))))
    );
}

#[test]
fn application_argument_accepts_every_atomic_expression() {
    // Whatever SML calls an `atexp` may be an argument without parens: constants
    // (including negative literals), identifiers, parenthesized exprs, tuples,
    // and `let ... in ... end`.
    for src in [
        "val z = f 1",
        "val z = f ~5",
        "val z = f x",
        "val z = f true",
        "val z = f (1 + 2)",
        "val z = f (1, 2)",
        "val z = f let val a = 1 in a end",
        "val z = (fn x : int => x) 3",
    ] {
        assert!(parse_program(src).is_ok(), "should parse: `{src}`");
    }
}

#[test]
fn application_binds_tighter_than_unary_minus() {
    let id = |n: &str| Box::new(ident(n));

    // `~` takes an atom, so application wins: `~ f x` is `(~f) x`, matching SML,
    // where `~` is an ordinary function identifier and application is
    // left-associative. (It's a type error there and here -- see
    // `typecheck.rs`'s `rejects_negating_a_function`.)
    assert_eq!(
        expr_of("val z = ~ f x"),
        Expr::App(Box::new(Expr::Neg(id("f"))), id("x"))
    );
    // Negating an application therefore needs its parens back.
    assert_eq!(
        expr_of("val z = ~(f x)"),
        Expr::Neg(Box::new(Expr::App(id("f"), id("x"))))
    );
    // And a negative literal is usable as an argument without them, since it's
    // an atom on both sides of the grammar.
    assert_eq!(
        expr_of("val z = f ~5"),
        Expr::App(id("f"), Box::new(Expr::Neg(Box::new(Expr::IntConst(5)))))
    );
    // Unchanged: `~` still outranks every infix operator.
    assert_eq!(
        expr_of("val z = ~x * y"),
        Expr::Mul(Box::new(Expr::Neg(id("x"))), id("y"))
    );
}

#[test]
fn let_is_an_atomic_expression() {
    // `let ... end` is one of SML's `atexp` forms, so it needs no parens as an
    // application argument -- it's closed by `end`, unlike `if`/`case`/`fn`.
    assert_eq!(
        expr_of("val z = f let val a = 1 in a end"),
        Expr::App(
            Box::new(ident("f")),
            Box::new(Expr::Let(
                vec![val(pvar("a"), Expr::IntConst(1))],
                Box::new(ident("a")),
            ))
        )
    );
    // The open-ended forms still need parens, as they do in real SML.
    assert!(parse_program("val z = f if b then 1 else 2").is_err());
    assert!(parse_program("val z = f case x of _ => 1").is_err());
    assert!(parse_program("val z = f fn x : int => x").is_err());
}
