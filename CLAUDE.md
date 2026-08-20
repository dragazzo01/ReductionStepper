# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project overview

Reduction Stepper is a small-step operational semantics visualizer for a subset of SML. A `stepper-core` Rust crate parses, typechecks, and single-steps SML-subset programs, compiles to WebAssembly, and is driven by a static `www/` frontend: type in a program, click "Step" repeatedly, and watch it reduce one redex at a time with the next redex highlighted.

The supported language (see `stepper-core/src/frontend/ast.rs`) is real SML syntax, not a look-alike:
- `val` declarations with patterns (`ident`, `_`, literal, tuple `(p1, p2, ...)`) and optional `: type` annotations
- ints and bools; arithmetic (`+ - * div mod ~`, floor-semantics `div`/`mod` matching SML, not Rust's truncating `/`/`%`); comparisons (`= <> < <= > >=`, nonassociative — no `a < b < c`); `andalso`/`orelse` (short-circuiting, right-associative)
- `if e then e else e`; `let decls in e end`; tuples `(e1, e2, ...)`; `case e of p1 => e1 | p2 => e2 | ...`
- non-recursive lambdas `fn p : t => e` (the parameter type is mandatory — there's no unification engine to infer it) and function application `e1 e2`, plus function types `t1 -> t2` (right-associative). Application is left-associative and binds tighter than every infix operator *and* than `~`, exactly as in SML: `f x + y` is `(f x) + y`, and `~ f x` is `(~f) x`.
- No lists, no recursion, no exceptions, and no exhaustiveness checking on `case` yet. An unmatched `case` or a literal pattern that doesn't match its value panics at runtime (real SML would raise `Match`/`Bind`) — see `HighlightColor::Red`, reserved for a future exception indicator.

## Commands

- `make build` — compiles `stepper-core` to WASM via `wasm-pack` and outputs bindings into `www/pkg` (runs `wasm-pack build --target web --out-dir ../www/pkg` from `stepper-core/`).
- `make serve` — builds, then serves `www/` over HTTP on port 8585 (override with `make serve PORT=xxxx`) using Python's built-in server. The frontend must be loaded through this (or another) HTTP server, not opened as a `file://` URL, since ES module imports require it.
- `make test` — runs `cd stepper-core && cargo test`. Prefer this (or `cargo test` directly from `stepper-core/`) over `make build`/`make serve` when iterating on interpreter logic — it's much faster and doesn't need `wasm-pack`.
- `make clean` — removes `stepper-core/target` and `www/pkg` build artifacts.
- Typical workflow after changing Rust code: `cargo test` to check correctness, then `make build` and `make serve` (or just reload the browser if already serving) to check it in the actual UI.

## Architecture

- `stepper-core/src/frontend/` — parsing and typechecking, purely syntactic/static, no evaluation.
  - `ast.rs` — the AST: `Expr`, `Pattern`, `Type`, `Decl`, `Program` (`= Vec<Decl>`), and `HighlightColor`.
  - `grammar.l` / `grammar.y` — the lexer and LALR grammar, built by `lrlex`/`lrpar` (the `grmtools` toolchain) at compile time via `build.rs` into generated parser code — not source, don't hand-edit, and there's nothing to regenerate manually (`cargo build`/`cargo check` does it).
  - `mod.rs` — `parse_program`, the frontend's only public entry point.
  - `typecheck.rs` — a small structurally-recursive typechecker (`HashMap<String, Type>` env); `let`/`case` arms typecheck into a cloned env so bindings don't leak to siblings or outer scope.
- `stepper-core/src/pretty.rs` — precedence-aware pretty-printer; parsing a pretty-printed program should always reproduce the same AST (several tests exercise this round-trip). Also where `Expr::Highlighted` renders: each `HighlightColor` gets a pair of Unicode Private Use Area sentinel characters wrapping the highlighted text (never emits HTML directly) — `www/index.html` scans for these sentinels client-side to build nested `<span>`s. Changing these sentinels means updating both `pretty.rs` and the `HIGHLIGHT_COLORS` table in `www/index.html`.
- `stepper-core/src/stepping/` — the small-step semantics.
  - `eval.rs` — `step`: performs exactly one reduction on a `Program`, returning the new program and a human-readable message (e.g. `"Evaluated 1 + 2 to 3"`, `"Substituted x = 5"`). `is_value` defines what counts as fully reduced. Also strips any leftover green highlight from the previous step before stepping.
  - `highlight.rs` — `highlight_next`: finds (without performing) whatever `step` would act on next and wraps it in a yellow `Expr::Highlighted`, purely for display — mirrors `step`'s search order exactly, so it never drifts out of sync with what `step` actually does next.
  - `subst.rs` — `substitute` (capture-avoidance isn't needed: this is a pedagogical, first-order, non-shadowing-safe substitution model, not a full language), `try_match`/`destructure` (pattern matching — `try_match` returns `None` on a literal mismatch for `case` to try the next arm; `destructure` is a panicking wrapper used where there's only ever one pattern to satisfy, e.g. a `val` decl).
  - `mod.rs` — re-exports `step` and `highlight_next`; everything else in `stepping/` is private.
- `stepper-core/src/lib.rs` — the `wasm_bindgen` boundary: `enter_formula` (parse + typecheck + store), `step_formula` (advance one step), `current_render` (pretty-print the stored program with the next step highlighted). Holds the current program in a `thread_local!` — there's exactly one program "loaded" at a time, matching the single-textarea frontend.
- `stepper-core/tests/` — all tests, as integration tests (`cargo test`) driving the crate's public API only, one file per phase of the pipeline: `grammar.rs` (what parses to what), `pretty.rs` (printing, mostly as a parse/print round-trip property), `typecheck.rs`, `stepping.rs`, `highlight.rs` (the yellow next-redex preview and green substitution markers), `recursion.rs` (`val rec`, which needs the thread-local recursive-value env reset per test), and `api.rs` (the `lib.rs` wasm-bindgen boundary). A new language feature should get tests in each phase file it touches; test names don't repeat the file name (`grammar.rs`'s `parses_a_case_expression`, not `grammar_parses_a_case_expression`).
  - `tests/common/mod.rs` — shared helpers, included by each file with `mod common;`. Short AST constructors (`val`, `val_rec`, `pvar`, `pat`, ...) for the parser tests; `parse`/`expr_of`/`pattern_of`/`type_of` for pulling one piece out of a parsed program; and `show`/`render_next`/`step_once`/`run` for the stepping ones. `show` renders highlight sentinels as readable markers — `[y...y]` yellow, `[g...g]` green — so expected strings can be written literally. `run`/`run_to_value` also assert, at every step, that `highlight_next` and `step` agree on whether anything is left to do.
- `www/` — static frontend, plain JS with ES modules, no build step of its own. `index.html` imports the generated `pkg/stepper_core.js`, calls `init()` to load the WASM binary, then calls `enter_formula`/`step_formula`/`current_render` directly as JS functions.
- `www/pkg/` — generated by `wasm-pack build`; not source, do not hand-edit. Regenerate with `make build` after changing anything under `stepper-core/src/`. Gitignored.

## Grammar gotchas

The grammar is LALR (`lrpar`/`grmtools`), built with `RecoveryKind::None` (no interactive error recovery — a parse either fully succeeds or returns an error list) and no `%expect`, so an unresolved shift/reduce or reduce/reduce conflict fails the build. Two constructs are deliberately "open-ended" (no closing keyword) — `if ... then ... else <expr>` and `case ... of <arms>` — so each needs the same trick to stay unambiguous when nested without parens: give the relevant terminal(s) the *lowest* declared precedence (see `%nonassoc 'ELSE'` and `%nonassoc 'OF' '=>'` / `%left '|'` at the top of `grammar.y`), which makes the LALR conflict resolution default to shift, i.e. "attach to the nearest/innermost open construct" — the same resolution real SML gives dangling-else and dangling `case` arms. When adding another open-ended construct, follow this pattern rather than reaching for explicit `%expect`.

The other structural rule is SML's `atexp`/`exp` split, which is how function application gets a precedence *above* the whole `%left`/`%right` table rather than a slot inside it. `AppExpr -> AppExpr AtomicExpr` is plain juxtaposition with no precedence declaration of its own: `AtomicExpr`'s FIRST set is disjoint from `FOLLOW(Expr)`, so the parser shifts another atom whenever one is available and only reduces up to `Expr` when the lookahead can't start one. Consequences worth knowing before touching it:
- A new expression form belongs in `AtomicExpr` only if it's self-delimiting, mirroring the Definition's `atexp` productions — literals, identifiers, `( ... )`, tuples, `let ... end`, and `~ <atom>`. The open-ended forms (`if`, `case`, `fn`) must stay on `Expr`, so they need parens as an application argument, just as in real SML.
- `~` is deliberately *not* a precedence-table prefix operator (it was one, wrongly, before functions existed). It takes an `AtomicExpr`, which is what keeps application tighter than negation and lets `f ~5` parse.
- `pretty.rs`'s numeric precedence ladder has to mirror all of this — application at 6, above every infix operator; `~` at 7 — or round-tripping breaks. Anything in `AtomicExpr` may print bare and ignore `min_prec`; anything else may not.
