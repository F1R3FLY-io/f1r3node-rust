use std::collections::HashMap;

use models::rhoapi::Par;
use rholang_parser::ast::{AnnProc, Bind, Id, Name, SendType, SyncSendCont};
use uuid::Uuid;

use crate::rust::interpreter::compiler::exports::{ProcVisitInputs, ProcVisitOutputs};
use crate::rust::interpreter::compiler::normalize_drive::{
    norm_drive_from, NormVal, NormWork, Step,
};
use crate::rust::interpreter::errors::InterpreterError;

/// `x!?(P)`, a pure **desugaring**: `new r in { x!(*r, P) | for (_ <- r) { K } }`.
///
/// There is no continuation and no combine half. The recursive form ended in an
/// unconditional `normalize_ann_proc(&p_new, input, …)` — a proper tail call —
/// so the machine simply *replaces* the current obligation with the rewritten
/// node ([`Step::Tail`]). A chain of desugarings is therefore flat: `let`
/// rewriting to `match` rewriting to `for` costs three work-stack pops and no
/// frames at all.
///
/// ⚠ The rewritten node is built out of locals, and it survives on the work
/// stack because `ast_builder().alloc_*` interns each `Proc` in the parser arena
/// for `'ast`; only the two-word `AnnProc` wrapper is local, and `AnnProc` is
/// `Copy`.
#[inline(never)]
pub(crate) fn descend_p_send_sync<'ast>(
    channel: &'ast Name<'ast>,
    messages: &'ast rholang_parser::ast::ProcList<'ast>,
    cont: &SyncSendCont<'ast>,
    span: &rholang_parser::SourceSpan,
    input: ProcVisitInputs,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Step<'ast> {
    let identifier = Uuid::new_v4().to_string();

    // Allocate identifier string in the parser's string arena
    let identifier_str = parser.ast_builder().alloc_str(&identifier);

    // Create variable name for the response channel
    let name_var = rholang_parser::ast::Name::NameVar(rholang_parser::ast::Var::Id(Id {
        name: identifier_str,
        pos: span.start,
    }));

    // Build the send process: channel!(name_var, ...messages)
    let send: AnnProc = {
        let mut listproc = Vec::new();

        // Add the response channel name as first argument
        listproc.push(AnnProc {
            proc: parser.ast_builder().alloc_eval(name_var),
            span: *span,
        });

        // Add the original messages
        for msg in messages.iter() {
            listproc.push(*msg);
        }

        AnnProc {
            proc: parser
                .ast_builder()
                .alloc_send(SendType::Single, *channel, &listproc),
            span: *span,
        }
    };

    // Build the receive process: for (_ <- name_var) { cont }
    let receive: AnnProc = {
        // Create wildcard pattern
        let wildcard = rholang_parser::ast::Name::NameVar(rholang_parser::ast::Var::Wildcard);

        // Create bind for the pattern: _ <- name_var
        let bind = Bind::Linear {
            lhs: rholang_parser::ast::Names {
                names: smallvec::SmallVec::from_vec(vec![wildcard]),
                remainder: None,
            },
            rhs: rholang_parser::ast::Source::Simple { name: name_var },
        };

        // Create receipt containing the bind
        let receipt: smallvec::SmallVec<[Bind<'ast>; 1]> = smallvec::SmallVec::from_vec(vec![bind]);
        let receipts: smallvec::SmallVec<[smallvec::SmallVec<[Bind<'ast>; 1]>; 1]> =
            smallvec::SmallVec::from_vec(vec![receipt]);

        // Get the continuation process
        let cont_proc = match cont {
            SyncSendCont::Empty => AnnProc {
                proc: parser.ast_builder().const_nil(),
                span: *span,
            },
            SyncSendCont::NonEmpty(proc) => *proc,
        };

        AnnProc {
            proc: parser.ast_builder().alloc_for(receipts, cont_proc),
            span: *span,
        }
    };

    // Create name declaration for the new variable
    let name_decl = rholang_parser::ast::NameDecl {
        id: Id {
            name: identifier_str,
            pos: span.start,
        },
        uri: None,
    };

    // Build Par of send and receive
    let p_par = AnnProc {
        proc: parser.ast_builder().alloc_par(send, receive),
        span: *span,
    };

    // Build New process: new name_var in { send | receive }
    let p_new = AnnProc {
        proc: parser.ast_builder().alloc_new(p_par, vec![name_decl]),
        span: *span,
    };
    Step::Tail(NormWork::Proc {
        proc: p_new,
        input,
    })
}

/// `x!?(P)` on a **fresh** drive. Only the unit tests enter here; the dispatch
/// pushes [`descend_p_send_sync`]'s `Step` onto the drive it is already on.
pub fn normalize_p_send_sync<'ast>(
    channel: &'ast Name<'ast>,
    messages: &'ast rholang_parser::ast::ProcList<'ast>,
    cont: &SyncSendCont<'ast>,
    span: &rholang_parser::SourceSpan,
    input: ProcVisitInputs,
    env: &HashMap<String, Par>,
    parser: &'ast rholang_parser::RholangParser<'ast>,
) -> Result<ProcVisitOutputs, InterpreterError> {
    let step = descend_p_send_sync(channel, messages, cont, span, input, parser);
    norm_drive_from(step, env, parser).map(NormVal::into_proc)
}


#[cfg(test)]
mod tests {
    use models::rhoapi::Par;

    use super::*;
    use crate::rust::interpreter::compiler::exports::{BoundMapChain, FreeMap};
    use crate::rust::interpreter::compiler::normalize::VarSort;

    #[test]
    fn p_send_sync_should_normalize_a_basic_send_sync() {
        use rholang_parser::ast::{Name, Var};
        use rholang_parser::{SourcePos, SourceSpan};

        fn inputs() -> ProcVisitInputs {
            ProcVisitInputs {
                par: Par::default(),
                bound_map_chain: BoundMapChain::new(),
                free_map: FreeMap::<VarSort>::new(),
            }
        }

        let env = HashMap::<String, Par>::new();
        let parser = rholang_parser::RholangParser::new();

        let channel = Name::NameVar(Var::Wildcard);

        let messages = smallvec::SmallVec::new();

        let cont = rholang_parser::ast::SyncSendCont::Empty;

        let span = SourceSpan {
            start: SourcePos { line: 3, col: 3 },
            end: SourcePos { line: 3, col: 3 },
        };

        let result =
            normalize_p_send_sync(&channel, &messages, &cont, &span, inputs(), &env, &parser);
        assert!(result.is_ok());
    }
}
