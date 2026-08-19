use super::*;

#[test]
fn grammar_parses_a_simple_decl() {
    let program = parse_program("val x = 15 * 10").unwrap();
    assert_eq!(
        program,
        vec![ast::Decl {
            pat: ast::Pattern::Ident("x".to_string()),
            ty: None,
            expr: ast::Expr::Mul(
                Box::new(ast::Expr::IntConst(15)),
                Box::new(ast::Expr::IntConst(10)),
            ),
        }]
    );
}

#[test]
fn grammar_parses_minus_div_mod_at_expected_precedence() {
    // div/mod bind as tightly as *, above the +/- level.
    let program = parse_program("val x = 10 - 6 div 2 mod 4").unwrap();
    assert_eq!(
        program,
        vec![ast::Decl {
            pat: ast::Pattern::Ident("x".to_string()),
            ty: None,
            expr: ast::Expr::Sub(
                Box::new(ast::Expr::IntConst(10)),
                Box::new(ast::Expr::Mod(
                    Box::new(ast::Expr::Div(
                        Box::new(ast::Expr::IntConst(6)),
                        Box::new(ast::Expr::IntConst(2)),
                    )),
                    Box::new(ast::Expr::IntConst(4)),
                )),
            ),
        }]
    );
}

#[test]
fn grammar_parses_bool_literals_and_type_annotation() {
    let program = parse_program("val b : bool = true").unwrap();
    assert_eq!(
        program,
        vec![ast::Decl {
            pat: ast::Pattern::Ident("b".to_string()),
            ty: Some(ast::Type::Bool),
            expr: ast::Expr::BoolConst(true),
        }]
    );
    assert_eq!(
        parse_program("val b = false").unwrap()[0].expr,
        ast::Expr::BoolConst(false)
    );
}

#[test]
fn grammar_parses_if_then_else() {
    let program = parse_program("val x = if true then 1 else 2").unwrap();
    assert_eq!(
        program,
        vec![ast::Decl {
            pat: ast::Pattern::Ident("x".to_string()),
            ty: None,
            expr: ast::Expr::If(
                Box::new(ast::Expr::BoolConst(true)),
                Box::new(ast::Expr::IntConst(1)),
                Box::new(ast::Expr::IntConst(2)),
            ),
        }]
    );
}

#[test]
fn grammar_else_branch_greedily_extends_across_operators() {
    // The else-branch should keep consuming "+ 1" rather than the whole
    // if-expression becoming the left operand of +.
    let program = parse_program("val x = if true then 1 else 2 + 1").unwrap();
    assert_eq!(
        program[0].expr,
        ast::Expr::If(
            Box::new(ast::Expr::BoolConst(true)),
            Box::new(ast::Expr::IntConst(1)),
            Box::new(ast::Expr::Add(
                Box::new(ast::Expr::IntConst(2)),
                Box::new(ast::Expr::IntConst(1)),
            )),
        )
    );
}

#[test]
fn grammar_parses_unary_minus_tighter_than_mul() {
    // ~2 * 3 should be (~2) * 3, not ~(2 * 3) -- unary minus binds tighter than *.
    let program = parse_program("val x = ~2 * 3").unwrap();
    assert_eq!(
        program,
        vec![ast::Decl {
            pat: ast::Pattern::Ident("x".to_string()),
            ty: None,
            expr: ast::Expr::Mul(
                Box::new(ast::Expr::Neg(Box::new(ast::Expr::IntConst(2)))),
                Box::new(ast::Expr::IntConst(3)),
            ),
        }]
    );
}

#[test]
fn grammar_parses_unary_minus_before_binary_add() {
    // ~1 + 2 should be (~1) + 2, not ~(1 + 2).
    let program = parse_program("val x = ~1 + 2").unwrap();
    assert_eq!(
        program,
        vec![ast::Decl {
            pat: ast::Pattern::Ident("x".to_string()),
            ty: None,
            expr: ast::Expr::Add(
                Box::new(ast::Expr::Neg(Box::new(ast::Expr::IntConst(1)))),
                Box::new(ast::Expr::IntConst(2)),
            ),
        }]
    );
}

#[test]
fn grammar_parens_override_precedence() {
    let program = parse_program("val x = (1 + 2) * 3").unwrap();
    assert_eq!(
        program,
        vec![ast::Decl {
            pat: ast::Pattern::Ident("x".to_string()),
            ty: None,
            expr: ast::Expr::Mul(
                Box::new(ast::Expr::Add(
                    Box::new(ast::Expr::IntConst(1)),
                    Box::new(ast::Expr::IntConst(2)),
                )),
                Box::new(ast::Expr::IntConst(3)),
            ),
        }]
    );
}

#[test]
fn grammar_parens_are_redundant_when_they_match_precedence() {
    let with_parens = parse_program("val x = 1 + (2 * 3)").unwrap();
    let without_parens = parse_program("val x = 1 + 2 * 3").unwrap();
    assert_eq!(with_parens, without_parens);
}

#[test]
fn grammar_respects_precedence_and_type_annotations() {
    let program = parse_program("val y : int = x + 15 * 1000").unwrap();
    assert_eq!(
        program,
        vec![ast::Decl {
            pat: ast::Pattern::Ident("y".to_string()),
            ty: Some(ast::Type::Int),
            expr: ast::Expr::Add(
                Box::new(ast::Expr::Ident("x".to_string())),
                Box::new(ast::Expr::Mul(
                    Box::new(ast::Expr::IntConst(15)),
                    Box::new(ast::Expr::IntConst(1000)),
                )),
            ),
        }]
    );
}

#[test]
fn grammar_parses_multiple_decls() {
    let program = parse_program("val x = 1\nval y = x + 1").unwrap();
    assert_eq!(program.len(), 2);
}

#[test]
fn grammar_rejects_garbage() {
    assert!(parse_program("val = 1").is_err());
}

#[test]
fn pretty_prints_without_type_annotation() {
    let program = parse_program("val x = 15 * 10").unwrap();
    assert_eq!(pretty::pretty_print(&program), "val x = 15 * 10");
}

#[test]
fn pretty_prints_with_type_annotation_and_precedence() {
    let program = parse_program("val y : int = x + 15 * 1000").unwrap();
    assert_eq!(pretty::pretty_print(&program), "val y : int = x + 15 * 1000");
}

#[test]
fn pretty_prints_multiple_decls_on_separate_lines() {
    let program = parse_program("val x = 1\nval y = x + 1").unwrap();
    assert_eq!(pretty::pretty_print(&program), "val x = 1\nval y = x + 1");
}

#[test]
fn pretty_prints_necessary_parens_round_trip() {
    // Left-associative nesting needs no parens...
    assert_eq!(
        pretty::pretty_print(&parse_program("val x = 1 + 2 + 3").unwrap()),
        "val x = 1 + 2 + 3"
    );
    // ...but the same shape on the right of a same-precedence op does, since
    // without them it would reparse as ((1 + 2) + 3) instead.
    assert_eq!(
        pretty::pretty_print(&parse_program("val x = 1 + (2 + 3)").unwrap()),
        "val x = 1 + (2 + 3)"
    );
    // A lower-precedence child needs parens on either side of a `*`.
    assert_eq!(
        pretty::pretty_print(&parse_program("val x = (1 + 2) * 3").unwrap()),
        "val x = (1 + 2) * 3"
    );
    assert_eq!(
        pretty::pretty_print(&parse_program("val x = 3 * (1 + 2)").unwrap()),
        "val x = 3 * (1 + 2)"
    );
    // A same-precedence op nested on the right (only reachable via explicit parens,
    // since * is left-associative) keeps its parens too, for the same reason as +.
    assert_eq!(
        pretty::pretty_print(&parse_program("val x = (1 + 2) * (3 * 4)").unwrap()),
        "val x = (1 + 2) * (3 * 4)"
    );
    // But on the left, same-precedence nesting is redundant and doesn't reappear.
    assert_eq!(
        pretty::pretty_print(&parse_program("val x = (3 * 4) * (1 + 2)").unwrap()),
        "val x = 3 * 4 * (1 + 2)"
    );
}

#[test]
fn pretty_prints_minus_div_mod() {
    assert_eq!(
        pretty::pretty_print(&parse_program("val x = 10 - 6 div 2 mod 4").unwrap()),
        "val x = 10 - 6 div 2 mod 4"
    );
    // Sub is not commutative, so unlike +, right-side same-precedence nesting needs
    // parens even when it's Sub nested under Add (mixed ops, same class).
    assert_eq!(
        pretty::pretty_print(&parse_program("val x = 1 - (2 + 3)").unwrap()),
        "val x = 1 - (2 + 3)"
    );
}

/// Repeatedly steps `program` to a fully-reduced single value and returns its text.
fn run_to_value(mut program: ast::Program) -> String {
    while let Some(outcome) = stepping::step(&program) {
        program = outcome.program;
    }
    pretty::pretty_print(&program)
}

#[test]
fn stepping_uses_sml_floor_division_not_truncating_division() {
    // Floor semantics: ~7 div 2 = ~4 (not ~3, which truncating division gives),
    // and ~7 mod 2 = 1 (always same sign as the divisor).
    assert_eq!(
        run_to_value(parse_program("val x = ~7 div 2").unwrap()),
        "val x = ~4"
    );
    assert_eq!(
        run_to_value(parse_program("val x = ~7 mod 2").unwrap()),
        "val x = 1"
    );
}

#[test]
fn pretty_prints_unary_minus() {
    // Binds tighter than * and needs no parens around a bare atom.
    assert_eq!(
        pretty::pretty_print(&parse_program("val x = ~2 * 3").unwrap()),
        "val x = ~2 * 3"
    );
    // Applied to a compound expression, it needs parens to reparse correctly.
    assert_eq!(
        pretty::pretty_print(&parse_program("val x = ~(1 + 2)").unwrap()),
        "val x = ~(1 + 2)"
    );
    // Double negation round-trips without needing parens between the two `~`s.
    assert_eq!(
        pretty::pretty_print(&parse_program("val x = ~~5").unwrap()),
        "val x = ~~5"
    );
}

#[test]
fn stepping_reduces_unary_minus() {
    assert_eq!(run_to_value(parse_program("val x = ~5").unwrap()), "val x = ~5");
    assert_eq!(run_to_value(parse_program("val x = ~~5").unwrap()), "val x = 5");
    assert_eq!(
        run_to_value(parse_program("val x = ~(2 + 3)").unwrap()),
        "val x = ~5"
    );
}

#[test]
fn pretty_prints_if_then_else_bare_at_top_level() {
    assert_eq!(
        pretty::pretty_print(&parse_program("val x = if true then 1 else 2").unwrap()),
        "val x = if true then 1 else 2"
    );
}

#[test]
fn pretty_prints_if_needs_parens_as_an_operand() {
    // As the left operand of +, needed: without parens this would misparse (the
    // else-branch would swallow "+ 1" instead of it applying to the whole if).
    assert_eq!(
        pretty::pretty_print(&parse_program("val x = (if true then 1 else 2) + 1").unwrap()),
        "val x = (if true then 1 else 2) + 1"
    );
    // As the right operand too (conservatively parenthesized even though this
    // particular case would technically round-trip bare).
    assert_eq!(
        pretty::pretty_print(&parse_program("val x = 1 + (if true then 1 else 2)").unwrap()),
        "val x = 1 + (if true then 1 else 2)"
    );
}

#[test]
fn pretty_prints_else_if_chain_without_extra_parens() {
    let program = parse_program("val x = if true then 1 else if false then 2 else 3").unwrap();
    assert_eq!(
        pretty::pretty_print(&program),
        "val x = if true then 1 else if false then 2 else 3"
    );
}

#[test]
fn stepping_reduces_condition_before_selecting_a_branch() {
    // The condition is itself a not-yet-reduced if-expression, so it must be
    // stepped to a bool before either branch can be selected.
    let program =
        parse_program("val x = if (if true then true else false) then 1 else 2").unwrap();

    let step1 = stepping::step(&program).unwrap();
    assert_eq!(step1.message, "Evaluated if true then true else false to true");
    assert_eq!(pretty::pretty_print(&step1.program), "val x = if true then 1 else 2");

    let step2 = stepping::step(&step1.program).unwrap();
    assert_eq!(step2.message, "Evaluated if true then 1 else 2 to 1");
    assert_eq!(pretty::pretty_print(&step2.program), "val x = 1");
}

#[test]
fn stepping_discards_the_untaken_branch_unevaluated() {
    // If the else-branch were mistakenly evaluated first, this would take two
    // steps (reduce 2 + 3, then select the then-branch) instead of one.
    let program = parse_program("val x = if true then 1 else 2 + 3").unwrap();
    let step1 = stepping::step(&program).unwrap();
    assert_eq!(pretty::pretty_print(&step1.program), "val x = 1");
    assert!(stepping::step(&step1.program).is_none());
}

#[test]
fn stepping_if_substitutes_through_to_its_condition() {
    let program = parse_program("val b = true\nval x = if b then 1 else 2").unwrap();

    let step1 = stepping::step(&program).unwrap();
    assert_eq!(step1.message, "Substituted b = true");

    let step2 = stepping::step(&step1.program).unwrap();
    assert_eq!(step2.message, "Evaluated if true then 1 else 2 to 1");
    assert_eq!(pretty::pretty_print(&step2.program), "val x = 1");
}

#[test]
fn highlight_next_nests_yellow_around_a_green_substituted_condition() {
    use pretty::{GREEN_END, GREEN_START, HIGHLIGHT_END, HIGHLIGHT_START};

    let program = parse_program("val b = true\nval x = if b then 1 else 2").unwrap();
    let step1 = stepping::step(&program).unwrap();
    let next = stepping::highlight_next(&step1.program).unwrap();
    assert_eq!(
        pretty::pretty_print(&next),
        format!(
            "val x = {HIGHLIGHT_START}if {GREEN_START}true{GREEN_END} then 1 else 2{HIGHLIGHT_END}"
        )
    );
}

#[test]
fn grammar_parses_all_comparison_operators() {
    let cases = [
        ("val x = 1 = 2", ast::Expr::Eq as fn(_, _) -> _),
        ("val x = 1 <> 2", ast::Expr::Ne),
        ("val x = 1 < 2", ast::Expr::Lt),
        ("val x = 1 <= 2", ast::Expr::Le),
        ("val x = 1 > 2", ast::Expr::Gt),
        ("val x = 1 >= 2", ast::Expr::Ge),
    ];
    for (src, ctor) in cases {
        assert_eq!(
            parse_program(src).unwrap()[0].expr,
            ctor(
                Box::new(ast::Expr::IntConst(1)),
                Box::new(ast::Expr::IntConst(2))
            ),
            "parsing {src:?}"
        );
    }
}

#[test]
fn grammar_comparisons_bind_looser_than_arithmetic() {
    // 1 + 2 < 3 * 4 should compare the two arithmetic results, not e.g. try to add
    // 2 to a bool.
    let program = parse_program("val x = 1 + 2 < 3 * 4").unwrap();
    assert_eq!(
        program[0].expr,
        ast::Expr::Lt(
            Box::new(ast::Expr::Add(
                Box::new(ast::Expr::IntConst(1)),
                Box::new(ast::Expr::IntConst(2)),
            )),
            Box::new(ast::Expr::Mul(
                Box::new(ast::Expr::IntConst(3)),
                Box::new(ast::Expr::IntConst(4)),
            )),
        )
    );
}

#[test]
fn grammar_rejects_chained_comparisons() {
    // Nonassociative: "a < b < c" isn't meaningful (a < b is a bool, < wants ints),
    // so the grammar should reject it outright rather than silently pick a grouping.
    assert!(parse_program("val x = 1 < 2 < 3").is_err());
}

#[test]
fn grammar_parenthesized_comparisons_can_still_nest() {
    // Parens sidestep the nonassoc restriction; nothing stops this syntactically
    // even though it wouldn't type-check in real SML.
    let program = parse_program("val x = (1 < 2) = (3 < 4)").unwrap();
    assert_eq!(
        program[0].expr,
        ast::Expr::Eq(
            Box::new(ast::Expr::Lt(
                Box::new(ast::Expr::IntConst(1)),
                Box::new(ast::Expr::IntConst(2)),
            )),
            Box::new(ast::Expr::Lt(
                Box::new(ast::Expr::IntConst(3)),
                Box::new(ast::Expr::IntConst(4)),
            )),
        )
    );
}

#[test]
fn pretty_prints_comparisons_bare_at_top_level() {
    assert_eq!(
        pretty::pretty_print(&parse_program("val x = 1 + 2 < 3 * 4").unwrap()),
        "val x = 1 + 2 < 3 * 4"
    );
}

#[test]
fn pretty_prints_parenthesized_comparison_nesting() {
    let program = parse_program("val x = (1 < 2) = (3 < 4)").unwrap();
    assert_eq!(
        pretty::pretty_print(&program),
        "val x = (1 < 2) = (3 < 4)"
    );
}

#[test]
fn stepping_reduces_each_comparison_operator() {
    let cases = [
        ("val x = 1 = 1", "true"),
        ("val x = 1 = 2", "false"),
        ("val x = 1 <> 2", "true"),
        ("val x = 2 < 3", "true"),
        ("val x = 3 < 3", "false"),
        ("val x = 3 <= 3", "true"),
        ("val x = 4 > 3", "true"),
        ("val x = 3 >= 3", "true"),
    ];
    for (src, expected) in cases {
        assert_eq!(
            run_to_value(parse_program(src).unwrap()),
            format!("val x = {expected}"),
            "stepping {src:?}"
        );
    }
}

#[test]
fn stepping_comparison_operands_reduce_before_comparing() {
    let program = parse_program("val x = 1 + 2 < 10 div 2").unwrap();
    let step1 = stepping::step(&program).unwrap();
    assert_eq!(step1.message, "Evaluated 1 + 2 to 3");
    let step2 = stepping::step(&step1.program).unwrap();
    assert_eq!(step2.message, "Evaluated 10 div 2 to 5");
    let step3 = stepping::step(&step2.program).unwrap();
    assert_eq!(step3.message, "Evaluated 3 < 5 to true");
    assert_eq!(pretty::pretty_print(&step3.program), "val x = true");
}

#[test]
fn stepping_comparison_feeds_an_if_condition() {
    let program = parse_program("val x = if 3 < 5 then 1 else 2").unwrap();
    let step1 = stepping::step(&program).unwrap();
    assert_eq!(step1.message, "Evaluated 3 < 5 to true");
    let step2 = stepping::step(&step1.program).unwrap();
    assert_eq!(step2.message, "Evaluated if true then 1 else 2 to 1");
    assert_eq!(pretty::pretty_print(&step2.program), "val x = 1");
}

#[test]
fn grammar_andalso_binds_tighter_than_orelse() {
    let program = parse_program("val x = true orelse false andalso false").unwrap();
    assert_eq!(
        program[0].expr,
        ast::Expr::OrElse(
            Box::new(ast::Expr::BoolConst(true)),
            Box::new(ast::Expr::AndAlso(
                Box::new(ast::Expr::BoolConst(false)),
                Box::new(ast::Expr::BoolConst(false)),
            )),
        )
    );
}

#[test]
fn grammar_andalso_orelse_are_right_associative() {
    let program = parse_program("val x = true andalso true andalso false").unwrap();
    assert_eq!(
        program[0].expr,
        ast::Expr::AndAlso(
            Box::new(ast::Expr::BoolConst(true)),
            Box::new(ast::Expr::AndAlso(
                Box::new(ast::Expr::BoolConst(true)),
                Box::new(ast::Expr::BoolConst(false)),
            )),
        )
    );
}

#[test]
fn grammar_andalso_binds_looser_than_comparisons() {
    let program = parse_program("val x = 1 < 2 andalso 3 < 4").unwrap();
    assert_eq!(
        program[0].expr,
        ast::Expr::AndAlso(
            Box::new(ast::Expr::Lt(
                Box::new(ast::Expr::IntConst(1)),
                Box::new(ast::Expr::IntConst(2)),
            )),
            Box::new(ast::Expr::Lt(
                Box::new(ast::Expr::IntConst(3)),
                Box::new(ast::Expr::IntConst(4)),
            )),
        )
    );
}

#[test]
fn pretty_prints_andalso_orelse_chain_bare() {
    assert_eq!(
        pretty::pretty_print(&parse_program("val x = true andalso true andalso false").unwrap()),
        "val x = true andalso true andalso false"
    );
    assert_eq!(
        pretty::pretty_print(&parse_program("val x = true orelse false andalso false").unwrap()),
        "val x = true orelse false andalso false"
    );
}

#[test]
fn pretty_prints_andalso_needs_parens_when_left_nested() {
    // Right-associative, so unlike +, it's the LEFT side that needs parens when a
    // same-precedence op is nested there (only reachable via explicit parens).
    let program = parse_program("val x = (true andalso true) andalso false").unwrap();
    assert_eq!(
        pretty::pretty_print(&program),
        "val x = (true andalso true) andalso false"
    );
}

#[test]
fn stepping_andalso_short_circuits_false_without_evaluating_right() {
    // If the right side were mistakenly evaluated, this would take two steps
    // (reduce 1 + 1 = 2, then short-circuit) instead of one.
    let program = parse_program("val x = false andalso (1 + 1 = 2)").unwrap();
    let step1 = stepping::step(&program).unwrap();
    assert_eq!(step1.message, "Evaluated false andalso 1 + 1 = 2 to false");
    assert_eq!(pretty::pretty_print(&step1.program), "val x = false");
    assert!(stepping::step(&step1.program).is_none());
}

#[test]
fn stepping_orelse_short_circuits_true_without_evaluating_right() {
    let program = parse_program("val x = true orelse (1 + 1 = 2)").unwrap();
    let step1 = stepping::step(&program).unwrap();
    assert_eq!(step1.message, "Evaluated true orelse 1 + 1 = 2 to true");
    assert_eq!(pretty::pretty_print(&step1.program), "val x = true");
    assert!(stepping::step(&step1.program).is_none());
}

#[test]
fn stepping_andalso_evaluates_right_when_left_is_true() {
    assert_eq!(
        run_to_value(parse_program("val x = true andalso (1 < 2)").unwrap()),
        "val x = true"
    );
}

#[test]
fn stepping_orelse_evaluates_right_when_left_is_false() {
    assert_eq!(
        run_to_value(parse_program("val x = false orelse (1 < 2)").unwrap()),
        "val x = true"
    );
}

#[test]
fn grammar_parses_let_in_end() {
    let program = parse_program("val y = let val x = 1 in x + 1 end").unwrap();
    assert_eq!(
        program[0].expr,
        ast::Expr::Let(
            vec![ast::Decl {
                pat: ast::Pattern::Ident("x".to_string()),
                ty: None,
                expr: ast::Expr::IntConst(1),
            }],
            Box::new(ast::Expr::Add(
                Box::new(ast::Expr::Ident("x".to_string())),
                Box::new(ast::Expr::IntConst(1)),
            )),
        )
    );
}

#[test]
fn grammar_let_supports_multiple_decls() {
    let program = parse_program("val y = let val x = 1 val z = 2 in x + z end").unwrap();
    let ast::Expr::Let(decls, _) = &program[0].expr else {
        panic!("expected a Let");
    };
    assert_eq!(decls.len(), 2);
}

#[test]
fn pretty_prints_let_single_line() {
    assert_eq!(
        pretty::pretty_print(&parse_program("val y = let val x = 1 in x + 1 end").unwrap()),
        "val y = let val x = 1 in x + 1 end"
    );
}

#[test]
fn pretty_prints_empty_let() {
    assert_eq!(
        pretty::pretty_print(&parse_program("val y = let in 5 end").unwrap()),
        "val y = let in 5 end"
    );
}

#[test]
fn pretty_prints_let_as_operand_without_parens() {
    // Bracketed by let/end, so unlike `if` it's atomic from the outside: no parens
    // needed even nested inside an arithmetic expression.
    assert_eq!(
        pretty::pretty_print(&parse_program("val y = 1 + let val x = 1 in x end").unwrap()),
        "val y = 1 + let val x = 1 in x end"
    );
}

#[test]
fn stepping_let_reduces_decl_substitutes_then_collapses() {
    let program = parse_program("val y = let val x = 1 + 1 in x * 10 end").unwrap();

    let step1 = stepping::step(&program).unwrap();
    assert_eq!(step1.message, "Evaluated 1 + 1 to 2");
    assert_eq!(
        pretty::pretty_print(&step1.program),
        "val y = let val x = 2 in x * 10 end"
    );

    let step2 = stepping::step(&step1.program).unwrap();
    assert_eq!(step2.message, "Substituted x = 2");
    assert_eq!(
        pretty::pretty_print(&step2.program),
        format!(
            "val y = {}2{} * 10",
            pretty::GREEN_START,
            pretty::GREEN_END
        )
    );

    let step3 = stepping::step(&step2.program).unwrap();
    assert_eq!(step3.message, "Evaluated 2 * 10 to 20");
    assert_eq!(pretty::pretty_print(&step3.program), "val y = 20");

    assert!(stepping::step(&step3.program).is_none());
}

#[test]
fn stepping_let_shadowing_protects_inner_rebinding_from_outer_substitution() {
    // If a substitution from outside the let naively touched the let's own body
    // (ignoring that its `val x` rebinds the name), this would wrongly reach 6
    // (1 + 5, using the *outer* x) instead of 11 (10 + 1, using the *inner* x).
    let program = parse_program("val x = 5\nval y = let val x = 10 in x + 1 end").unwrap();

    let step1 = stepping::step(&program).unwrap();
    assert_eq!(step1.message, "Substituted x = 5");
    // Nothing to substitute: the let's own `x` shadows the outer one throughout.
    assert_eq!(
        pretty::pretty_print(&step1.program),
        "val y = let val x = 10 in x + 1 end"
    );

    assert_eq!(run_to_value(step1.program), "val y = 11");
}

#[test]
fn stepping_top_level_redeclaration_shadows_earlier_binding() {
    // Same shadowing rule as the let case above, at the top level: y's `x` must
    // refer to the *second* (nearer) declaration, giving 11, not 6.
    let program = parse_program("val x = 5\nval x = 10\nval y = x + 1").unwrap();
    assert_eq!(run_to_value(program), "val y = 11");
}

#[test]
fn stepping_full_worked_example_from_project_origin() {
    // The exact example that originally motivated this project: nested let with
    // shadowing, substitution across several decls, and a final comparison.
    let program = parse_program(
        "val x = 15 * 10
         val y = x + 15 * 1000
         val z =
             let
                 val x = 10
             in
                 y < x
             end",
    )
    .unwrap();
    assert_eq!(run_to_value(program), "val z = false");
}

#[test]
fn enter_formula_reports_parse_errors() {
    assert!(enter_formula("val = 1").starts_with("Parse error:"));
}

#[test]
fn enter_step_current_render_round_trip() {
    use pretty::{HIGHLIGHT_END, HIGHLIGHT_START};

    // enter_formula already shows what Step is about to do.
    assert_eq!(
        enter_formula("val x = 15 * 10"),
        format!("val x = {HIGHLIGHT_START}15 * 10{HIGHLIGHT_END}")
    );
    assert_eq!(step_formula(), "Evaluated 15 * 10 to 150");
    // Fully reduced: nothing left to highlight.
    assert_eq!(current_render(), "val x = 150");
    assert_eq!(step_formula(), "No more steps.");
}

/// Helper: pretty-print `program` with `highlight_next`'s target marked (or plain
/// text if there's nothing left to highlight).
fn render_next(program: &ast::Program) -> String {
    match stepping::highlight_next(program) {
        Some(highlighted) => pretty::pretty_print(&highlighted),
        None => pretty::pretty_print(program),
    }
}

#[test]
fn stepping_reduces_arithmetic_then_substitutes_then_finishes() {
    use pretty::{GREEN_END, GREEN_START, HIGHLIGHT_END, HIGHLIGHT_START};

    let mut program = parse_program("val x = 15 * 10\nval y = x + 15 * 1000").unwrap();
    assert_eq!(
        render_next(&program),
        format!("val x = {HIGHLIGHT_START}15 * 10{HIGHLIGHT_END}\nval y = x + 15 * 1000")
    );

    let step1 = stepping::step(&program).unwrap();
    assert_eq!(step1.message, "Evaluated 15 * 10 to 150");
    assert_eq!(pretty::pretty_print(&step1.program), "val x = 150\nval y = x + 15 * 1000");
    program = step1.program;
    assert_eq!(
        render_next(&program),
        format!("val x = {HIGHLIGHT_START}150{HIGHLIGHT_END}\nval y = x + 15 * 1000")
    );

    let step2 = stepping::step(&program).unwrap();
    assert_eq!(step2.message, "Substituted x = 150");
    assert_eq!(
        pretty::pretty_print(&step2.program),
        format!("val y = {GREEN_START}150{GREEN_END} + 15 * 1000")
    );
    program = step2.program;
    // Green from the substitution is still present, and highlight_next also finds
    // the next redex (15 * 1000) alongside it — the two are disjoint here, so they
    // render as separate, non-nested regions in the same line.
    assert_eq!(
        render_next(&program),
        format!("val y = {GREEN_START}150{GREEN_END} + {HIGHLIGHT_START}15 * 1000{HIGHLIGHT_END}")
    );

    let step3 = stepping::step(&program).unwrap();
    assert_eq!(step3.message, "Evaluated 15 * 1000 to 15000");
    assert_eq!(pretty::pretty_print(&step3.program), "val y = 150 + 15000");
    program = step3.program;
    assert_eq!(
        render_next(&program),
        format!("val y = {HIGHLIGHT_START}150 + 15000{HIGHLIGHT_END}")
    );

    let step4 = stepping::step(&program).unwrap();
    assert_eq!(step4.message, "Evaluated 150 + 15000 to 15150");
    assert_eq!(pretty::pretty_print(&step4.program), "val y = 15150");
    program = step4.program;
    assert_eq!(render_next(&program), "val y = 15150");

    assert!(stepping::step(&program).is_none());
}

#[test]
fn pretty_print_renders_a_highlighted_node_wrapped_in_sentinels() {
    use pretty::{HIGHLIGHT_END, HIGHLIGHT_START};

    let program = vec![ast::Decl {
        pat: ast::Pattern::Ident("x".to_string()),
        ty: None,
        expr: ast::Expr::Add(
            Box::new(ast::Expr::IntConst(1)),
            Box::new(ast::Expr::Highlighted(
                Box::new(ast::Expr::IntConst(2)),
                ast::HighlightColor::Yellow,
            )),
        ),
    }];
    assert_eq!(
        pretty::pretty_print(&program),
        format!("val x = 1 + {HIGHLIGHT_START}2{HIGHLIGHT_END}")
    );
}

#[test]
fn substitution_marks_every_occurrence_green_then_clears_on_next_step() {
    use pretty::{GREEN_END, GREEN_START};

    let program = parse_program("val x = 5\nval y = x + x").unwrap();

    let step1 = stepping::step(&program).unwrap();
    assert_eq!(step1.message, "Substituted x = 5");
    assert_eq!(
        pretty::pretty_print(&step1.program),
        format!("val y = {GREEN_START}5{GREEN_END} + {GREEN_START}5{GREEN_END}")
    );
    // Both operands count as values through their green wrappers, so the whole Add
    // is the next redex: yellow nests around the two green-marked 5s.
    use pretty::{HIGHLIGHT_END, HIGHLIGHT_START};
    let next = stepping::highlight_next(&step1.program).unwrap();
    assert_eq!(
        pretty::pretty_print(&next),
        format!(
            "val y = {HIGHLIGHT_START}{GREEN_START}5{GREEN_END} + {GREEN_START}5{GREEN_END}{HIGHLIGHT_END}"
        )
    );

    let step2 = stepping::step(&step1.program).unwrap();
    assert_eq!(step2.message, "Evaluated 5 + 5 to 10");
    assert_eq!(pretty::pretty_print(&step2.program), "val y = 10");
}

#[test]
fn grammar_parses_tuples() {
    let program = parse_program("val x = (1, true, 2 + 3)").unwrap();
    assert_eq!(
        program[0].expr,
        ast::Expr::Tuple(vec![
            ast::Expr::IntConst(1),
            ast::Expr::BoolConst(true),
            ast::Expr::Add(Box::new(ast::Expr::IntConst(2)), Box::new(ast::Expr::IntConst(3))),
        ])
    );
}

#[test]
fn grammar_single_parenthesized_expr_is_grouping_not_a_tuple() {
    let program = parse_program("val x = (1 + 2)").unwrap();
    assert_eq!(
        program[0].expr,
        ast::Expr::Add(Box::new(ast::Expr::IntConst(1)), Box::new(ast::Expr::IntConst(2)))
    );
}

#[test]
fn pretty_prints_tuples() {
    let program = parse_program("val x = (1, true, 2 + 3)").unwrap();
    assert_eq!(pretty::pretty_print(&program), "val x = (1, true, 2 + 3)");
}

#[test]
fn stepping_reduces_tuple_elements_left_to_right_then_stops() {
    let program = parse_program("val x = (1 + 1, 2 + 2)").unwrap();

    let step1 = stepping::step(&program).unwrap();
    assert_eq!(step1.message, "Evaluated 1 + 1 to 2");
    assert_eq!(pretty::pretty_print(&step1.program), "val x = (2, 2 + 2)");

    let step2 = stepping::step(&step1.program).unwrap();
    assert_eq!(step2.message, "Evaluated 2 + 2 to 4");
    assert_eq!(pretty::pretty_print(&step2.program), "val x = (2, 4)");

    // A tuple of values is itself a value: nothing left to do.
    assert_eq!(stepping::step(&step2.program), None);
}

#[test]
fn highlight_next_finds_the_first_unreduced_tuple_element() {
    use pretty::{HIGHLIGHT_END, HIGHLIGHT_START};

    let program = parse_program("val x = (1 + 1, 2 + 2)").unwrap();
    let next = stepping::highlight_next(&program).unwrap();
    assert_eq!(
        pretty::pretty_print(&next),
        format!("val x = ({HIGHLIGHT_START}1 + 1{HIGHLIGHT_END}, 2 + 2)")
    );
}

#[test]
fn typecheck_accepts_a_well_typed_program() {
    let program = parse_program("val x = 1\nval y = x + 1").unwrap();
    assert_eq!(typecheck(&program), None);
}

#[test]
fn typecheck_rejects_a_declared_type_mismatch() {
    let program = parse_program("val x : bool = 1 + 2").unwrap();
    assert_eq!(
        typecheck(&program),
        Some("Expected Bool but got type Int".to_string())
    );
}

#[test]
fn typecheck_rejects_an_unbound_identifier() {
    let program = parse_program("val x = y + 1").unwrap();
    assert_eq!(typecheck(&program), Some("Unbound identifier: y".to_string()));
}

#[test]
fn typecheck_later_decls_see_earlier_bindings_but_not_the_reverse() {
    // `x` is visible to `y`, matching evaluation order, but declaring them in the
    // other order should fail: `y` isn't in scope yet when `x` is checked.
    assert_eq!(typecheck(&parse_program("val x = 1\nval y = x + 1").unwrap()), None);
    assert_eq!(
        typecheck(&parse_program("val y = x + 1\nval x = 1").unwrap()),
        Some("Unbound identifier: x".to_string())
    );
}

#[test]
fn typecheck_top_level_redeclaration_shadows_for_later_decls() {
    let program = parse_program("val x = 1\nval x = true\nval y = x").unwrap();
    // `y` should see the second (bool) `x`, not the first (int) one.
    assert_eq!(typecheck(&program), None);
}

#[test]
fn typecheck_if_requires_a_bool_condition() {
    let program = parse_program("val x = if 1 then 2 else 3").unwrap();
    assert_eq!(
        typecheck(&program),
        Some("Expected Bool but got type Int".to_string())
    );
}

#[test]
fn typecheck_if_requires_both_branches_to_match() {
    let program = parse_program("val x = if true then 1 else false").unwrap();
    assert_eq!(
        typecheck(&program),
        Some("Expected Int but got type Bool".to_string())
    );
}

#[test]
fn typecheck_if_of_matching_branches_infers_their_type() {
    let program = parse_program("val x : int = if true then 1 else 2").unwrap();
    assert_eq!(typecheck(&program), None);
}

#[test]
fn typecheck_let_bindings_are_visible_in_the_body_but_not_after() {
    let program = parse_program("val x = let val y = 1 in y + 1 end").unwrap();
    assert_eq!(typecheck(&program), None);

    let leaks = parse_program("val x = (let val y = 1 in y end) + y").unwrap();
    assert_eq!(typecheck(&leaks), Some("Unbound identifier: y".to_string()));
}

#[test]
fn typecheck_let_binding_can_shadow_an_outer_one_with_a_different_type() {
    let program =
        parse_program("val x = true\nval y = (let val x = 1 in x + 1 end)").unwrap();
    assert_eq!(typecheck(&program), None);
}

#[test]
fn typecheck_infers_a_tuple_as_a_product_type() {
    let program = parse_program("val x = (1, true, 2 + 3)").unwrap();
    assert_eq!(typecheck(&program), None);
}

#[test]
fn typecheck_rejects_mismatched_tuple_shapes_between_if_branches() {
    // Exercises `Type::Product` comparison the same way
    // `typecheck_if_requires_both_branches_to_match` does for scalars: two
    // differently-shaped tuples meeting at an `if`.
    let program = parse_program("val x = if true then (1, true) else (1, 2)").unwrap();
    assert!(matches!(typecheck(&program), Some(msg) if msg.starts_with("Expected")));
}

#[test]
fn grammar_parses_a_product_type_annotation() {
    let program = parse_program("val x : int * bool = (1, true)").unwrap();
    assert_eq!(
        program[0].ty,
        Some(ast::Type::Product(vec![
            Box::new(ast::Type::Int),
            Box::new(ast::Type::Bool),
        ]))
    );
}

#[test]
fn grammar_parses_a_product_type_with_more_than_two_elements() {
    // Left-recursive `TypeProduct`, mirroring `TupleItems`: each `*` should push
    // onto the same flat `Vec` rather than nesting.
    let program = parse_program("val x : int * bool * int = (1, true, 2)").unwrap();
    assert_eq!(
        program[0].ty,
        Some(ast::Type::Product(vec![
            Box::new(ast::Type::Int),
            Box::new(ast::Type::Bool),
            Box::new(ast::Type::Int),
        ]))
    );
}

#[test]
fn pretty_prints_a_product_type_annotation() {
    let program = parse_program("val x : int * bool = (1, true)").unwrap();
    assert_eq!(pretty::pretty_print(&program), "val x : int * bool = (1, true)");
}

#[test]
fn typecheck_accepts_a_matching_product_type_annotation() {
    let program = parse_program("val x : int * bool * int = (1, true, 2 + 3)").unwrap();
    assert_eq!(typecheck(&program), None);
}

#[test]
fn typecheck_rejects_a_tuple_arity_mismatch_against_its_annotation() {
    let program = parse_program("val x : int * bool = (1, true, 2)").unwrap();
    assert_eq!(
        typecheck(&program),
        Some(
            "Expected Product([Int, Bool]) but got type Product([Int, Bool, Int])".to_string()
        )
    );
}

#[test]
fn grammar_parenthesized_type_is_just_grouping() {
    let program = parse_program("val x : (int) = 1").unwrap();
    assert_eq!(program[0].ty, Some(ast::Type::Int));
}

#[test]
fn grammar_parens_let_a_type_nest_inside_a_product() {
    let program = parse_program("val x : (int * bool) * int = ((1, true), 2)").unwrap();
    assert_eq!(
        program[0].ty,
        Some(ast::Type::Product(vec![
            Box::new(ast::Type::Product(vec![
                Box::new(ast::Type::Int),
                Box::new(ast::Type::Bool),
            ])),
            Box::new(ast::Type::Int),
        ]))
    );
}

#[test]
fn pretty_prints_a_parenthesized_type_without_the_redundant_parens() {
    let program = parse_program("val x : (int) = 1").unwrap();
    assert_eq!(pretty::pretty_print(&program), "val x : int = 1");
}

#[test]
fn pretty_prints_a_nested_product_type_with_necessary_parens() {
    // Printed bare, "int * bool * int" would reparse as one flat 3-tuple instead of
    // this 2-tuple whose first component is itself a 2-tuple, so the nested
    // Product must keep its parens to round-trip.
    let program = parse_program("val x : (int * bool) * int = ((1, true), 2)").unwrap();
    assert_eq!(
        pretty::pretty_print(&program),
        "val x : (int * bool) * int = ((1, true), 2)"
    );
    assert_eq!(
        parse_program(&pretty::pretty_print(&program)).unwrap(),
        program
    );
}

#[test]
fn grammar_parses_a_wildcard_pattern() {
    let program = parse_program("val _ = 5").unwrap();
    assert_eq!(program[0].pat, ast::Pattern::Wildcard);
}

#[test]
fn grammar_parses_a_tuple_pattern() {
    let program = parse_program("val (x, y) = (1, 2)").unwrap();
    assert_eq!(
        program[0].pat,
        ast::Pattern::Tuple(vec![
            ast::Pattern::Ident("x".to_string()),
            ast::Pattern::Ident("y".to_string()),
        ])
    );
}

#[test]
fn grammar_parenthesized_pattern_is_just_grouping() {
    let program = parse_program("val (x) = 1").unwrap();
    assert_eq!(program[0].pat, ast::Pattern::Ident("x".to_string()));
}

#[test]
fn grammar_parses_a_nested_tuple_pattern_with_a_wildcard() {
    let program = parse_program("val (x, (y, _)) = (1, (2, 3))").unwrap();
    assert_eq!(
        program[0].pat,
        ast::Pattern::Tuple(vec![
            ast::Pattern::Ident("x".to_string()),
            ast::Pattern::Tuple(vec![
                ast::Pattern::Ident("y".to_string()),
                ast::Pattern::Wildcard,
            ]),
        ])
    );
}

#[test]
fn pretty_prints_wildcard_and_tuple_patterns() {
    assert_eq!(pretty::pretty_print(&parse_program("val _ = 5").unwrap()), "val _ = 5");
    assert_eq!(
        pretty::pretty_print(&parse_program("val (x, _) = (1, 2)").unwrap()),
        "val (x, _) = (1, 2)"
    );
}

#[test]
fn typecheck_tuple_pattern_binds_each_component_its_own_type() {
    let program = parse_program("val (x, y) = (1, true)\nval z = y").unwrap();
    assert_eq!(typecheck(&program), None);

    let bad = parse_program("val (x, y) = (1, true)\nval z = y + 1").unwrap();
    assert_eq!(typecheck(&bad), Some("Expected Int but got type Bool".to_string()));
}

#[test]
fn typecheck_wildcard_matches_anything_and_binds_nothing() {
    let program = parse_program("val _ = true\nval x = 1").unwrap();
    assert_eq!(typecheck(&program), None);
}

#[test]
fn typecheck_rejects_a_tuple_pattern_against_a_non_tuple_type() {
    let program = parse_program("val (x, y) = 5").unwrap();
    assert_eq!(
        typecheck(&program),
        Some("Pattern (x, y) expects a tuple type but got Int".to_string())
    );
}

#[test]
fn typecheck_rejects_a_tuple_pattern_arity_mismatch() {
    let program = parse_program("val (x, y) = (1, 2, 3)").unwrap();
    assert_eq!(
        typecheck(&program),
        Some("Pattern (x, y) has 2 components but its type has 3".to_string())
    );
}

#[test]
fn typecheck_rejects_a_non_linear_pattern() {
    let program = parse_program("val (x, x) = (1, 2)").unwrap();
    assert_eq!(
        typecheck(&program),
        Some("Variable x is bound more than once in this pattern".to_string())
    );
}

#[test]
fn stepping_destructures_a_tuple_pattern_into_multiple_bindings() {
    let program = parse_program("val (x, y) = (1, 2)\nval z = x + y").unwrap();

    let step1 = stepping::step(&program).unwrap();
    assert_eq!(step1.message, "Substituted (x, y) = (1, 2)");
    assert_eq!(
        pretty::pretty_print(&step1.program),
        format!(
            "val z = {}1{} + {}2{}",
            pretty::GREEN_START,
            pretty::GREEN_END,
            pretty::GREEN_START,
            pretty::GREEN_END,
        )
    );

    let step2 = stepping::step(&step1.program).unwrap();
    assert_eq!(step2.message, "Evaluated 1 + 2 to 3");
    assert_eq!(pretty::pretty_print(&step2.program), "val z = 3");
}

#[test]
fn stepping_wildcard_decl_is_consumed_without_substituting_anything() {
    let program = parse_program("val _ = 5\nval x = 1").unwrap();
    let step1 = stepping::step(&program).unwrap();
    assert_eq!(step1.message, "Substituted _ = 5");
    assert_eq!(pretty::pretty_print(&step1.program), "val x = 1");
}

#[test]
fn stepping_tuple_pattern_bindings_shadow_independently() {
    // The tuple decl binds both x and y; the later `val x = 10` shadows only x,
    // not y, so z should end up seeing the *second* x (still free, pending its own
    // step) but the *first* y (already substituted in).
    let program = parse_program("val (x, y) = (1, 2)\nval x = 10\nval z = x + y").unwrap();

    let step1 = stepping::step(&program).unwrap();
    assert_eq!(step1.message, "Substituted (x, y) = (1, 2)");
    assert_eq!(
        pretty::pretty_print(&step1.program),
        format!("val x = 10\nval z = x + {}2{}", pretty::GREEN_START, pretty::GREEN_END)
    );
}

#[test]
fn stepping_let_destructures_a_tuple_pattern() {
    let program = parse_program("val z = let val (x, y) = (1, 2) in x + y end").unwrap();

    let step1 = stepping::step(&program).unwrap();
    assert_eq!(step1.message, "Substituted (x, y) = (1, 2)");
    assert_eq!(
        pretty::pretty_print(&step1.program),
        format!(
            "val z = {}1{} + {}2{}",
            pretty::GREEN_START,
            pretty::GREEN_END,
            pretty::GREEN_START,
            pretty::GREEN_END,
        )
    );

    let step2 = stepping::step(&step1.program).unwrap();
    assert_eq!(step2.message, "Evaluated 1 + 2 to 3");
    assert_eq!(pretty::pretty_print(&step2.program), "val z = 3");
}

#[test]
fn grammar_parses_a_case_expression() {
    let program = parse_program("val x = case 1 of 0 => true | _ => false").unwrap();
    assert_eq!(
        program[0].expr,
        ast::Expr::Match(
            Box::new(ast::Expr::IntConst(1)),
            vec![
                (ast::Pattern::IntConst(0), ast::Expr::BoolConst(true)),
                (ast::Pattern::Wildcard, ast::Expr::BoolConst(false)),
            ],
        )
    );
}

#[test]
fn grammar_case_arm_body_extends_greedily_across_operators() {
    // Like the else-branch of `if`, an arm's body should keep consuming "+ 1"
    // rather than the whole case-expression becoming the left operand of +.
    let program = parse_program("val x = case 1 of _ => 2 + 1").unwrap();
    assert_eq!(
        program[0].expr,
        ast::Expr::Match(
            Box::new(ast::Expr::IntConst(1)),
            vec![(
                ast::Pattern::Wildcard,
                ast::Expr::Add(Box::new(ast::Expr::IntConst(2)), Box::new(ast::Expr::IntConst(1))),
            )],
        )
    );
}

#[test]
fn grammar_nested_case_dangling_bar_attaches_to_the_innermost_case() {
    // Without parens, a trailing "| p3 => c" after a nested case should attach to
    // the *inner* case (shift-preferred, same "nearest wins" rule real SML uses
    // for this — mirrors dangling-else), not float out to the outer one.
    let program = parse_program(
        "val x = case 1 of a => case 2 of b => 10 | c => 20 | d => 30",
    )
    .unwrap();
    assert_eq!(
        program[0].expr,
        ast::Expr::Match(
            Box::new(ast::Expr::IntConst(1)),
            vec![(
                ast::Pattern::Ident("a".to_string()),
                ast::Expr::Match(
                    Box::new(ast::Expr::IntConst(2)),
                    vec![
                        (ast::Pattern::Ident("b".to_string()), ast::Expr::IntConst(10)),
                        (ast::Pattern::Ident("c".to_string()), ast::Expr::IntConst(20)),
                        (ast::Pattern::Ident("d".to_string()), ast::Expr::IntConst(30)),
                    ],
                ),
            )],
        )
    );
}

#[test]
fn grammar_parens_let_a_trailing_bar_attach_to_the_outer_case() {
    // Explicit parens around the inner case override the default "nearest wins"
    // attachment, letting "| c => 20" belong to the outer case instead.
    let program = parse_program(
        "val x = case 1 of a => (case 2 of b => 10) | c => 20",
    )
    .unwrap();
    assert_eq!(
        program[0].expr,
        ast::Expr::Match(
            Box::new(ast::Expr::IntConst(1)),
            vec![
                (
                    ast::Pattern::Ident("a".to_string()),
                    ast::Expr::Match(
                        Box::new(ast::Expr::IntConst(2)),
                        vec![(ast::Pattern::Ident("b".to_string()), ast::Expr::IntConst(10))],
                    ),
                ),
                (ast::Pattern::Ident("c".to_string()), ast::Expr::IntConst(20)),
            ],
        )
    );
}

#[test]
fn pretty_prints_case_bare_at_top_level() {
    assert_eq!(
        pretty::pretty_print(&parse_program("val x = case 1 of 0 => true | _ => false").unwrap()),
        "val x = case 1 of 0 => true | _ => false"
    );
}

#[test]
fn pretty_prints_case_needs_parens_as_an_operand() {
    assert_eq!(
        pretty::pretty_print(
            &parse_program("val x = (case 1 of _ => 1) + 1").unwrap()
        ),
        "val x = (case 1 of _ => 1) + 1"
    );
}

#[test]
fn typecheck_case_requires_every_arm_to_produce_the_same_type() {
    let program = parse_program("val x = case 1 of 0 => true | _ => 1").unwrap();
    assert_eq!(
        typecheck(&program),
        Some("Expected Bool but got type Int".to_string())
    );
}

#[test]
fn typecheck_case_binds_pattern_variables_within_their_own_arm_only() {
    let program = parse_program("val x = case (1, 2) of (a, b) => a + b").unwrap();
    assert_eq!(typecheck(&program), None);

    let leaks = parse_program("val x = (case (1, 2) of (a, b) => a + b) + a").unwrap();
    assert_eq!(typecheck(&leaks), Some("Unbound identifier: a".to_string()));
}

#[test]
fn typecheck_case_scrutinee_and_patterns_must_agree_in_type() {
    let program = parse_program("val x = case true of 0 => 1 | _ => 2").unwrap();
    assert!(matches!(typecheck(&program), Some(msg) if msg.starts_with("Expected")));
}

#[test]
fn stepping_case_reduces_scrutinee_before_selecting_an_arm() {
    let program = parse_program("val x = case 1 + 1 of 2 => 10 | _ => 20").unwrap();

    let step1 = stepping::step(&program).unwrap();
    assert_eq!(step1.message, "Evaluated 1 + 1 to 2");
    assert_eq!(
        pretty::pretty_print(&step1.program),
        "val x = case 2 of 2 => 10 | _ => 20"
    );

    let step2 = stepping::step(&step1.program).unwrap();
    assert_eq!(step2.message, "Substituted 2 = 2");
    assert_eq!(pretty::pretty_print(&step2.program), "val x = 10");
}

#[test]
fn stepping_case_tries_arms_in_order_and_stops_at_the_first_match() {
    let program = parse_program("val x = case 5 of 0 => 1 | _ => 2").unwrap();
    assert_eq!(run_to_value(program), "val x = 2");
}

#[test]
fn stepping_case_binds_pattern_variables_into_the_chosen_arm() {
    use pretty::{GREEN_END, GREEN_START};

    let program = parse_program("val x = case (1, 2) of (a, b) => a + b").unwrap();
    let step1 = stepping::step(&program).unwrap();
    assert_eq!(step1.message, "Substituted (a, b) = (1, 2)");
    assert_eq!(
        pretty::pretty_print(&step1.program),
        format!("val x = {GREEN_START}1{GREEN_END} + {GREEN_START}2{GREEN_END}")
    );

    let step2 = stepping::step(&step1.program).unwrap();
    assert_eq!(step2.message, "Evaluated 1 + 2 to 3");
    assert_eq!(pretty::pretty_print(&step2.program), "val x = 3");
}

#[test]
fn stepping_case_unmatched_arm_variables_are_not_substituted() {
    // Only the chosen arm's bindings should ever reach substitution; a variable
    // with the same name in a discarded arm must not leak in.
    let program = parse_program("val x = case 1 of 1 => 10 | y => y").unwrap();
    assert_eq!(run_to_value(program), "val x = 10");
}

#[test]
#[should_panic(expected = "Match failure")]
fn stepping_case_panics_on_no_matching_arm() {
    let program = parse_program("val x = case 1 of 0 => 10").unwrap();
    stepping::step(&program);
}

#[test]
fn highlight_next_case_highlights_the_scrutinee_then_the_whole_match() {
    use pretty::{HIGHLIGHT_END, HIGHLIGHT_START};

    let program = parse_program("val x = case 1 + 1 of 2 => 10 | _ => 20").unwrap();
    assert_eq!(
        render_next(&program),
        format!("val x = case {HIGHLIGHT_START}1 + 1{HIGHLIGHT_END} of 2 => 10 | _ => 20")
    );

    let step1 = stepping::step(&program).unwrap();
    assert_eq!(
        render_next(&step1.program),
        format!("val x = {HIGHLIGHT_START}case 2 of 2 => 10 | _ => 20{HIGHLIGHT_END}")
    );
}
