%start Program
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
    ;

Pattern -> Pattern:
      PatternBase {Pattern {pat: $1, typ: None}}
    | PatternBase ':' Type {Pattern {pat: $1, typ: Some($3)}}
    ;

PatternBase -> PatternBase:
      'ID'
      {
          PatternBase::Ident($lexer.span_str($1.unwrap_or_else(|e| e).span()).to_string())
      }
    | 'WILDCARD' { PatternBase::Wildcard }
    | 'INT'
      {
          PatternBase::IntConst(
              $lexer.span_str($1.unwrap_or_else(|e| e).span()).parse().unwrap_or(0)
          )
      }
    | '~' 'INT'
      {
          PatternBase::IntConst(
              -$lexer.span_str($2.unwrap_or_else(|e| e).span()).parse::<i64>().unwrap_or(0)
          )
      }
    | 'TRUE' { PatternBase::BoolConst(true) }
    | 'FALSE' { PatternBase::BoolConst(false) }
    | '(' PatternBase ')' { $2 }
    | '(' PatternTuple ')' { PatternBase::Tuple($2) }
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
