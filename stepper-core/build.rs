use cfgrammar::yacc::YaccKind;
use lrlex::CTLexerBuilder;
use lrpar::RecoveryKind;

fn main() {
    CTLexerBuilder::new()
        .lrpar_config(|ctp| {
            // RecoveryKind::None: we only need a clean success/failure result, not
            // interactive error recovery. This also avoids lrpar's recovery-search
            // time budget, which calls std::time::Instant::now() internally and
            // panics on wasm32-unknown-unknown (no clock on that target).
            ctp.yacckind(YaccKind::Grmtools)
                .recoverer(RecoveryKind::None)
                .grammar_in_src_dir("frontend/grammar.y")
                .unwrap()
        })
        .lexer_in_src_dir("frontend/grammar.l")
        .unwrap()
        .build()
        .unwrap();
}
