use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use linkme::distributed_slice;
use models::rhoapi::Par;
use tokio::task::spawn_blocking;

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::io::bulk::{
    self, hex_root, parse_hex_root, validate_namespace, validate_stage_id, BulkTreeError,
};
use crate::rust::interpreter::io::costs::fs_bulk_apply_cost;
use crate::rust::interpreter::io::errors::{
    fserr_to_code, join_err_abort, FserrCode, FSERR_BAD_ARG, FSERR_BUSY,
    FSERR_CODE_CONSENSUS_DIVERGENCE, FSERR_IO, FSERR_NOT_FOUND, FSERR_QUARANTINE,
    FSERR_QUOTA_EXCEEDED, FSERR_UNSUPPORTED,
};
use crate::rust::interpreter::io::handler_trait::{
    dispatch_via_trait_owned, FsHandler, FsHandlerEntry, HandlerFamily, HandlerReply, JournalPath,
    SyscallCtx, FS_HANDLERS,
};
use crate::rust::interpreter::io::handlers::helpers::{
    finalize_failure_journal_via_table, journal_bulk_apply_via_table,
};
use crate::rust::interpreter::io::path::canonicalize_lexical;
use crate::rust::interpreter::io::response::{err, extract_err_code, ok_string};
use crate::rust::interpreter::io::{resolve_cmode, ConsensusMode};
use crate::rust::interpreter::rho_type::RhoString;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

pub struct FsBulkApplyHandler;

pub struct FsBulkApplyArgs {
    root: String,
    stage_id: String,
    namespace: String,
    expected: [u8; 32],
    base: [u8; 32],
    cmode: ConsensusMode,
}

fn staging_rel(stage_id: &str) -> String {
    format!("{}/{}/{}", bulk::CONTROL_DIR, bulk::STAGING_DIR, stage_id)
}

fn tree_err_code(e: &BulkTreeError) -> FserrCode {
    match e {
        BulkTreeError::Io { message, .. } if message == "staged tree not present" => {
            FSERR_NOT_FOUND
        }
        BulkTreeError::Io { .. } => FSERR_IO,
        BulkTreeError::Malformed { .. }
        | BulkTreeError::MissingRootFile
        | BulkTreeError::RootMismatch { .. }
        | BulkTreeError::ListingMismatch(_)
        | BulkTreeError::InvalidName(_) => FSERR_BAD_ARG,
        BulkTreeError::BaseMismatch { .. } => FSERR_BUSY,
        BulkTreeError::Symlink(_) => FSERR_QUARANTINE,
    }
}

impl FsHandler for FsBulkApplyHandler {
    const NAME: &'static str = "fs_bulk_apply";
    const ARITY: usize = 7;
    const VERIFYING: bool = true;

    type Args = FsBulkApplyArgs;

    fn parse_content(args: &[Par]) -> Result<FsBulkApplyArgs, Box<HandlerReply>> {
        let shape = "expected 5 String args + cmode";
        let [root_par, stage_par, ns_par, expected_par, base_par, cmode_par] = args else {
            return Err(HandlerReply::boxed_err(FSERR_BAD_ARG, shape));
        };
        let cmode = resolve_cmode(cmode_par).ok_or_else(|| {
            HandlerReply::boxed_err(
                FSERR_BAD_ARG,
                "cmode must be String \"oracular\" or \"consensus\"",
            )
        })?;
        if cmode != ConsensusMode::Consensus {
            return Err(HandlerReply::boxed_err(
                FSERR_UNSUPPORTED,
                "bulk apply requires consensus mode",
            ));
        }
        let (Some(root), Some(stage_id), Some(namespace), Some(expected), Some(base)) = (
            RhoString::unapply(root_par),
            RhoString::unapply(stage_par),
            RhoString::unapply(ns_par),
            RhoString::unapply(expected_par),
            RhoString::unapply(base_par),
        ) else {
            return Err(HandlerReply::boxed_err(FSERR_BAD_ARG, shape));
        };
        validate_stage_id(&stage_id)
            .map_err(|_| HandlerReply::boxed_err(FSERR_BAD_ARG, "bad stage id"))?;
        validate_namespace(&namespace)
            .map_err(|_| HandlerReply::boxed_err(FSERR_BAD_ARG, "bad namespace"))?;
        let expected = parse_hex_root(&expected)
            .ok_or_else(|| HandlerReply::boxed_err(FSERR_BAD_ARG, "bad expected root"))?;
        let base = parse_hex_root(&base)
            .ok_or_else(|| HandlerReply::boxed_err(FSERR_BAD_ARG, "bad base root"))?;
        Ok(FsBulkApplyArgs {
            root,
            stage_id,
            namespace,
            expected,
            base,
            cmode,
        })
    }

    fn pre_charge_cost() -> Cost { fs_bulk_apply_cost() }

    fn pre_syscall<'a>(
        ctx: SyscallCtx<'a>,
        args: &'a FsBulkApplyArgs,
    ) -> Pin<Box<dyn Future<Output = Result<(), Box<HandlerReply>>> + Send + 'a>> {
        Box::pin(async move {
            let Ok(staging_canon) = canonicalize_lexical(&args.root, &staging_rel(&args.stage_id))
            else {
                return Ok(());
            };
            let Ok(target_canon) = canonicalize_lexical(&args.root, &args.namespace) else {
                return Ok(());
            };
            if journal_bulk_apply_via_table(
                ctx.handles,
                args.cmode,
                staging_canon,
                target_canon,
                args.expected,
                ctx.ack,
            )
            .await
            .is_err()
            {
                return Err(HandlerReply::boxed_err(
                    FSERR_QUOTA_EXCEEDED,
                    "WAL cap exceeded",
                ));
            }
            Ok(())
        })
    }

    fn dispatch<'a>(
        ctx: SyscallCtx<'a>,
        args: FsBulkApplyArgs,
    ) -> Pin<Box<dyn Future<Output = HandlerReply> + Send + 'a>> {
        Box::pin(async move {
            let logical = PathBuf::from(&args.root);
            let (root_pb, _expected_id) = ctx.handles.root_registry.resolve_or_identity(&logical);
            #[allow(clippy::result_large_err)]
            let r = spawn_blocking(move || -> Result<Par, Par> {
                let staging = bulk::staging_path(&root_pb, &args.stage_id);
                let retired = bulk::retired_path(&root_pb, &args.stage_id);
                let target = root_pb.join(&args.namespace);
                match bulk::swap_in(&staging, &target, &retired, &args.expected, &args.base) {
                    Ok(_) => Ok(ok_string(hex_root(&args.expected))),
                    Err(e) => Err(err(tree_err_code(&e), e.to_string())),
                }
            })
            .await;
            match r {
                Err(je) => join_err_abort(je),
                Ok(Ok(par)) => HandlerReply::ok(par),
                Ok(Err(par)) => HandlerReply::Err(par),
            }
        })
    }

    fn resolve_replay_cmode<'a>(
        _ctx: SyscallCtx<'a>,
        raw_args: &'a [Par],
    ) -> Pin<Box<dyn Future<Output = Option<ConsensusMode>> + Send + 'a>> {
        Box::pin(async move {
            let [_, _, _, _, _, cmode_par] = raw_args else {
                return None;
            };
            resolve_cmode(cmode_par)
        })
    }

    fn journal<'a>(
        ctx: SyscallCtx<'a>,
        _raw_args: &'a [Par],
        path: JournalPath<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            if path.is_divergence() {
                finalize_failure_journal_via_table(
                    ctx.handles,
                    FSERR_CODE_CONSENSUS_DIVERGENCE,
                    ctx.ack,
                );
            } else {
                let reply = path.produce_reply();
                if let Some(code_str) = extract_err_code(std::slice::from_ref(reply)) {
                    finalize_failure_journal_via_table(
                        ctx.handles,
                        fserr_to_code(&code_str),
                        ctx.ack,
                    );
                }
            }
        })
    }
}

#[distributed_slice(FS_HANDLERS)]
static FS_BULK_APPLY_ENTRY: FsHandlerEntry = FsHandlerEntry {
    name: <FsBulkApplyHandler as FsHandler>::NAME,
    arity: <FsBulkApplyHandler as FsHandler>::ARITY,
    verifying: <FsBulkApplyHandler as FsHandler>::VERIFYING,
    dispatch: |fs, args| Box::pin(dispatch_via_trait_owned::<FsBulkApplyHandler>(fs, args)),
    urn_suffix: "bulkApply",
    fixed_channel: FixedChannels::fs_bulk_apply,
    body_ref: BodyRefs::FS_BULK_APPLY,
    family: HandlerFamily::Mutation,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Par { RhoString::create_par(v.to_string()) }

    fn valid() -> Vec<Par> {
        vec![
            s("/@bundle/import"),
            s(&"a".repeat(64)),
            s("osm/planet"),
            s(&"b".repeat(64)),
            s(&"0".repeat(64)),
            s("consensus"),
        ]
    }

    #[test]
    fn consts_wire_through() {
        assert_eq!(FsBulkApplyHandler::NAME, "fs_bulk_apply");
        assert_eq!(FsBulkApplyHandler::ARITY, 7);
        assert!(FsBulkApplyHandler::VERIFYING);
    }

    #[test]
    fn parse_accepts_valid_args() {
        let a = FsBulkApplyHandler::parse_content(&valid())
            .ok()
            .expect("parses");
        assert_eq!(a.namespace, "osm/planet");
        assert_eq!(a.expected, [0xbb; 32]);
        assert_eq!(a.base, [0u8; 32]);
    }

    #[test]
    fn parse_rejects_oracular_and_bad_fields() {
        for (i, bad) in [
            (1, "short"),
            (2, "../escape"),
            (2, ".bulk/staging"),
            (3, "zz"),
            (4, "B".repeat(64).as_str()),
            (5, "oracular"),
        ] {
            let mut a = valid();
            a[i] = s(bad);
            assert!(
                FsBulkApplyHandler::parse_content(&a).is_err(),
                "slot {i} accepted {bad}"
            );
        }
        let short: Vec<Par> = valid().into_iter().take(5).collect();
        assert!(FsBulkApplyHandler::parse_content(&short).is_err());
    }

    #[test]
    fn error_codes_classify() {
        assert_eq!(
            tree_err_code(&BulkTreeError::BaseMismatch {
                expected: String::new(),
                actual: String::new()
            }),
            FSERR_BUSY
        );
        assert_eq!(
            tree_err_code(&BulkTreeError::Io {
                path: String::new(),
                message: "staged tree not present".into()
            }),
            FSERR_NOT_FOUND
        );
        assert_eq!(
            tree_err_code(&BulkTreeError::Symlink(String::new())),
            FSERR_QUARANTINE
        );
    }

    #[test]
    fn pre_charge_cost_delegates() {
        assert_eq!(
            FsBulkApplyHandler::pre_charge_cost().value,
            fs_bulk_apply_cost().value
        );
    }
}
