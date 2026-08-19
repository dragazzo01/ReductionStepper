%start Program
%nonassoc 'ELSE'
%nonassoc 'OF' '=>'
%left '|'
%right 'ORELSE'
%right 'ANDALSO'
%nonassoc '=' '<>' '<' '<=' '>' '>='
%left '+' '-'
%left '*' 'DIV' 'MOD'
%left '~'
%%
Program -> Vec<Decl>:
      Program Decl { let mut v = $1; v.push($2); v }
    | { Vec::new() }
    ;

Decl -> Decl:
      'VAL' Pattern TypeDecl '=' Expr
      {
          Decl {
              pat: $2,
              ty: $3,
              expr: $5,
          }
      }
    ;

/// Same shape as `Expr`'s own atom/tuple split (`'(' Expr ')'` groups,
/// `'(' TupleItems ')'` builds a `Tuple`) — no flattening operator like `Type`'s
/// `*` is involved here, so there's no risk of the ambiguity that forced
/// `AtomicType`/`TypeProduct` apart; `Pattern` can stay a single nonterminal.
Pattern -> Pattern:
      'ID'
      {
          Pattern::Ident($lexer.span_str($1.unwrap_or_else(|e| e).span()).to_string())
      }
    | 'WILDCARD' { Pattern::Wildcard }
    | 'INT'
      {
          Pattern::IntConst(
              $lexer.span_str($1.unwrap_or_else(|e| e).span()).parse().unwrap_or(0)
          )
      }
    | '~' 'INT'
      {
          Pattern::IntConst(
              -$lexer.span_str($2.unwrap_or_else(|e| e).span()).parse::<i64>().unwrap_or(0)
          )
      }
    | 'TRUE' { Pattern::BoolConst(true) }
    | 'FALSE' { Pattern::BoolConst(false) }
    | '(' Pattern ')' { $2 }
    | '(' PatternItems ')' { Pattern::Tuple($2) }
    ;

PatternItems -> Vec<Pattern>:
      Pattern ',' Pattern { vec![$1, $3] }
    | PatternItems ',' Pattern { let mut v = $1; v.push($3); v }
    ;

TypeDecl -> Option<Type>:
      ':' Type { Some($2) }
    | { None }
    ;

Type -> Type:
      AtomicType { $1 }
    | TypeProduct { Type::Product($1) }
    ;

/// The only thing allowed as an operand of `TypeProduct` — deliberately *not*
/// `Type` itself. If it were, `TypeProduct -> Type '*' Type` combined with
/// `Type -> TypeProduct` would let the same input derive two different ASTs: a
/// flat product built by left-recursive extension, or a nested one built by
/// reducing a sub-product back up to `Type` and re-entering the base rule (e.g.
/// `int * bool * int` as `Product([Int, Bool, Int])` vs `Product([Product([Int,
/// Bool]), Int])`) — a genuine Reduce/Reduce ambiguity, not just an associativity
/// choice. Restricting the operand to `AtomicType` closes that second path: an
/// already-built product can only reappear as a `*` operand by going through
/// `'(' Type ')'` first, which is exactly the signal that it should stay nested
/// rather than flatten in. A future `Type -> Type 'ARROW' Type` (function types)
/// can skip this problem entirely and live directly on `Type` instead, since it's
/// a genuine binary node, not a flattening one.
AtomicType -> Type:
      'INT_TY' { Type::Int }
    | 'BOOL_TY' { Type::Bool }
    | '(' Type ')' { $2 }
    ;

/// Left-recursive, same shape as `TupleItems`, so each `*` just pushes onto the
/// built-up prefix — O(1) amortized per element rather than the O(n) copy a
/// right-recursive rule would force at every reduction.
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
    | '~' Expr { Expr::Neg(Box::new($2)) }
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
    | 'LET' Program 'IN' Expr 'END' { Expr::Let($2, Box::new($4)) }
    | 'CASE' Expr 'OF' MatchArms { Expr::Match(Box::new($2), $4) }
    | '(' Expr ')' { $2 }
    | '(' TupleItems ')' { Expr::Tuple($2) }
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

TupleItems -> Vec<Expr>:
      Expr ',' Expr { vec![$1, $3] }
    | TupleItems ',' Expr { let mut v = $1; v.push($3); v }
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
use super::ast::{Decl, Expr, Pattern, Type};
