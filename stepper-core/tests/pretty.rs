//! Pretty-printing, which is really a round-trip property: printing a program and
//! re-parsing it must reproduce the same AST, so every test here is about
//! parenthesizing exactly as much as that requires and no more.

mod common;

use common::*;
use common::expr_builders::Expr;
use stepper_core::ast::{PatternBase, Type};
use stepper_core::ast::lambda_ids;
use stepper_core::view::HighlightColor;
use stepper_core::pretty::pretty_print;
/// Asserts that `src` prints back as itself, and that the printed form still
/// parses to the same AST.
fn round_trips(src: &str) {
    let program = parse(src);
    let printed = pretty_print(&program);
    assert_eq!(printed, src, "round-trip changed `{src}`");
    assert_eq!(parse(&printed), program, "reparsing `{printed}` changed the AST");
}

/// Asserts that `src` prints as `expected` (they differ when the source has
/// redundant parens or non-canonical spacing), and that `expected` reparses to the
/// same AST.
fn prints_as(src: &str, expected: &str) {
    let program = parse(src);
    assert_eq!(pretty_print(&program), expected, "printing `{src}`");
    assert_eq!(parse(expected), program, "reparsing `{expected}` changed the AST");
}

#[test]
fn prints_decls_with_and_without_annotations() {
    round_trips("val x = 15 * 10");
    round_trips("val y : int = x + 15 * 1000");
    round_trips("val x = 1\nval y = x + 1");
}

#[test]
fn prints_necessary_parens_only() {
    // Left-associative nesting needs no parens...
    round_trips("val x = 1 + 2 + 3");
    // ...but the same shape on the right of a same-precedence op does, since
    // without them it would reparse as ((1 + 2) + 3) instead.
    round_trips("val x = 1 + (2 + 3)");
    // A lower-precedence child needs parens on either side of a `*`.
    round_trips("val x = (1 + 2) * 3");
    round_trips("val x = 3 * (1 + 2)");
    // A same-precedence op nested on the right (only reachable via explicit parens,
    // since * is left-associative) keeps its parens too, for the same reason as +.
    round_trips("val x = (1 + 2) * (3 * 4)");
    // But on the left, same-precedence nesting is redundant and doesn't reappear.
    prints_as("val x = (3 * 4) * (1 + 2)", "val x = 3 * 4 * (1 + 2)");
}

#[test]
fn prints_minus_div_mod() {
    round_trips("val x = 10 - 6 div 2 mod 4");
    // Sub is not commutative, so unlike +, right-side same-precedence nesting needs
    // parens even when it's Sub nested under Add (mixed ops, same class).
    round_trips("val x = 1 - (2 + 3)");
}

#[test]
fn prints_unary_minus() {
    // Binds tighter than * and needs no parens around a bare atom.
    round_trips("val x = ~2 * 3");
    // Applied to a compound expression, it needs parens to reparse correctly.
    round_trips("val x = ~(1 + 2)");
    // Double negation round-trips without needing parens between the two `~`s.
    round_trips("val x = ~~5");
    // SML spells negative literals with `~`, not `-`.
    assert_eq!(
        pretty_print(&vec![val(pvar("x"), Expr::IntConst(-5))]),
        "val x = ~5"
    );
}

#[test]
fn prints_if_bare_at_top_level_but_parenthesized_as_an_operand() {
    round_trips("val x = if true then 1 else 2");
    // As the left operand of +, needed: without parens this would misparse (the
    // else-branch would swallow "+ 1" instead of it applying to the whole if).
    round_trips("val x = (if true then 1 else 2) + 1");
    // As the right operand too (conservatively parenthesized even though this
    // particular case would technically round-trip bare).
    round_trips("val x = 1 + (if true then 1 else 2)");
    // An else-if chain needs no extra parens: `else` already delimits it.
    round_trips("val x = if true then 1 else if false then 2 else 3");
}

#[test]
fn prints_comparisons() {
    round_trips("val x = 1 + 2 < 3 * 4");
    // Nonassociative, so a comparison nested inside another keeps its parens.
    round_trips("val x = (1 < 2) = (3 < 4)");
}

#[test]
fn prints_andalso_orelse() {
    round_trips("val x = true andalso true andalso false");
    round_trips("val x = true orelse false andalso false");
    // Right-associative, so unlike +, it's the LEFT side that needs parens when a
    // same-precedence op is nested there (only reachable via explicit parens).
    round_trips("val x = (true andalso true) andalso false");
}

#[test]
fn prints_let_as_an_atom() {
    round_trips("val y = let val x = 1 in x + 1 end");
    round_trips("val y = let in 5 end");
    // Bracketed by let/end, so unlike `if` it's atomic from the outside: no parens
    // needed even nested inside an arithmetic expression.
    round_trips("val y = 1 + let val x = 1 in x end");
    // Multiple decls stay on one line inside the brackets.
    round_trips("val y = let val x = 1 val z = 2 in x + z end");
}

#[test]
fn prints_tuples_and_patterns() {
    round_trips("val x = (1, true, 2 + 3)");
    round_trips("val _ = 5");
    round_trips("val (x, _) = (1, 2)");
    round_trips("val (x, (y, _)) = (1, (2, 3))");
}

#[test]
fn prints_type_annotations() {
    round_trips("val x : int * bool = (1, true)");
    // Redundant parens around an atom don't come back.
    prints_as("val x : (int) = 1", "val x : int = 1");
    // Printed bare, "int * bool * int" would reparse as one flat 3-tuple instead of
    // this 2-tuple whose first component is itself a 2-tuple, so the nested
    // Product must keep its parens to round-trip.
    round_trips("val x : (int * bool) * int = ((1, true), 2)");
}

#[test]
fn prints_arrow_types() {
    // Right-associative: the result type prints bare, the parameter type doesn't.
    round_trips("val f : int -> int -> int = fn x : int => fn y : int => x");
    round_trips("val f : (int -> int) -> int = fn g : int -> int => g 1");
    // An arrow inside a product genuinely needs its parens (`*` binds tighter).
    round_trips("val p : (int -> int) * bool = (fn x : int => x, true)");
    // A product as an arrow's *parameter* doesn't — `int * int -> bool` already
    // parses as `(int * int) -> bool` — but `pretty_print_type_atom` treats both
    // positions alike and parenthesizes anyway. Harmless (it still reparses to the
    // same type), just not minimal.
    prints_as(
        "val f : int * int -> bool = fn p : int * int => true",
        "val f : (int * int) -> bool = fn p : int * int => true",
    );
}

#[test]
fn prints_an_annotation_wherever_the_pattern_carries_one() {
    round_trips("val (x, y) : int * bool = (1, true)");
    round_trips("val (x : int, y) = (1, true)");
}

#[test]
fn prints_case_bare_at_top_level_but_parenthesized_as_an_operand() {
    round_trips("val x = case 1 of 0 => true | _ => false");
    round_trips("val x = (case 1 of _ => 1) + 1");
    // A nested case as the last arm's body prints bare and simply absorbs any
    // further arms, mirroring the grammar's own shift preference.
    round_trips("val x = case 1 of a => case 2 of b => 10 | c => 20");
}

#[test]
fn prints_lambdas() {
    round_trips("val f = fn x : int => x + 1");
    round_trips("val f = fn n : int => n | 0 => 1");
    // Open-ended like `if`/`case`, so it needs parens the moment it sits inside
    // anything else — as an application operand, most of all.
    round_trips("val z = f (fn x : int => x)");
    round_trips("val y = (fn x : int => x) 3");
}

#[test]
fn prints_val_rec() {
    round_trips("val rec f : int -> int = fn n : int => if n = 0 then 1 else n * f (n - 1)");
    // Inside a `let`, on one line, same as a plain `val`.
    round_trips("val y = let val rec f : int -> int = fn n : int => n in f 1 end");
}

#[test]
fn prints_application() {
    for src in [
        "val z = f x + y",
        "val z = f x y",
        "val z = f (g x)",
        "val z = ~(f x)",
        "val z = f ~5",
        "val z = f (1, 2)",
        "val z = f let val a = 1 in a end",
        "val z = f (if b then 1 else 2)",
        "val z = f (fn x : int => x)",
    ] {
        round_trips(src);
    }
}

#[test]
fn breaks_a_let_over_lines_with_its_keywords_aligned() {
    let program = parse("val y = let val a = 1 val b = 2 in a + b + a + b end");
    // The `let` hugs the `=` rather than being pushed onto a line of its own, and
    // `let`/`in`/`end` line up under each other wherever it landed.
    assert_eq!(
        show_at_width(&program, 40),
        "\
val y = let
          val a = 1
          val b = 2
        in
          a + b + a + b
        end"
    );
}

#[test]
fn breaks_a_case_with_its_bars_under_the_keyword() {
    let program = parse("val x = case n of 0 => 1 | 1 => 2 | m => m * m + m * m");
    assert_eq!(
        show_at_width(&program, 40),
        "\
val x = case n of
          0 => 1
        | 1 => 2
        | m => m * m + m * m"
    );
}

#[test]
fn breaks_an_application_with_its_arguments_aligned() {
    // Applications nest to the left, so a group per `App` node would indent the
    // *last* argument least. The spine is laid out as one group instead, so they
    // all line up.
    let program = parse("val x = foo (a + b) (c + d) (e + f) (g + h)");
    assert_eq!(
        show_at_width(&program, 24),
        "\
val x =
    foo
        (a + b)
        (c + d)
        (e + f)
        (g + h)"
    );
}

#[test]
fn breaking_lines_never_changes_the_program() {
    // The round-trip property has to survive layout: a break only ever replaces a
    // space with a newline, and the lexer treats the two alike. Narrow widths are
    // where every group is forced to break, so this is the strong version of the
    // check the rest of this file makes at full width.
    for src in [
        "val y = let val a = 1 val b = 2 in a + b + a + b end",
        "val f = fn n : int => if n = 0 then 1 else n * f (n - 1)",
        "val x = case n of 0 => 1 | 1 => 2 | m => m * m + m * m",
        "val x = foo (a + b) (c + d) (e + f) (g + h)",
        "val rec f : int -> int = fn n : int => if n = 0 then 1 else n * f (n - 1)",
        "val (a, b) : int * bool = (1 + 2 + 3 + 4 + 5, true andalso false)",
    ] {
        let program = parse(src);
        for width in [8, 16, 24, 40, 80] {
            let printed = show_at_width(&program, width);
            assert_eq!(
                parse(&printed),
                program,
                "at width {width}, `{src}` printed as:\n{printed}"
            );
        }
    }
}

#[test]
fn folds_a_lambda_to_its_parameter() {
    let program = parse("val f = fn x : int => x + 1\nval y = f 2");
    let ids = lambda_ids(&program);
    assert_eq!(ids.len(), 1);
    // The annotation goes with the body: the point of folding is to get the
    // function out of the way, and its type is part of what's in the way.
    assert_eq!(
        show_folded(&program, &ids),
        "val f = fn x => ...\nval y = f 2"
    );
    // Folding is display state, so the same program with an empty fold set prints
    // in full — nothing was consumed.
    assert_eq!(
        show_folded(&program, &[]),
        "val f = fn x : int => x + 1\nval y = f 2"
    );
}

#[test]
fn folding_the_inner_lambda_of_a_curried_function_leaves_the_outer_one() {
    let program = parse("val f = fn x : int => fn y : int => x + y");
    let ids = lambda_ids(&program);
    assert_eq!(ids.len(), 2, "outermost first");
    assert_eq!(show_folded(&program, &ids[1..]), "val f = fn x : int => fn y => ...");
    assert_eq!(show_folded(&program, &ids[..1]), "val f = fn x => ...");
}

#[test]
fn folding_changes_where_lines_break() {
    // Folding isn't a CSS trick: it changes the node's width, so it changes the
    // line-breaking of every group around it. That's why it's read while the
    // document is built rather than applied to the finished output.
    let program = parse("val y = (fn n : int => n + 1 + 1 + 1 + 1 + 1) 2");
    let ids = lambda_ids(&program);
    assert!(show_at_width(&program, 30).contains('\n'));
    let folded = {
        let mut view = stepper_core::view::ViewState::default();
        view.width = 30;
        view.collapsed = ids.iter().copied().collect();
        stepper_core::pretty::pretty_print_marked(&program, &view)
    };
    assert_eq!(folded, "val y = (fn n => ...) 2");
}

#[test]
fn marks_the_node_a_highlight_names() {
    // A highlight is a `NodeId` in the view state, not a node in the tree, so
    // marking one is a matter of naming it — the printer looks each node up as it
    // goes.
    let two = Expr::IntConst(2);
    let id = two.id;
    let program = vec![val(
        pvar("x"),
        Expr::Add(Box::new(Expr::IntConst(1)), Box::new(two)),
    )];
    assert_eq!(
        show_highlighted(&program, id, HighlightColor::Yellow),
        "val x = 1 + [y2y]"
    );
    // The same program with nothing highlighted is just the program: no residue
    // is left in the tree to strip.
    assert_eq!(show(&program), "val x = 1 + 2");
}

#[test]
fn a_highlight_wraps_the_parens_its_node_needs() {
    // Highlighting is invisible to the precedence ladder — it's applied to a node
    // that has already decided how to print itself — so a compound operand still
    // gets the parens it needs, and the marker goes outside them.
    let sum = Expr::Add(
        Box::new(Expr::IntConst(1)),
        Box::new(Expr::IntConst(2)),
    );
    let id = sum.id;
    let program = vec![val(
        pvar("x"),
        Expr::Mul(Box::new(Expr::IntConst(3)), Box::new(sum)),
    )];
    assert_eq!(
        show_highlighted(&program, id, HighlightColor::Green),
        "val x = 3 * [g(1 + 2)g]"
    );
    assert_eq!(parse(&strip_markers(&pretty_print(&program))).len(), 1);
}

#[test]
fn prints_every_pattern_form_in_a_case() {
    // Literal patterns (negative ints included), wildcards, variables and tuples
    // all print in the form they parse from.
    round_trips("val x = case n of ~1 => 0 | true => 1 | (a, b) => a + b | _ => 2");
    assert_eq!(
        pretty_print(&vec![val(
            pat_typed(
                PatternBase::Tuple(vec![pvar("a"), pat(PatternBase::Wildcard)]),
                Type::Product(vec![Box::new(Type::Int), Box::new(Type::Bool)]),
            ),
            Expr::Tuple(vec![Expr::IntConst(1), Expr::BoolConst(true)]),
        )]),
        "val (a, _) : int * bool = (1, true)"
    );
}

#[test]
fn prints_a_fun_as_the_val_it_elaborates_to() {
    // `fun` is gone by the time there's an AST to print (see
    // `frontend::elaborate`), so what comes back is the `val`/`val rec` it stands
    // for — which is the point in a stepper: the desugaring is on screen.
    prints_as("fun f (x : int) = x + 1", "val f = fn x : int => x + 1");
    prints_as(
        "fun fact (n : int) : int = if n = 0 then 1 else n * fact (n - 1)",
        "val rec fact : int -> int = fn n : int => if n = 0 then 1 else n * fact (n - 1)",
    );
    prints_as(
        "fun add (x : int) (y : int) : int = x + y",
        "val add : int -> int -> int = fn x : int => fn y : int => x + y",
    );
    prints_as(
        "fun g (0 : int) (y : int) = y | g (x : int) (y : int) = x * y",
        "val g = fn argA : int => fn argB : int \
         => case (argA, argB) of (0 : int, y : int) => y | (x : int, y : int) => x * y",
    );
}

#[test]
fn prints_a_parenthesized_annotated_pattern_without_its_parens() {
    // `(x : int)` and `x : int` are the same pattern; only `fun` parameters need
    // the parens, and nothing printed here is one.
    prints_as("val (x : int) = 5", "val x : int = 5");
}
