%start Program
%parse-param errors: &::std::cell::RefCell<::std::vec::Vec<String>>
%nonassoc 'ELSE'
%right 'ARROW'
%nonassoc 'OF' '=>'
%nonassoc 'FN'
%left '|'
%right 'ORELSE'
%right 'ANDALSO'
%nonassoc '=' '<>' '<' '<=' '>' '>='
%left '+' '-'
%left '*' 'DIV' 'MOD'
%%
Program -> Vec<Decl>:
      Program Decl { let mut v = $1; v.push($2); v }
    | { Vec::new() }
    ;

Decl -> Decl:
      'VAL' Pattern '=' Expr
      {
          Decl::ValDecl(ValDecl {
              pat: $2,
              expr: $4,
          })
      }
    | 'VAL' 'REC' Pattern '=' Expr {
        Decl::ValRecDecl(ValDecl {
          pat: $3,
          expr: $5
        })
    }
    | 'FUN' FunClauses { fun_decl($2, errors) }
    ;

/// The clauses of one `fun` declaration, elaborated into a `val`/`val rec` on the
/// spot (see `elaborate`) so that `Decl` — and therefore everything downstream of
/// this file — never has to know `fun` exists. `'|'` separates clauses here just as
/// it separates `MatchArms`, and the same lowest-precedence-shifts-first
/// resolution applies: a trailing `| ...` attaches to an innermost open `case`/`fn`
/// in a clause's body rather than starting a new clause, which is why (exactly as
/// in real SML) such a body needs parens if more clauses are to follow it.
FunClauses -> Vec<FunClause>:
      FunClause { vec![$1] }
    | FunClauses '|' FunClause { let mut v = $1; v.push($3); v }
    ;

/// `f p1 p2 ... = e`, with an optional result type before the `=`. The result type
/// is what a recursive `fun` needs to become a `val rec`, since that requires a
/// complete annotation and there's no unification engine to recover one.
FunClause -> FunClause:
      'ID' FunParams '=' Expr
      {
          FunClause {
              name: $lexer.span_str($1.unwrap_or_else(|e| e).span()).to_string(),
              params: $2,
              result_ty: None,
              body: $4,
          }
      }
    | 'ID' FunParams ':' Type '=' Expr
      {
          FunClause {
              name: $lexer.span_str($1.unwrap_or_else(|e| e).span()).to_string(),
              params: $2,
              result_ty: Some($4),
              body: $6,
          }
      }
    ;

/// A clause's parameters: juxtaposed `AtomicPattern`s, the pattern-level twin of
/// `AppExpr`'s juxtaposed `AtomicExpr`s and unambiguous for the same reason —
/// FIRST(AtomicPattern) is disjoint from what can follow the list (`:` and `=`), so
/// the parser shifts another parameter whenever one is available. Note that this
/// takes `AtomicPattern`, not `Pattern`: an annotated parameter must be written
/// with parens, `fun f (x : int) = ...`, or the `: int` would be read as the
/// *result* type — which is SML's convention anyway.
FunParams -> Vec<Pattern>:
      AtomicPattern { vec![$1] }
    | FunParams AtomicPattern { let mut v = $1; v.push($2); v }
    ;

Pattern -> Pattern:
      AtomicPattern { $1 }
    | AtomicPattern ':' Type { Pattern { pat: $1.pat, typ: Some($3) } }
    ;

/// SML's `atpat`: the self-delimiting pattern forms, and so exactly the ones usable
/// as a `fun` parameter without parens (compare `AtomicExpr`). `'(' Pattern ')'`
/// rather than an unannotated inner pattern, so that `(x : int)` — the only way to
/// annotate a parameter — parses.
AtomicPattern -> Pattern:
      'ID'
      {
          Pattern::untyped(PatternBase::Ident(
              $lexer.span_str($1.unwrap_or_else(|e| e).span()).to_string()
          ))
      }
    | 'WILDCARD' { Pattern::untyped(PatternBase::Wildcard) }
    | 'INT'
      {
          Pattern::untyped(PatternBase::IntConst(
              $lexer.span_str($1.unwrap_or_else(|e| e).span()).parse().unwrap_or(0)
          ))
      }
    | '~' 'INT'
      {
          Pattern::untyped(PatternBase::IntConst(
              -$lexer.span_str($2.unwrap_or_else(|e| e).span()).parse::<i64>().unwrap_or(0)
          ))
      }
    | 'TRUE' { Pattern::untyped(PatternBase::BoolConst(true)) }
    | 'FALSE' { Pattern::untyped(PatternBase::BoolConst(false)) }
    | '(' Pattern ')' { $2 }
    | '(' PatternTuple ')' { Pattern::untyped(PatternBase::Tuple($2)) }
    ;

PatternTuple -> Vec<Pattern>:
      Pattern ',' Pattern { vec![$1, $3] }
    | PatternTuple ',' Pattern { let mut v = $1; v.push($3); v }
    ;


Type -> Type:
      AtomicType { $1 }
    | TypeProduct { Type::Product($1) }
    | Type 'ARROW' Type { Type::Arrow(Box::new($1), Box::new($3)) }
    ;
AtomicType -> Type:
      'INT_TY' { Type::Int }
    | 'BOOL_TY' { Type::Bool }
    | '(' Type ')' { $2 }
    ;

TypeProduct -> Vec<Box<Type>>:
      AtomicType '*' AtomicType { vec![Box::new($1), Box::new($3)] }
    | TypeProduct '*' AtomicType { let mut v = $1; v.push(Box::new($3)); v }
    ;

Expr -> Expr:
      Expr '+' Expr { Expr::Add(Box::new($1), Box::new($3)) }
    | Expr '-' Expr { Expr::Sub(Box::new($1), Box::new($3)) }
    | Expr '*' Expr { Expr::Mul(Box::new($1), Box::new($3)) }
    | Expr 'DIV' Expr { Expr::Div(Box::new($1), Box::new($3)) }
    | Expr 'MOD' Expr { Expr::Mod(Box::new($1), Box::new($3)) }
    | Expr '=' Expr { Expr::Eq(Box::new($1), Box::new($3)) }
    | Expr '<>' Expr { Expr::Ne(Box::new($1), Box::new($3)) }
    | Expr '<' Expr { Expr::Lt(Box::new($1), Box::new($3)) }
    | Expr '<=' Expr { Expr::Le(Box::new($1), Box::new($3)) }
    | Expr '>' Expr { Expr::Gt(Box::new($1), Box::new($3)) }
    | Expr '>=' Expr { Expr::Ge(Box::new($1), Box::new($3)) }
    | Expr 'ANDALSO' Expr { Expr::AndAlso(Box::new($1), Box::new($3)) }
    | Expr 'ORELSE' Expr { Expr::OrElse(Box::new($1), Box::new($3)) }
    | 'IF' Expr 'THEN' Expr 'ELSE' Expr
      { Expr::If(Box::new($2), Box::new($4), Box::new($6)) }
    | 'CASE' Expr 'OF' MatchArms { Expr::Match(Box::new($2), $4) }
    | 'FN' MatchArms
      { Expr::Lambda($2) }
    | AppExpr { $1 }
    ;

TupleItems -> Vec<Expr>:
      Expr ',' Expr { vec![$1, $3] }
    | TupleItems ',' Expr { let mut v = $1; v.push($3); v }
    ;

/// Function application (`e1 e2`): plain left-recursive juxtaposition of
/// `AtomicExpr`s, same shape as `TupleItems`/`TypeProduct`/`Program`, and the same
/// shape the Definition gives it (`exp ::= exp atexp`). Needs no precedence
/// declaration of its own (unlike every other `Expr` production) — `AtomicExpr`'s
/// FIRST set is disjoint from `FOLLOW(Expr)` (every infix operator and closing
/// token), so the parser always has exactly one valid action: shift another
/// `AtomicExpr` to keep applying, or reduce up to `Expr` once the lookahead can't
/// start one. Shifting always wins while an atom is available, which is what makes
/// application bind tighter than every infix operator — `f x + y` is `(f x) + y`,
/// never `f (x + y)` — without touching the precedence table at all.
AppExpr -> Expr:
      AtomicExpr { $1 }
    | AppExpr AtomicExpr { Expr::App(Box::new($1), Box::new($2)) }
    ;

/// SML's `atexp`: the self-delimiting expression forms, each usable as an
/// application argument without parens because it's a single token or bracketed by
/// its own. Membership here follows the Definition's `atexp` productions, so `let`
/// belongs (it's closed by `end`, and `f let val a = 1 in a end` is legal SML)
/// while `if`/`case`/`fn` don't — they're open-ended, so juxtaposing one bare would
/// be ambiguous, which is why real SML needs parens in `f (if b then x else y)`.
///
/// `~` lives here too, applying to an `AtomicExpr` rather than to an `Expr`, which
/// is what keeps application binding tighter than negation: `~ f x` is `(~f) x`
/// (a type error, exactly as in SML, where `~` is an ordinary function identifier
/// and application is left-associative), and `f ~5` parses as `f` applied to `~5`
/// rather than failing. The Definition gets the latter lexically — `~5` is one
/// negative-literal token there, so SML distinguishes `f ~5` from `f ~ 5`
/// (`(f ~) 5`); keeping `~` a separate token means we accept both as `f (~5)`,
/// which is the one deliberate divergence here and the friendlier reading.
AtomicExpr -> Expr:
      '(' Expr ')' { $2 }
    | '(' TupleItems ')' { Expr::Tuple($2) }
    | 'LET' Program 'IN' Expr 'END' { Expr::Let($2, Box::new($4)) }
    | '~' AtomicExpr { Expr::Neg(Box::new($2)) }
    | 'TRUE' { Expr::BoolConst(true) }
    | 'FALSE' { Expr::BoolConst(false) }
    | 'INT'
      {
          Expr::IntConst(
              $lexer.span_str($1.unwrap_or_else(|e| e).span()).parse().unwrap_or(0)
          )
      }
    | 'ID'
      {
          Expr::Ident($lexer.span_str($1.unwrap_or_else(|e| e).span()).to_string())
      }
    ;

/// Left-recursive, same shape as `TupleItems`/`TypeProduct`. `'|'` is declared
/// with higher precedence than `'OF'`/`'=>'` (see the precedence block above) so
/// that a trailing `| pat => expr` always shifts onto the *innermost* open
/// `case`'s `MatchArms` rather than closing it and attaching to an outer one —
/// same "nearest wins" resolution real SML uses, and the same shift-preferring
/// trick `'ELSE'` uses to resolve dangling-else.
MatchArms -> Vec<(Pattern, Expr)>:
      Pattern '=>' Expr { vec![($1, $3)] }
    | MatchArms '|' Pattern '=>' Expr { let mut v = $1; v.push(($3, $5)); v }
    ;
%%
use super::ast::{Decl, ValDecl, Expr, Pattern, PatternBase, Type};
use super::elaborate::{fun_decl, FunClause};
