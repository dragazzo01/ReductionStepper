//! Millet's SML syntax tree, lowered into this crate's AST.
//!
//! Parsing is [millet]'s job — `sml-lex` and `sml-parse` accept the whole of
//! Standard ML and hand back a concrete syntax tree — and this module's job is to
//! pick out of that tree the fragment of the language the stepper implements. The
//! split is deliberate: a full-SML parser is a large thing to maintain and gets
//! the genuinely hard parts right (notably `infix` declarations, whose precedence
//! is decided *during* parsing and so is out of reach of any fixed grammar),
//! while what this crate can actually *step* is small and changes often.
//!
//! So every construct this crate has no AST for is rejected here, by name and
//! with the source range that produced it — `unsupported` is the one place that
//! happens, and adding a language feature means turning one of its arms into a
//! real lowering. Nothing downstream ever sees a tree it can't handle.
//!
//! Two things worth knowing about the tree being consumed:
//!
//! - **Every accessor returns `Option`**, because millet's parser recovers from
//!   errors and hands back whatever it managed to build. `parse_program` rejects a
//!   program with parse errors before calling in here, so a `None` means the tree
//!   is shaped in a way we didn't anticipate rather than that the user typed
//!   something wrong; `require` reports it rather than panicking.
//! - **Patterns don't distinguish variables from constructors.** `val x = ...`
//!   yields a `ConPat`, since which of the two `x` is depends on what's in scope —
//!   a static question millet answers in a later pass we don't use. With no
//!   `datatype` in this subset the answer is simply that a bare name is a
//!   variable, and `true`/`false` are the only constructors there are.
//!
//! Binders are minted fresh at every occurrence, binding or not, exactly as the
//! old grammar's actions did; [`super::resolve`] repoints the uses afterwards.
//!
//! [millet]: https://github.com/azdavis/millet

use std::collections::HashMap;

use sml_syntax::ast::{self, AstNode as _};
use sml_syntax::kind::SyntaxNode;

use super::ast::{
    BinOp, Binder, Decl, Expr, ExprKind, Pattern, PatternBase, Program, Type, TypeDecl, ValDecl,
};
use super::elaborate::{FunClause, elaborate};

/// The type aliases in scope, each name paired with what it stands for.
///
/// The one piece of context lowering has to carry. A `ConTy` is only a name, so
/// deciding whether `point` is an alias — and of what — needs to know which
/// `type` declarations came before it. Resolving here rather than later is what
/// lets `Type::Named` carry its own expansion, so no pass after this one needs an
/// environment at all.
///
/// Scoped by cloning at a `let`, so an alias declared inside one doesn't escape
/// it; a later declaration of the same name simply overwrites, which is the
/// shadowing rule the rest of the language already follows.
type TypeAliases = HashMap<String, Type>;

/// Something in the program that can't be lowered, and where in the source it
/// was. `parse_program` turns the offset into a line and column; keeping it an
/// offset here means this module never needs the input string.
#[derive(Debug)]
pub struct Error {
    pub message: String,
    pub start: usize,
}

type Result<T> = std::result::Result<T, Error>;

impl Error {
    fn at(node: &SyntaxNode, message: impl Into<String>) -> Self {
        Error {
            message: message.into(),
            start: usize::from(node.text_range().start()),
        }
    }
}

/// Rejects a construct the stepper has no AST for. `what` names it as the user
/// wrote it: "`datatype` declarations — not supported yet". The dash keeps the
/// phrasing free of subject-verb agreement, since `what` is sometimes a plural
/// class of things and sometimes one named operator or type.
fn unsupported<T>(node: &SyntaxNode, what: &str) -> Result<T> {
    Err(Error::at(node, format!("{what} — not supported yet")))
}

/// Unwraps a child the tree should have had. See the note on `Option` above: this
/// fires only on a tree shape we didn't anticipate, never on ordinary bad input.
fn require<T>(child: Option<T>, node: &SyntaxNode, what: &str) -> Result<T> {
    child.ok_or_else(|| Error::at(node, format!("expected {what} here")))
}

/// Lowers a whole parsed file.
pub fn lower(root: &ast::Root) -> Result<Program> {
    lower_decs(root.decs(), &mut TypeAliases::new())
}

/// The declarations of a file or a `let`, flattened.
///
/// Millet nests these three deep — a `Dec` holds a `DecWithTail` of `DecInSeq`s,
/// which is where SML's optional `;` separators and `sharing` tails live — and a
/// whole program usually arrives as one `Dec` with many `DecInSeq`s inside it.
/// None of that structure survives into `Program`, which is a flat `Vec<Decl>`.
/// `types` is threaded through *mutably* here, and only here: declarations take
/// effect in order, so a `type` adds to the aliases the declarations after it can
/// use.
fn lower_decs(
    decs: impl Iterator<Item = ast::Dec>,
    types: &mut TypeAliases,
) -> Result<Vec<Decl>> {
    let mut out = Vec::new();
    for dec in decs {
        let tail = require(dec.dec_with_tail(), dec.syntax(), "a declaration")?;
        for seq in tail.dec_in_seqs() {
            let one = require(seq.dec_one(), seq.syntax(), "a declaration")?;
            out.push(lower_dec(&one, types)?);
        }
    }
    Ok(out)
}

fn lower_dec(dec: &ast::DecOne, types: &mut TypeAliases) -> Result<Decl> {
    match dec {
        ast::DecOne::ValDec(d) => {
            if d.ty_var_seq().is_some_and(|s| s.ty_var_args().next().is_some()) {
                return unsupported(dec.syntax(), "explicit type variables");
            }
            let mut binds = d.val_binds();
            let first = require(binds.next(), d.syntax(), "a binding after `val`")?;
            if binds.next().is_some() {
                return unsupported(dec.syntax(), "`val ... and ...` simultaneous bindings");
            }
            lower_val_bind(&first, types)
        }
        // The one declaration that binds a type rather than a value: it adds to
        // `types` and is otherwise inert from here on.
        ast::DecOne::TyDec(d) => lower_ty_dec(d, types),
        ast::DecOne::FunDec(d) => {
            if d.ty_var_seq().is_some_and(|s| s.ty_var_args().next().is_some()) {
                return unsupported(dec.syntax(), "explicit type variables");
            }
            let mut binds = d.fun_binds();
            let first = require(binds.next(), d.syntax(), "a binding after `fun`")?;
            if binds.next().is_some() {
                return unsupported(dec.syntax(), "`fun ... and ...` mutually recursive functions");
            }
            let clauses = first
                .fun_bind_cases()
                .map(|case| lower_fun_clause(&case, types))
                .collect::<Result<Vec<_>>>()?;
            if clauses.is_empty() {
                return Err(Error::at(d.syntax(), "expected a clause after `fun`"));
            }
            elaborate(clauses).map_err(|message| Error::at(d.syntax(), message))
        }
        // A bare expression, which the REPL of every SML implementation takes as
        // `val it = <exp>`; millet parses one as a declaration of its own so that
        // a language server can say something about it, and lowering it the same
        // way is what lets the stepper be typed into like a REPL.
        ast::DecOne::ExpDec(d) => {
            let exp = require(d.exp(), d.syntax(), "an expression")?;
            Ok(Decl::ValDecl(ValDecl {
                pat: Pattern::untyped(PatternBase::Var(Binder::new("it"))),
                expr: lower_exp(&exp, types)?,
            }))
        }
        _ => unsupported(dec.syntax(), dec_description(dec)),
    }
}

/// How to name a declaration form we don't implement, in a message.
fn dec_description(dec: &ast::DecOne) -> &'static str {
    match dec {
        ast::DecOne::HoleDec(_) => "`...` holes",
        ast::DecOne::DatDec(_) | ast::DecOne::DatCopyDec(_) => "`datatype` declarations",
        ast::DecOne::AbstypeDec(_) => "`abstype` declarations",
        ast::DecOne::ExDec(_) => "`exception` declarations",
        ast::DecOne::OpenDec(_) => "`open` declarations",
        ast::DecOne::InfixDec(_) | ast::DecOne::InfixrDec(_) | ast::DecOne::NonfixDec(_) => {
            "user-declared infix operators"
        }
        ast::DecOne::DoDec(_) => "`do` declarations",
        ast::DecOne::LocalDec(_) => "`local` declarations",
        ast::DecOne::StructureDec(_) => "`structure` declarations",
        ast::DecOne::SignatureDec(_) => "`signature` declarations",
        ast::DecOne::FunctorDec(_) => "`functor` declarations",
        ast::DecOne::IncludeDec(_) => "`include` declarations",
        ast::DecOne::EsImportDec(_) => "`_esImport` declarations",
        ast::DecOne::ValDec(_)
        | ast::DecOne::FunDec(_)
        | ast::DecOne::TyDec(_)
        | ast::DecOne::ExpDec(_) => {
            unreachable!("handled in lower_dec")
        }
    }
}

/// `type name = ty`, the only declaration that binds a type.
///
/// The parameterized forms are turned down rather than lowered: `type 'a pair`
/// declares a type *constructor*, and `Type` has no arguments to apply — the same
/// reason `lower_ty` rejects `int list`.
fn lower_ty_dec(dec: &ast::TyDec, types: &mut TypeAliases) -> Result<Decl> {
    if dec
        .ty_head()
        .is_some_and(|head| matches!(head.kind, ast::TyHeadKind::EqtypeKw))
    {
        return unsupported(dec.syntax(), "`eqtype` declarations");
    }
    let mut binds = dec.ty_binds();
    let first = require(binds.next(), dec.syntax(), "a binding after `type`")?;
    if binds.next().is_some() {
        return unsupported(dec.syntax(), "`type ... and ...` simultaneous bindings");
    }
    if first
        .ty_var_seq()
        .is_some_and(|s| s.ty_var_args().next().is_some())
    {
        return unsupported(first.syntax(), "parameterized `type` declarations");
    }
    let name = require(first.name(), first.syntax(), "a type name")?
        .text()
        .to_string();
    let eq_ty = require(first.eq_ty(), first.syntax(), "`= <type>`")?;
    let ty = require(eq_ty.ty(), eq_ty.syntax(), "a type")?;
    // Lowered *before* the name is in scope, so `type t = t` is an unknown type
    // rather than a cycle — which is what keeps `Type::resolved` terminating.
    let definition = lower_ty(&ty, types)?;
    types.insert(name.clone(), definition.clone());
    Ok(Decl::TypeDecl(TypeDecl::new(name, definition)))
}

fn lower_val_bind(bind: &ast::ValBind, types: &TypeAliases) -> Result<Decl> {
    let pat = lower_pat(&require(bind.pat(), bind.syntax(), "a pattern")?, types)?;
    let eq_exp = require(bind.eq_exp(), bind.syntax(), "`= <expression>`")?;
    let expr = lower_exp(&require(eq_exp.exp(), eq_exp.syntax(), "an expression")?, types)?;
    let val_decl = ValDecl { pat, expr };
    Ok(if bind.rec_kw().is_some() {
        Decl::ValRecDecl(val_decl)
    } else {
        Decl::ValDecl(val_decl)
    })
}

/// One clause of a `fun`. [`super::elaborate`] turns the collected clauses into
/// the `val`/`val rec` they stand for.
fn lower_fun_clause(case: &ast::FunBindCase, types: &TypeAliases) -> Result<FunClause> {
    let head = require(case.fun_bind_case_head(), case.syntax(), "a function name")?;
    let name = match head {
        ast::FunBindCaseHead::PrefixFunBindCaseHead(head) => {
            let name = require(head.name_star_eq(), head.syntax(), "a function name")?;
            name.token.text().to_string()
        }
        ast::FunBindCaseHead::InfixFunBindCaseHead(head) => {
            return unsupported(head.syntax(), "infix `fun` declarations");
        }
    };
    let params = case
        .pats()
        .map(|pat| lower_pat(&pat, types))
        .collect::<Result<Vec<_>>>()?;
    // Millet accepts a clause with no parameters so it can say something better
    // about it later; `fun` binds a function, so there is nothing to elaborate.
    if params.is_empty() {
        return Err(Error::at(
            case.syntax(),
            format!("`{name}` needs at least one argument to be a `fun`: use `val` instead"),
        ));
    }
    let result_ty = case
        .ty_annotation()
        .map(|ann| {
            let ty = require(ann.ty(), ann.syntax(), "a type")?;
            lower_ty(&ty, types)
        })
        .transpose()?;
    let eq_exp = require(case.eq_exp(), case.syntax(), "`= <expression>`")?;
    let body = lower_exp(&require(eq_exp.exp(), eq_exp.syntax(), "an expression")?, types)?;
    Ok(FunClause {
        name,
        params,
        result_ty,
        body,
    })
}

fn lower_exp(exp: &ast::Exp, types: &TypeAliases) -> Result<Expr> {
    let kind = match exp {
        ast::Exp::SConExp(e) => {
            let scon = require(e.s_con(), e.syntax(), "a constant")?;
            match lower_scon(&scon, e.syntax())? {
                Literal::Int(n) => ExprKind::IntConst(n),
                Literal::Real(x) => ExprKind::RealConst(x),
                Literal::Str(s) => ExprKind::StringConst(s),
            }
        }
        // `true` and `false` are constructors rather than variables, and with no
        // `datatype` in this subset they are the only two there are.
        ast::Exp::PathExp(e) => {
            let path = require(e.path(), e.syntax(), "a name")?;
            match single_name(&path, e.syntax())?.as_str() {
                "true" => ExprKind::BoolConst(true),
                "false" => ExprKind::BoolConst(false),
                name => ExprKind::Var(Binder::new(name)),
            }
        }
        // Parens carry no meaning into the AST — `pretty/build.rs` puts back
        // whatever the precedences call for.
        ast::Exp::ParenExp(e) => {
            return lower_exp(&require(e.exp(), e.syntax(), "an expression")?, types);
        }
        ast::Exp::TupleExp(e) => {
            let items = e
                .exp_args()
                .map(|arg| lower_exp(&require(arg.exp(), arg.syntax(), "an expression")?, types))
                .collect::<Result<Vec<_>>>()?;
            // `()` is the 0-tuple. Millet gives it the same node as any other
            // tuple; one-element tuples don't exist (those are `ParenExp`), so
            // this is the only case where a `TupleExp` isn't a `Tuple`.
            match items.is_empty() {
                true => ExprKind::Unit,
                false => ExprKind::Tuple(items),
            }
        }
        // `~e` is an ordinary application of `~` in SML, and that is how millet
        // parses it. `~5` never reaches here — the lexer makes it one negative
        // literal — which is exactly what lets `f ~5` apply `f` to `~5` while
        // `~ f x` stays `(~f) x`.
        ast::Exp::AppExp(e) => {
            let func = require(e.func(), e.syntax(), "a function")?;
            let arg = lower_exp(&require(e.arg(), e.syntax(), "an argument")?, types)?;
            if is_named(&func, "~") {
                ExprKind::Neg(Box::new(arg))
            } else {
                ExprKind::App(Box::new(lower_exp(&func, types)?), Box::new(arg))
            }
        }
        // Infix operators arrive by name, already grouped by precedence — the
        // parser resolved that, `infix` declarations included. All that's left is
        // to recognise the eleven this crate can evaluate.
        ast::Exp::InfixExp(e) => {
            let name = require(e.name_star_eq(), e.syntax(), "an operator")?;
            let symbol = name.token.text();
            let Some(op) = BinOp::from_symbol(symbol) else {
                return unsupported(e.syntax(), &format!("the operator `{symbol}`"));
            };
            let lhs = lower_exp(&require(e.lhs(), e.syntax(), "an expression")?, types)?;
            let rhs = lower_exp(&require(e.rhs(), e.syntax(), "an expression")?, types)?;
            ExprKind::BinOp(op, Box::new(lhs), Box::new(rhs))
        }
        ast::Exp::AndalsoExp(e) => {
            let lhs = lower_exp(&require(e.lhs(), e.syntax(), "an expression")?, types)?;
            let rhs = lower_exp(&require(e.rhs(), e.syntax(), "an expression")?, types)?;
            ExprKind::AndAlso(Box::new(lhs), Box::new(rhs))
        }
        ast::Exp::OrelseExp(e) => {
            let lhs = lower_exp(&require(e.lhs(), e.syntax(), "an expression")?, types)?;
            let rhs = lower_exp(&require(e.rhs(), e.syntax(), "an expression")?, types)?;
            ExprKind::OrElse(Box::new(lhs), Box::new(rhs))
        }
        ast::Exp::IfExp(e) => {
            let cond = lower_exp(&require(e.cond(), e.syntax(), "a condition")?, types)?;
            let yes = lower_exp(&require(e.yes(), e.syntax(), "a `then` branch")?, types)?;
            let no = lower_exp(&require(e.no(), e.syntax(), "an `else` branch")?, types)?;
            ExprKind::If(Box::new(cond), Box::new(yes), Box::new(no))
        }
        ast::Exp::CaseExp(e) => {
            let scrutinee = lower_exp(&require(e.exp(), e.syntax(), "an expression")?, types)?;
            let matcher = require(e.matcher(), e.syntax(), "match arms")?;
            ExprKind::Match(Box::new(scrutinee), lower_matcher(&matcher, types)?)
        }
        ast::Exp::FnExp(e) => {
            let matcher = require(e.matcher(), e.syntax(), "match arms")?;
            ExprKind::Lambda(lower_matcher(&matcher, types)?)
        }
        // A `let` scopes types as well as values, so its declarations extend a
        // *copy* of the aliases: a `type` declared inside doesn't outlive the
        // `end`.
        ast::Exp::LetExp(e) => {
            let mut inner = types.clone();
            let decls = lower_decs(e.decs(), &mut inner)?;
            let mut body = e.exps_in_seq();
            let first = require(body.next(), e.syntax(), "an expression after `in`")?;
            if body.next().is_some() {
                return unsupported(e.syntax(), "`;` expression sequences");
            }
            let body = lower_exp(
                &require(first.exp(), first.syntax(), "an expression")?,
                &inner,
            )?;
            ExprKind::Let(decls, Box::new(body))
        }
        _ => return unsupported(exp.syntax(), exp_description(exp)),
    };
    Ok(Expr::new(kind))
}

/// How to name an expression form we don't implement, in a message.
fn exp_description(exp: &ast::Exp) -> &'static str {
    match exp {
        ast::Exp::HoleExp(_) | ast::Exp::WildcardExp(_) => "`...` and `_` holes",
        ast::Exp::OpAndalsoExp(_) | ast::Exp::OpOrelseExp(_) => "`op andalso` and `op orelse`",
        ast::Exp::RecordExp(_) => "records",
        ast::Exp::SelectorExp(_) => "record selectors like `#lab`",
        ast::Exp::ListExp(_) => "lists",
        ast::Exp::VectorExp(_) => "vectors",
        ast::Exp::SeqExp(_) => "`;` expression sequences",
        ast::Exp::TypedExp(_) => "type annotations on expressions",
        ast::Exp::HandleExp(_) => "`handle` expressions",
        ast::Exp::RaiseExp(_) => "`raise` expressions",
        ast::Exp::WhileExp(_) => "`while` loops",
        _ => unreachable!("handled in lower_exp"),
    }
}

fn lower_matcher(matcher: &ast::Matcher, types: &TypeAliases) -> Result<Vec<(Pattern, Expr)>> {
    matcher
        .arms()
        .map(|arm| {
            let pat = lower_pat(&require(arm.pat(), arm.syntax(), "a pattern")?, types)?;
            let exp = lower_exp(&require(arm.exp(), arm.syntax(), "an expression")?, types)?;
            Ok((pat, exp))
        })
        .collect()
}

fn lower_pat(pat: &ast::Pat, types: &TypeAliases) -> Result<Pattern> {
    let base = match pat {
        ast::Pat::WildcardPat(_) => PatternBase::Wildcard,
        // A real literal is the one constant SML won't let you match on: a pattern
        // may hold a constant only of an equality type, and `real` isn't one (see
        // `ast::ExprKind::RealConst`). That's the rule itself rather than a gap in
        // this subset, so it doesn't go through `unsupported`.
        ast::Pat::SConPat(p) => {
            let scon = require(p.s_con(), p.syntax(), "a constant")?;
            match lower_scon(&scon, p.syntax())? {
                Literal::Int(n) => PatternBase::IntConst(n),
                Literal::Str(s) => PatternBase::StringConst(s),
                Literal::Real(_) => {
                    return Err(Error::at(
                        p.syntax(),
                        "`real` is not an equality type, so a real literal cannot be a pattern",
                    ));
                }
            }
        }
        // See the module header: a bare name is a variable here, because the only
        // constructors this subset has are `true` and `false`.
        ast::Pat::ConPat(p) => {
            if p.pat().is_some() {
                return unsupported(p.syntax(), "constructor patterns");
            }
            let path = require(p.path(), p.syntax(), "a name")?;
            match single_name(&path, p.syntax())?.as_str() {
                "true" => PatternBase::BoolConst(true),
                "false" => PatternBase::BoolConst(false),
                name => PatternBase::Var(Binder::new(name)),
            }
        }
        ast::Pat::ParenPat(p) => {
            return lower_pat(&require(p.pat(), p.syntax(), "a pattern")?, types);
        }
        ast::Pat::TuplePat(p) => {
            let items = p
                .pat_args()
                .map(|arg| lower_pat(&require(arg.pat(), arg.syntax(), "a pattern")?, types))
                .collect::<Result<Vec<_>>>()?;
            match items.is_empty() {
                true => PatternBase::Unit,
                false => PatternBase::Tuple(items),
            }
        }
        // The annotation attaches to the pattern already built, keeping its id —
        // `x` and `x : int` are the same place on screen.
        ast::Pat::TypedPat(p) => {
            let inner = lower_pat(&require(p.pat(), p.syntax(), "a pattern")?, types)?;
            let ty = lower_ty(&require(p.ty(), p.syntax(), "a type")?, types)?;
            return Ok(Pattern {
                id: inner.id,
                pat: inner.pat,
                typ: Some(ty),
            });
        }
        _ => return unsupported(pat.syntax(), pat_description(pat)),
    };
    Ok(Pattern::untyped(base))
}

/// How to name a pattern form we don't implement, in a message.
fn pat_description(pat: &ast::Pat) -> &'static str {
    match pat {
        ast::Pat::RecordPat(_) => "record patterns",
        ast::Pat::ListPat(_) => "list patterns",
        ast::Pat::VectorPat(_) => "vector patterns",
        ast::Pat::InfixPat(_) => "infix constructor patterns",
        ast::Pat::AsPat(_) => "`as` patterns",
        ast::Pat::OrPat(_) => "`|` patterns",
        _ => unreachable!("handled in lower_pat"),
    }
}

fn lower_ty(ty: &ast::Ty, types: &TypeAliases) -> Result<Type> {
    match ty {
        ast::Ty::ConTy(t) => {
            if t.ty_seq().is_some() {
                return unsupported(t.syntax(), "type constructors with arguments");
            }
            let path = require(t.path(), t.syntax(), "a type name")?;
            let name = single_name(&path, t.syntax())?;
            // Aliases are looked up first, so a `type` declaration shadows a base
            // type the way SML lets it. The alias keeps its *name* alongside the
            // type it stands for — that pairing is the whole of `Type::Named`.
            if let Some(definition) = types.get(&name) {
                return Ok(Type::Named(name, Box::new(definition.clone())));
            }
            match name.as_str() {
                "int" => Ok(Type::Int),
                "real" => Ok(Type::Real),
                "string" => Ok(Type::String),
                "bool" => Ok(Type::Bool),
                "unit" => Ok(Type::Unit),
                name => unsupported(t.syntax(), &format!("the type `{name}`")),
            }
        }
        ast::Ty::FnTy(t) => {
            let param = lower_ty(&require(t.param(), t.syntax(), "a type")?, types)?;
            let res = lower_ty(&require(t.res(), t.syntax(), "a type")?, types)?;
            Ok(Type::Arrow(Box::new(param), Box::new(res)))
        }
        // `t1 * t2 * t3` arrives as one head plus a list of `* t`, and stays flat:
        // `Type::Product` is n-ary, and SML's `*` doesn't associate anyway.
        ast::Ty::TupleTy(t) => {
            let head = lower_ty(&require(t.ty(), t.syntax(), "a type")?, types)?;
            let mut parts = vec![Box::new(head)];
            for star in t.star_tys() {
                let ty = require(star.ty(), star.syntax(), "a type")?;
                parts.push(Box::new(lower_ty(&ty, types)?));
            }
            match <[Box<Type>; 1]>::try_from(parts) {
                Ok([only]) => Ok(*only),
                Err(parts) => Ok(Type::Product(parts)),
            }
        }
        ast::Ty::ParenTy(t) => lower_ty(&require(t.ty(), t.syntax(), "a type")?, types),
        ast::Ty::OneArgConTy(t) => unsupported(t.syntax(), "type constructors with arguments"),
        ast::Ty::TyVarTy(t) => unsupported(t.syntax(), "type variables"),
        ast::Ty::RecordTy(t) => unsupported(t.syntax(), "record types"),
        ast::Ty::HoleTy(_) | ast::Ty::WildcardTy(_) => {
            unsupported(ty.syntax(), "`...` and `_` type holes")
        }
    }
}

/// The one name a path spells, rejecting qualified ones (`List.map`) — this crate
/// has no structures, so a dot can only be a mistake.
fn single_name(path: &ast::Path, node: &SyntaxNode) -> Result<String> {
    let mut parts = path.name_star_eq_dots();
    let first = require(parts.next(), node, "a name")?;
    if parts.next().is_some() {
        return unsupported(node, "qualified names like `List.map`");
    }
    let name = require(first.name_star_eq(), node, "a name")?;
    Ok(name.token.text().to_string())
}

/// Whether `exp` is exactly the variable `name` — used to spot the `~` in the
/// application `~e`.
fn is_named(exp: &ast::Exp, name: &str) -> bool {
    let ast::Exp::PathExp(exp) = exp else {
        return false;
    };
    exp.path()
        .and_then(|path| single_name(&path, exp.syntax()).ok())
        .is_some_and(|found| found == name)
}

/// The value a special constant denotes. Words and characters are the two forms
/// this crate has no value for; the other three it does.
enum Literal {
    Int(i64),
    Real(f64),
    Str(String),
}

fn lower_scon(scon: &ast::SCon, node: &SyntaxNode) -> Result<Literal> {
    match scon.kind {
        ast::SConKind::IntLit => lower_int(scon, node).map(Literal::Int),
        ast::SConKind::RealLit => lower_real(scon, node).map(Literal::Real),
        ast::SConKind::StringLit => lower_string(scon, node).map(Literal::Str),
        ast::SConKind::WordLit => unsupported(node, "word literals"),
        ast::SConKind::CharLit => unsupported(node, "character literals"),
    }
}

/// The value of an integer literal. `~` is SML's minus sign and `0x` its hex
/// prefix.
fn lower_int(scon: &ast::SCon, node: &SyntaxNode) -> Result<i64> {
    let text = scon.token.text();
    let (negative, digits) = match text.strip_prefix('~') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let magnitude = match digits.strip_prefix("0x") {
        Some(hex) => i64::from_str_radix(hex, 16),
        None => digits.parse::<i64>(),
    };
    let Ok(magnitude) = magnitude else {
        return Err(Error::at(node, format!("`{text}` is not a valid integer")));
    };
    Ok(if negative { -magnitude } else { magnitude })
}

/// The value of a real literal. SML writes its minus sign `~`, in the mantissa
/// and the exponent alike (`~1.5e~3`); with those turned back into `-`, what is
/// left is exactly what Rust's `f64` parser reads, correctly rounded.
fn lower_real(scon: &ast::SCon, node: &SyntaxNode) -> Result<f64> {
    let text = scon.token.text();
    text.replace('~', "-")
        .parse::<f64>()
        .map_err(|_| Error::at(node, format!("`{text}` is not a valid real")))
}

/// The value of a string literal: the token's text with its escapes resolved.
///
/// The lexer already worked this out in order to accept the token, and then threw
/// it away — so `lex_util`, which is that same code, is asked again here. Doing it
/// that way is what keeps `\n`, `\ddd`, `\^c` and multi-line continuations meaning
/// in the AST exactly what they meant to the lexer.
fn lower_string(scon: &ast::SCon, node: &SyntaxNode) -> Result<String> {
    let text = scon.token.text();
    if has_u_escape(text.as_bytes()) {
        return unsupported(node, "`\\uXXXX` escapes");
    }
    let mut idx = 0;
    let res = lex_util::string::get(&mut idx, text.as_bytes());
    match res.actual {
        Some(value) if idx == text.len() && res.errors.is_empty() => Ok(value),
        _ => Err(Error::at(node, format!("`{text}` is not a valid string"))),
    }
}

/// Whether the literal spelled by `bs` contains a `\uXXXX` escape.
///
/// The one escape `lower_string` can't take the lexer's word for: `lex_util`
/// assembles its four hex digits with `<< 2` where it means `<< 4`, and writes
/// the result as raw bytes rather than as UTF-8, so `"é"` silently lowers to
/// the wrong character rather than to `é`. Turning it down is the honest answer
/// until that's fixed upstream — a stepper that shows you the wrong character is
/// worse than one that says it can't.
///
/// Only the escape's *first* byte can be a `u`, so this walks the escapes rather
/// than searching for the pair: `\\up` is a backslash followed by "up", and a
/// `\...\` gap's closing backslash is likewise not the start of an escape.
fn has_u_escape(bs: &[u8]) -> bool {
    let mut i = 0;
    while i < bs.len() {
        if bs[i] != b'\\' {
            i += 1;
            continue;
        }
        match bs.get(i + 1) {
            None => return false,
            Some(b'u') => return true,
            // A gap runs to its closing backslash, which this consumes along with
            // it so what follows is read as ordinary content.
            Some(b) if b.is_ascii_whitespace() => {
                i += 2;
                while i < bs.len() && bs[i] != b'\\' {
                    i += 1;
                }
                i += 1;
            }
            Some(_) => i += 2,
        }
    }
    false
}
