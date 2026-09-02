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

use sml_syntax::ast::{self, AstNode as _};
use sml_syntax::kind::SyntaxNode;

use super::ast::{
    BinOp, Binder, Decl, Expr, ExprKind, Pattern, PatternBase, Program, Type, ValDecl,
};
use super::elaborate::{FunClause, elaborate};

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
    lower_decs(root.decs())
}

/// The declarations of a file or a `let`, flattened.
///
/// Millet nests these three deep — a `Dec` holds a `DecWithTail` of `DecInSeq`s,
/// which is where SML's optional `;` separators and `sharing` tails live — and a
/// whole program usually arrives as one `Dec` with many `DecInSeq`s inside it.
/// None of that structure survives into `Program`, which is a flat `Vec<Decl>`.
fn lower_decs(decs: impl Iterator<Item = ast::Dec>) -> Result<Vec<Decl>> {
    let mut out = Vec::new();
    for dec in decs {
        let tail = require(dec.dec_with_tail(), dec.syntax(), "a declaration")?;
        for seq in tail.dec_in_seqs() {
            let one = require(seq.dec_one(), seq.syntax(), "a declaration")?;
            out.push(lower_dec(&one)?);
        }
    }
    Ok(out)
}

fn lower_dec(dec: &ast::DecOne) -> Result<Decl> {
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
            lower_val_bind(&first)
        }
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
                .map(|case| lower_fun_clause(&case))
                .collect::<Result<Vec<_>>>()?;
            if clauses.is_empty() {
                return Err(Error::at(d.syntax(), "expected a clause after `fun`"));
            }
            elaborate(clauses).map_err(|message| Error::at(d.syntax(), message))
        }
        // An expression on its own is not a declaration in SML; millet parses one
        // anyway so that a language server can say something useful about it, and
        // it's also what a mis-parsed application degrades into — `val z = f if b
        // then 1 else 2` comes back as `val z = f` followed by the `if` as its own
        // `ExpDec` — so this arm catches that too.
        ast::DecOne::ExpDec(d) => Err(Error::at(
            d.syntax(),
            "an expression is not a declaration: bind it with `val`",
        )),
        _ => unsupported(dec.syntax(), dec_description(dec)),
    }
}

/// How to name a declaration form we don't implement, in a message.
fn dec_description(dec: &ast::DecOne) -> &'static str {
    match dec {
        ast::DecOne::HoleDec(_) => "`...` holes",
        ast::DecOne::TyDec(_) => "`type` declarations",
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
        ast::DecOne::ValDec(_) | ast::DecOne::FunDec(_) | ast::DecOne::ExpDec(_) => {
            unreachable!("handled in lower_dec")
        }
    }
}

fn lower_val_bind(bind: &ast::ValBind) -> Result<Decl> {
    let pat = lower_pat(&require(bind.pat(), bind.syntax(), "a pattern")?)?;
    let eq_exp = require(bind.eq_exp(), bind.syntax(), "`= <expression>`")?;
    let expr = lower_exp(&require(eq_exp.exp(), eq_exp.syntax(), "an expression")?)?;
    let val_decl = ValDecl { pat, expr };
    Ok(if bind.rec_kw().is_some() {
        Decl::ValRecDecl(val_decl)
    } else {
        Decl::ValDecl(val_decl)
    })
}

/// One clause of a `fun`. [`super::elaborate`] turns the collected clauses into
/// the `val`/`val rec` they stand for.
fn lower_fun_clause(case: &ast::FunBindCase) -> Result<FunClause> {
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
        .map(|pat| lower_pat(&pat))
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
            lower_ty(&ty)
        })
        .transpose()?;
    let eq_exp = require(case.eq_exp(), case.syntax(), "`= <expression>`")?;
    let body = lower_exp(&require(eq_exp.exp(), eq_exp.syntax(), "an expression")?)?;
    Ok(FunClause {
        name,
        params,
        result_ty,
        body,
    })
}

fn lower_exp(exp: &ast::Exp) -> Result<Expr> {
    let kind = match exp {
        ast::Exp::SConExp(e) => {
            let scon = require(e.s_con(), e.syntax(), "a constant")?;
            ExprKind::IntConst(lower_int(&scon, e.syntax())?)
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
            return lower_exp(&require(e.exp(), e.syntax(), "an expression")?);
        }
        ast::Exp::TupleExp(e) => {
            let items = e
                .exp_args()
                .map(|arg| lower_exp(&require(arg.exp(), arg.syntax(), "an expression")?))
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
            let arg = lower_exp(&require(e.arg(), e.syntax(), "an argument")?)?;
            if is_named(&func, "~") {
                ExprKind::Neg(Box::new(arg))
            } else {
                ExprKind::App(Box::new(lower_exp(&func)?), Box::new(arg))
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
            let lhs = lower_exp(&require(e.lhs(), e.syntax(), "an expression")?)?;
            let rhs = lower_exp(&require(e.rhs(), e.syntax(), "an expression")?)?;
            ExprKind::BinOp(op, Box::new(lhs), Box::new(rhs))
        }
        ast::Exp::AndalsoExp(e) => {
            let lhs = lower_exp(&require(e.lhs(), e.syntax(), "an expression")?)?;
            let rhs = lower_exp(&require(e.rhs(), e.syntax(), "an expression")?)?;
            ExprKind::AndAlso(Box::new(lhs), Box::new(rhs))
        }
        ast::Exp::OrelseExp(e) => {
            let lhs = lower_exp(&require(e.lhs(), e.syntax(), "an expression")?)?;
            let rhs = lower_exp(&require(e.rhs(), e.syntax(), "an expression")?)?;
            ExprKind::OrElse(Box::new(lhs), Box::new(rhs))
        }
        ast::Exp::IfExp(e) => {
            let cond = lower_exp(&require(e.cond(), e.syntax(), "a condition")?)?;
            let yes = lower_exp(&require(e.yes(), e.syntax(), "a `then` branch")?)?;
            let no = lower_exp(&require(e.no(), e.syntax(), "an `else` branch")?)?;
            ExprKind::If(Box::new(cond), Box::new(yes), Box::new(no))
        }
        ast::Exp::CaseExp(e) => {
            let scrutinee = lower_exp(&require(e.exp(), e.syntax(), "an expression")?)?;
            let matcher = require(e.matcher(), e.syntax(), "match arms")?;
            ExprKind::Match(Box::new(scrutinee), lower_matcher(&matcher)?)
        }
        ast::Exp::FnExp(e) => {
            let matcher = require(e.matcher(), e.syntax(), "match arms")?;
            ExprKind::Lambda(lower_matcher(&matcher)?)
        }
        ast::Exp::LetExp(e) => {
            let decls = lower_decs(e.decs())?;
            let mut body = e.exps_in_seq();
            let first = require(body.next(), e.syntax(), "an expression after `in`")?;
            if body.next().is_some() {
                return unsupported(e.syntax(), "`;` expression sequences");
            }
            let body = lower_exp(&require(first.exp(), first.syntax(), "an expression")?)?;
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

fn lower_matcher(matcher: &ast::Matcher) -> Result<Vec<(Pattern, Expr)>> {
    matcher
        .arms()
        .map(|arm| {
            let pat = lower_pat(&require(arm.pat(), arm.syntax(), "a pattern")?)?;
            let exp = lower_exp(&require(arm.exp(), arm.syntax(), "an expression")?)?;
            Ok((pat, exp))
        })
        .collect()
}

fn lower_pat(pat: &ast::Pat) -> Result<Pattern> {
    let base = match pat {
        ast::Pat::WildcardPat(_) => PatternBase::Wildcard,
        ast::Pat::SConPat(p) => {
            let scon = require(p.s_con(), p.syntax(), "a constant")?;
            PatternBase::IntConst(lower_int(&scon, p.syntax())?)
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
            return lower_pat(&require(p.pat(), p.syntax(), "a pattern")?);
        }
        ast::Pat::TuplePat(p) => {
            let items = p
                .pat_args()
                .map(|arg| lower_pat(&require(arg.pat(), arg.syntax(), "a pattern")?))
                .collect::<Result<Vec<_>>>()?;
            match items.is_empty() {
                true => PatternBase::Unit,
                false => PatternBase::Tuple(items),
            }
        }
        // The annotation attaches to the pattern already built, keeping its id —
        // `x` and `x : int` are the same place on screen.
        ast::Pat::TypedPat(p) => {
            let inner = lower_pat(&require(p.pat(), p.syntax(), "a pattern")?)?;
            let ty = lower_ty(&require(p.ty(), p.syntax(), "a type")?)?;
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

fn lower_ty(ty: &ast::Ty) -> Result<Type> {
    match ty {
        ast::Ty::ConTy(t) => {
            if t.ty_seq().is_some() {
                return unsupported(t.syntax(), "type constructors with arguments");
            }
            let path = require(t.path(), t.syntax(), "a type name")?;
            match single_name(&path, t.syntax())?.as_str() {
                "int" => Ok(Type::Int),
                "bool" => Ok(Type::Bool),
                "unit" => Ok(Type::Unit),
                name => unsupported(t.syntax(), &format!("the type `{name}`")),
            }
        }
        ast::Ty::FnTy(t) => {
            let param = lower_ty(&require(t.param(), t.syntax(), "a type")?)?;
            let res = lower_ty(&require(t.res(), t.syntax(), "a type")?)?;
            Ok(Type::Arrow(Box::new(param), Box::new(res)))
        }
        // `t1 * t2 * t3` arrives as one head plus a list of `* t`, and stays flat:
        // `Type::Product` is n-ary, and SML's `*` doesn't associate anyway.
        ast::Ty::TupleTy(t) => {
            let head = lower_ty(&require(t.ty(), t.syntax(), "a type")?)?;
            let mut parts = vec![Box::new(head)];
            for star in t.star_tys() {
                let ty = require(star.ty(), star.syntax(), "a type")?;
                parts.push(Box::new(lower_ty(&ty)?));
            }
            match <[Box<Type>; 1]>::try_from(parts) {
                Ok([only]) => Ok(*only),
                Err(parts) => Ok(Type::Product(parts)),
            }
        }
        ast::Ty::ParenTy(t) => lower_ty(&require(t.ty(), t.syntax(), "a type")?),
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

/// The value of an integer literal. `~` is SML's minus sign and `0x` its hex
/// prefix; every other special constant is a form this crate has no value for.
fn lower_int(scon: &ast::SCon, node: &SyntaxNode) -> Result<i64> {
    match scon.kind {
        ast::SConKind::IntLit => {}
        ast::SConKind::RealLit => return unsupported(node, "real literals"),
        ast::SConKind::WordLit => return unsupported(node, "word literals"),
        ast::SConKind::CharLit => return unsupported(node, "character literals"),
        ast::SConKind::StringLit => return unsupported(node, "string literals"),
    }
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
