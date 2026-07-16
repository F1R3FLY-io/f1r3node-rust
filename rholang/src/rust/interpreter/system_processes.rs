use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::hash::keccak256::Keccak256;
use crypto::rust::hash::sha_256::Sha256Hasher;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::ed25519::Ed25519;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::Signed;
use k256::ecdsa::signature::hazmat::PrehashSigner;
use k256::ecdsa::{Signature, SigningKey};
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance::GPrivateBody;
use models::rhoapi::{Bundle, ETuple, Expr, GPrivate, GUnforgeable, ListParWithRandom, Par, Var};
use models::rust::casper::protocol::casper_message;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::rholang::implicits::single_expr;
use models::rust::utils::{new_gbool_par, new_gbytearray_par, new_gsys_auth_token_par};
use prost::Message;
use shared::rust::{BitSet, Byte};

use super::contract_call::ContractCall;
use super::dispatch::RhoDispatch;
use super::errors::{illegal_argument_error, InterpreterError};
use super::grpc_client_service::GrpcClientService;
use super::ollama_service::{ChatMessage, SharedOllamaService};
use super::openai_service::SharedOpenAIService;
use super::pretty_printer::PrettyPrinter;
use super::registry::registry::Registry;
use super::registry::{semver, versioned_urn};
use super::rho_runtime::RhoISpace;
use super::rho_type::{
    RhoBoolean, RhoByteArray, RhoDeployId, RhoDeployerId, RhoList, RhoName, RhoNumber, RhoString,
    RhoSysAuthToken, RhoUri,
};
use super::util::vault_address::VaultAddress;
use crate::rust::interpreter::chromadb_service::SharedChromaDBService;
#[cfg(feature = "chromadb")]
use crate::rust::interpreter::chromadb_service::{CollectionEntries, Metadata};
#[cfg(feature = "chromadb")]
use crate::rust::interpreter::rho_type::{Extractor, RhoList, RhoNil};

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/SystemProcesses.scala
// NOTE: Not implementing Logger
pub type RhoSysFunction = Box<
    dyn Fn(
            (Vec<ListParWithRandom>, bool, Vec<Par>),
        ) -> Pin<Box<dyn Future<Output = Result<Vec<Par>, InterpreterError>> + Send>>
        + Send
        + Sync,
>;
pub type RhoDispatchMap = Arc<tokio::sync::RwLock<HashMap<i64, RhoSysFunction>>>;
pub type Name = Par;
pub type Arity = i32;
pub type Remainder = Option<Var>;
pub type BodyRef = i64;
pub type Contract = dyn Fn(Vec<ListParWithRandom>);

#[derive(Clone)]
pub struct InvalidBlocks {
    pub invalid_blocks: Arc<tokio::sync::RwLock<Par>>,
}

impl InvalidBlocks {
    pub fn new() -> Self {
        InvalidBlocks {
            invalid_blocks: Arc::new(tokio::sync::RwLock::new(Par::default())),
        }
    }

    pub async fn set_params(&self, invalid_blocks: Par) -> () {
        let mut lock = self.invalid_blocks.write().await;

        *lock = invalid_blocks;
    }
}

pub fn byte_name(b: Byte) -> Par {
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(GPrivateBody(GPrivate { id: vec![b] })),
    }])
}

/// Implementation of the `"matchesVersion"` op. Argument is a 2-tuple
/// Par `(pattern_str, version_str)`; both fields are unwrapped, parsed
/// through `semver`, and matched. Returns `false` on any malformed
/// input so Rholang callers can safely skip non-conforming candidates
/// rather than handling per-error states.
fn matches_version(arg: &Par) -> bool {
    let (pat_par, ver_par) = match unapply_tuple2(arg) {
        Some(t) => t,
        None => return false,
    };
    let (Some(pat_str), Some(ver_str)) =
        (RhoString::unapply(&pat_par), RhoString::unapply(&ver_par))
    else {
        return false;
    };
    let (Ok(pattern), Ok(version)) = (
        semver::parse_pattern(&pat_str),
        semver::parse_version(&ver_str),
    ) else {
        return false;
    };
    pattern.matches(&version)
}

/// Implementation of `"selectBestVersion"`. Arg is a 2-tuple
/// `(pattern_str, versions_list)`. Returns the highest matching
/// version as a Rholang string, or `Nil` if none match or input is
/// malformed.
fn select_best_version(arg: &Par) -> Par {
    let Some((pattern_par, versions_par)) = unapply_tuple2(arg) else {
        return Par::default();
    };
    let Some(pattern_str) = RhoString::unapply(&pattern_par) else {
        return Par::default();
    };
    let Ok(pattern) = semver::parse_pattern(&pattern_str) else {
        return Par::default();
    };
    let Some(version_list) = RhoList::unapply(&versions_par) else {
        return Par::default();
    };

    let mut versions = Vec::with_capacity(version_list.len());
    for p in &version_list {
        if let Some(s) = RhoString::unapply(p) {
            if let Ok(v) = semver::parse_version(&s) {
                versions.push(v);
            }
        }
    }

    match pattern.best_match(&versions) {
        Some(best) => RhoString::create_par(best.to_string()),
        None => Par::default(),
    }
}

fn unapply_tuple2(p: &Par) -> Option<(Par, Par)> {
    let expr = single_expr(p)?;
    if let ExprInstance::ETupleBody(ETuple { ps, .. }) = expr.expr_instance? {
        if ps.len() == 2 {
            return Some((ps[0].clone(), ps[1].clone()));
        }
    }
    None
}

/// Encode a parsed versioned-registry URN as a 5-tuple Par for the
/// Rholang surface. Absent fields (`registry` shape) become `Nil`.
fn parsed_urn_to_tuple(parsed: versioned_urn::ParsedUrn) -> Par {
    let opt_string = |s: Option<String>| s.map(RhoString::create_par).unwrap_or_default();
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::ETupleBody(ETuple {
            ps: vec![
                RhoString::create_par(parsed.namespace),
                RhoString::create_par(parsed.service_version),
                opt_string(parsed.pub_key),
                opt_string(parsed.project_id),
                opt_string(parsed.project_version),
            ],
            locally_free: Vec::new(),
            connective_used: false,
        })),
    }])
}

pub struct FixedChannels;

impl FixedChannels {
    pub fn stdout() -> Par { byte_name(0) }

    pub fn stdout_ack() -> Par { byte_name(1) }

    pub fn stderr() -> Par { byte_name(2) }

    pub fn stderr_ack() -> Par { byte_name(3) }

    pub fn ed25519_verify() -> Par { byte_name(4) }

    pub fn sha256_hash() -> Par { byte_name(5) }

    pub fn keccak256_hash() -> Par { byte_name(6) }

    pub fn blake2b256_hash() -> Par { byte_name(7) }

    pub fn secp256k1_verify() -> Par { byte_name(8) }

    pub fn get_block_data() -> Par { byte_name(10) }

    pub fn get_invalid_blocks() -> Par { byte_name(11) }

    pub fn vault_address() -> Par { byte_name(12) }

    pub fn deployer_id_ops() -> Par { byte_name(13) }

    pub fn reg_lookup() -> Par { byte_name(14) }

    pub fn reg_insert_random() -> Par { byte_name(15) }

    pub fn reg_insert_signed() -> Par { byte_name(16) }

    pub fn reg_ops() -> Par { byte_name(17) }

    pub fn sys_authtoken_ops() -> Par { byte_name(18) }

    // TODO(Step 6): rename to `reg_v1` and re-URN to `rho:registry:1.0.0`
    // once the public entry point of the Versioned Registry FIP lands.
    // Step 3 uses this byte slot as a test-only handle so RhoSpec can
    // exercise insertVersion / deprecateVersion / approveVersion before
    // the resolver and entry-point are wired up.
    pub fn reg_v1_internal() -> Par { byte_name(19) }

    pub fn gpt4() -> Par { byte_name(20) }

    pub fn dalle3() -> Par { byte_name(21) }

    pub fn text_to_audio() -> Par { byte_name(22) }

    /// Versioned-registry helper URN (parses `rho:lib:…` / `rho:serve:…` /
    /// `rho:registry:…` URNs, also supports the legacy `buildUri` op).
    /// Lives alongside `rho:registry:ops` rather than extending it so the
    /// legacy `Registry.rho` keeps calling the same handler.
    pub fn reg_ops_v1() -> Par { byte_name(23) }

    /// Public versioned-registry entry point (`rho:registry:1.0.0`).
    /// The Rholang-side listener in VersionedRegistry.rho accepts a
    /// `(returnCh, notifyCh)` pair and replies with `bundle+{v1Api}` for
    /// the v1 API surface (insertVersion / deprecateVersion /
    /// approveVersion / lookupVersion). The test-only
    /// `rho:registry:v1:internal` URN (at byte 19) stays in place during
    /// the rollout; a follow-up cleanup commit will remove it.
    pub fn reg_v1() -> Par { byte_name(24) }

    pub fn grpc_tell() -> Par { byte_name(25) }

    pub fn dev_null() -> Par { byte_name(26) }

    pub fn abort() -> Par { byte_name(27) }

    pub fn ollama_chat() -> Par { byte_name(28) }

    pub fn ollama_generate() -> Par { byte_name(29) }

    pub fn ollama_models() -> Par { byte_name(30) }

    pub fn deploy_data() -> Par { byte_name(31) }

    pub fn chroma_create_collection() -> Par { byte_name(32) }

    pub fn chroma_get_collection_meta() -> Par { byte_name(33) }

    pub fn chroma_upsert_entries() -> Par { byte_name(34) }

    pub fn chroma_query() -> Par { byte_name(35) }

    pub fn chroma_delete_documents() -> Par { byte_name(36) }

    /// Unified URN-binding dispatcher. The handler `registry_lookup`
    /// (see below) serves any URN: legacy URNs are resolved by
    /// consulting `ProcessContext::urn_map`; versioned URNs
    /// (`rho:lib:…` / `rho:serve:…` / `rho:registry:<ver>`) are
    /// delegated to the Rholang-side `lookupVersion` contract on the
    /// v1 API channel; unknown URNs raise a runtime error.
    ///
    /// Registered under `rho:internal:registry_lookup` so the upcoming
    /// `eval_new` rewrite (a follow-up commit) can target it via that
    /// URN. Exposed publicly for now to make the handler testable;
    /// future cleanup may hide it behind the byte_name once eval_new
    /// stops needing to resolve URNs through `urn_map` itself.
    pub fn registry_lookup() -> Par { byte_name(37) }

    // File I/O native primitives (FIP 2026-02-06 File-I/O). These are
    // internal URNs (`rho:io:fs:native:1.0.0/*`); user-facing Rholang
    // code goes through the `Fs` agent under `rho:io:fs:1.*`.
    pub fn native_open() -> Par { byte_name(38) }
    pub fn native_close() -> Par { byte_name(39) }
    pub fn native_read() -> Par { byte_name(40) }
    pub fn native_write() -> Par { byte_name(41) }
    pub fn native_seek() -> Par { byte_name(42) }
    pub fn native_tell() -> Par { byte_name(43) }
    pub fn native_size() -> Par { byte_name(44) }
    pub fn native_truncate() -> Par { byte_name(45) }
    pub fn native_flush() -> Par { byte_name(46) }
    pub fn native_stat() -> Par { byte_name(47) }
    pub fn native_entries() -> Par { byte_name(48) }
    pub fn native_exists() -> Par { byte_name(49) }
    pub fn native_rename() -> Par { byte_name(50) }
    pub fn native_copy_file() -> Par { byte_name(51) }
    pub fn native_remove_file() -> Par { byte_name(52) }
    pub fn native_remove_dir() -> Par { byte_name(53) }
    pub fn native_chmod() -> Par { byte_name(54) }
    pub fn native_quarantine() -> Par { byte_name(55) }
}

pub struct BodyRefs;

impl BodyRefs {
    pub const STDOUT: i64 = 0;
    pub const STDOUT_ACK: i64 = 1;
    pub const STDERR: i64 = 2;
    pub const STDERR_ACK: i64 = 3;
    pub const ED25519_VERIFY: i64 = 4;
    pub const SHA256_HASH: i64 = 5;
    pub const KECCAK256_HASH: i64 = 6;
    pub const BLAKE2B256_HASH: i64 = 7;
    pub const SECP256K1_VERIFY: i64 = 9;
    pub const GET_BLOCK_DATA: i64 = 11;
    pub const GET_INVALID_BLOCKS: i64 = 12;
    pub const VAULT_ADDRESS: i64 = 13;
    pub const DEPLOYER_ID_OPS: i64 = 14;
    pub const REG_OPS: i64 = 15;
    pub const SYS_AUTHTOKEN_OPS: i64 = 16;
    pub const REG_OPS_V1: i64 = 17;
    pub const GPT4: i64 = 18;
    pub const DALLE3: i64 = 19;
    pub const TEXT_TO_AUDIO: i64 = 20;
    pub const GRPC_TELL: i64 = 23;
    pub const DEV_NULL: i64 = 24;
    pub const ABORT: i64 = 25;
    pub const OLLAMA_CHAT: i64 = 26;
    pub const OLLAMA_GENERATE: i64 = 27;
    pub const OLLAMA_MODELS: i64 = 28;
    pub const DEPLOY_DATA: i64 = 29;
    pub const CHROMA_CREATE_COLLECTION: i64 = 32;
    pub const CHROMA_GET_COLLECTION_META: i64 = 33;
    pub const CHROMA_UPSERT_ENTRIES: i64 = 34;
    pub const CHROMA_QUERY: i64 = 35;
    pub const CHROMA_DELETE_DOCUMENTS: i64 = 36;
    pub const REGISTRY_LOOKUP: i64 = 30;

    // File I/O native primitives (FIP 2026-02-06 File-I/O).
    pub const NATIVE_OPEN: i64 = 37;
    pub const NATIVE_CLOSE: i64 = 38;
    pub const NATIVE_READ: i64 = 39;
    pub const NATIVE_WRITE: i64 = 40;
    pub const NATIVE_SEEK: i64 = 41;
    pub const NATIVE_TELL: i64 = 42;
    pub const NATIVE_SIZE: i64 = 43;
    pub const NATIVE_TRUNCATE: i64 = 44;
    pub const NATIVE_FLUSH: i64 = 45;
    pub const NATIVE_STAT: i64 = 46;
    pub const NATIVE_ENTRIES: i64 = 47;
    pub const NATIVE_EXISTS: i64 = 48;
    pub const NATIVE_RENAME: i64 = 49;
    pub const NATIVE_COPY_FILE: i64 = 50;
    pub const NATIVE_REMOVE_FILE: i64 = 51;
    pub const NATIVE_REMOVE_DIR: i64 = 52;
    pub const NATIVE_CHMOD: i64 = 53;
    pub const NATIVE_QUARANTINE: i64 = 54;
}

pub fn non_deterministic_ops() -> HashSet<i64> {
    HashSet::from([
        BodyRefs::GPT4,
        BodyRefs::DALLE3,
        BodyRefs::TEXT_TO_AUDIO,
        BodyRefs::OLLAMA_CHAT,
        BodyRefs::OLLAMA_GENERATE,
        BodyRefs::OLLAMA_MODELS,
        BodyRefs::GRPC_TELL,
        BodyRefs::CHROMA_QUERY,
        // File I/O native primitives (FIP 2026-02-06). Every
        // primitive that observes or mutates host state goes
        // through the `FailedNonDeterministicCall` path
        // (`dispatch.rs`) so the lead node's result is captured
        // and replayed to followers. `NATIVE_CLOSE` is included
        // for defense-in-depth even though closing a known fd is
        // deterministic given deterministic prior state; the fd
        // itself came from a non-deterministic `NATIVE_OPEN`, so
        // routing the close through the same replay log keeps
        // the fd-table timeline coherent across nodes.
        BodyRefs::NATIVE_OPEN,
        BodyRefs::NATIVE_CLOSE,
        BodyRefs::NATIVE_READ,
        BodyRefs::NATIVE_WRITE,
        BodyRefs::NATIVE_SEEK,
        BodyRefs::NATIVE_TELL,
        BodyRefs::NATIVE_SIZE,
        BodyRefs::NATIVE_TRUNCATE,
        BodyRefs::NATIVE_FLUSH,
        BodyRefs::NATIVE_STAT,
        BodyRefs::NATIVE_ENTRIES,
        BodyRefs::NATIVE_EXISTS,
        BodyRefs::NATIVE_RENAME,
        BodyRefs::NATIVE_COPY_FILE,
        BodyRefs::NATIVE_REMOVE_FILE,
        BodyRefs::NATIVE_REMOVE_DIR,
        BodyRefs::NATIVE_CHMOD,
        BodyRefs::NATIVE_QUARANTINE,
    ])
}

#[derive(Clone)]
pub struct ProcessContext {
    pub space: RhoISpace,
    pub dispatcher: RhoDispatch,
    pub block_data: Arc<tokio::sync::RwLock<BlockData>>,
    pub invalid_blocks: InvalidBlocks,
    pub deploy_data: Arc<tokio::sync::RwLock<DeployData>>,
    /// The runtime's URN → Par binding table, shared with
    /// `DebruijnInterpreter`. Plumbed in so a future
    /// `registry_lookup` system process can serve legacy URNs by
    /// consulting it directly instead of duplicating the table.
    pub urn_map: Arc<HashMap<String, Par>>,
    pub system_processes: SystemProcesses,
}

impl ProcessContext {
    pub fn create(
        space: RhoISpace,
        dispatcher: RhoDispatch,
        block_data: Arc<tokio::sync::RwLock<BlockData>>,
        invalid_blocks: InvalidBlocks,
        deploy_data: Arc<tokio::sync::RwLock<DeployData>>,
        urn_map: Arc<HashMap<String, Par>>,
        openai_service: SharedOpenAIService,
        ollama_service: SharedOllamaService,
        grpc_client_service: GrpcClientService,
        chromadb_service: SharedChromaDBService,
    ) -> Self {
        ProcessContext {
            space: space.clone(),
            dispatcher: dispatcher.clone(),
            block_data: block_data.clone(),
            invalid_blocks,
            deploy_data: deploy_data.clone(),
            urn_map: urn_map.clone(),
            system_processes: SystemProcesses::create(
                dispatcher,
                space,
                block_data,
                deploy_data,
                urn_map,
                openai_service,
                ollama_service,
                grpc_client_service,
                chromadb_service,
            ),
        }
    }
}

pub struct Definition {
    pub urn: String,
    pub fixed_channel: Name,
    pub arity: Arity,
    pub body_ref: BodyRef,
    pub handler: Box<
        dyn FnMut(
                ProcessContext,
            ) -> Box<
                dyn Fn(
                        (Vec<ListParWithRandom>, bool, Vec<Par>),
                    )
                        -> Pin<Box<dyn Future<Output = Result<Vec<Par>, InterpreterError>> + Send>>
                    + Send
                    + Sync,
            > + Send,
    >,
    pub remainder: Remainder,
}

impl Definition {
    pub fn new(
        urn: String,
        fixed_channel: Name,
        arity: Arity,
        body_ref: BodyRef,
        handler: Box<
            dyn FnMut(
                    ProcessContext,
                ) -> Box<
                    dyn Fn(
                            (Vec<ListParWithRandom>, bool, Vec<Par>),
                        ) -> Pin<
                            Box<dyn Future<Output = Result<Vec<Par>, InterpreterError>> + Send>,
                        > + Send
                        + Sync,
                > + Send,
        >,
        remainder: Remainder,
    ) -> Self {
        Definition {
            urn,
            fixed_channel,
            arity,
            body_ref,
            handler,
            remainder,
        }
    }

    pub fn to_dispatch_table(
        &mut self,
        context: ProcessContext,
    ) -> (
        BodyRef,
        Box<
            dyn Fn(
                    (Vec<ListParWithRandom>, bool, Vec<Par>),
                )
                    -> Pin<Box<dyn Future<Output = Result<Vec<Par>, InterpreterError>> + Send>>
                + Send
                + Sync,
        >,
    ) {
        (self.body_ref, (self.handler)(context))
    }

    pub fn to_urn_map(&self) -> (String, Par) {
        let bundle: Par = Par::default().with_bundles(vec![Bundle {
            body: Some(self.fixed_channel.clone()),
            write_flag: true,
            read_flag: false,
        }]);

        (self.urn.clone(), bundle)
    }

    pub fn to_proc_defs(&self) -> (Name, Arity, Remainder, BodyRef) {
        (
            self.fixed_channel.clone(),
            self.arity,
            self.remainder.clone(),
            self.body_ref.clone(),
        )
    }
}

#[derive(Clone)]
pub struct BlockData {
    pub time_stamp: i64,
    pub block_number: i64,
    pub sender: PublicKey,
    pub seq_num: i32,
}

impl BlockData {
    pub fn empty() -> Self {
        BlockData {
            block_number: 0,
            sender: PublicKey::from_bytes(&hex::decode("00").unwrap()),
            seq_num: 0,
            time_stamp: 0,
        }
    }

    pub fn from_block(template: &BlockMessage) -> Self {
        BlockData {
            time_stamp: template.header.timestamp,
            block_number: template.body.state.block_number,
            sender: PublicKey::from_bytes(&template.sender),
            seq_num: template.seq_num,
        }
    }
}

#[derive(Clone)]
pub struct DeployData {
    pub timestamp: i64,
    pub deployer_id: PublicKey,
    pub deploy_id: Vec<u8>,
}

impl DeployData {
    pub fn empty() -> Self {
        DeployData {
            timestamp: 0,
            deployer_id: PublicKey::from_bytes(&[0]),
            deploy_id: vec![0],
        }
    }

    pub fn from_deploy(template: &Signed<casper_message::DeployData>) -> Self {
        DeployData {
            timestamp: template.data.time_stamp,
            deployer_id: template.pk.clone(),
            deploy_id: template.sig.to_vec(),
        }
    }
}

// TODO: Remove Clone
#[derive(Clone)]
pub struct SystemProcesses {
    pub dispatcher: RhoDispatch,
    pub space: RhoISpace,
    pub block_data: Arc<tokio::sync::RwLock<BlockData>>,
    pub deploy_data: Arc<tokio::sync::RwLock<DeployData>>,
    /// Shared with `ProcessContext` and `DebruijnInterpreter`. The
    /// upcoming `registry_lookup` handler consults this when serving
    /// legacy URNs.
    pub urn_map: Arc<HashMap<String, Par>>,
    /// Open-file table for the File-I/O native primitives. Shared
    /// across all clones of this `SystemProcesses` so `nativeOpen`
    /// on one dispatch worker and `nativeClose` on another see the
    /// same fd space. Empty at boot; populated by successful
    /// `nativeOpen` calls.
    pub file_handles: crate::rust::interpreter::io::handle_table::FileHandleTable,
    openai_service: SharedOpenAIService,
    ollama_service: SharedOllamaService,
    grpc_client_service: GrpcClientService,
    pretty_printer: PrettyPrinter,
    #[allow(dead_code)] // Note: This isn't dead when the chromadb flag is used
    chromadb_service: SharedChromaDBService,
}

impl SystemProcesses {
    fn create(
        dispatcher: RhoDispatch,
        space: RhoISpace,
        block_data: Arc<tokio::sync::RwLock<BlockData>>,
        deploy_data: Arc<tokio::sync::RwLock<DeployData>>,
        urn_map: Arc<HashMap<String, Par>>,
        openai_service: SharedOpenAIService,
        ollama_service: SharedOllamaService,
        grpc_client_service: GrpcClientService,
        chromadb_service: SharedChromaDBService,
    ) -> Self {
        SystemProcesses {
            dispatcher,
            space,
            block_data,
            deploy_data,
            urn_map,
            file_handles: crate::rust::interpreter::io::handle_table::FileHandleTable::new(),
            openai_service,
            ollama_service,
            grpc_client_service,
            pretty_printer: PrettyPrinter::new(),
            chromadb_service,
        }
    }

    fn is_contract_call(&self) -> ContractCall {
        ContractCall {
            space: self.space.clone(),
            dispatcher: self.dispatcher.clone(),
        }
    }

    async fn verify_signature_contract(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
        name: &str,
        algorithm: Box<dyn SignaturesAlg>,
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, _, _, vec)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error(name));
        };

        let [data, signature, pub_key, ack] = vec.as_slice() else {
            return Err(illegal_argument_error(name));
        };

        let (Some(data_bytes), Some(signature_bytes), Some(pub_key_bytes)) = (
            RhoByteArray::unapply(data),
            RhoByteArray::unapply(signature),
            RhoByteArray::unapply(pub_key),
        ) else {
            return Err(illegal_argument_error(name));
        };

        let verified = algorithm.verify(&data_bytes, &signature_bytes, &pub_key_bytes);
        let output = vec![Par::default().with_exprs(vec![RhoBoolean::create_expr(verified)])];
        let ret = output.clone();
        produce(&output, ack).await?;
        Ok(ret)
    }

    async fn hash_contract(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
        name: &str,
        algorithm: Box<dyn Fn(Vec<u8>) -> Vec<u8> + Send>,
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, _, _, vec)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error(name));
        };

        let [input, ack] = vec.as_slice() else {
            return Err(illegal_argument_error(name));
        };

        let Some(input) = RhoByteArray::unapply(input) else {
            return Err(illegal_argument_error(name));
        };

        let hash = algorithm(input);
        let output = vec![RhoByteArray::create_par(hash)];
        let ret = output.clone();
        produce(&output, ack).await?;
        Ok(ret)
    }

    fn print_std_out(&self, s: &str) -> Result<Vec<Par>, InterpreterError> {
        println!("{}", s);
        Ok(vec![])
    }

    fn print_std_err(&self, s: &str) -> Result<Vec<Par>, InterpreterError> {
        eprintln!("{}", s);
        Ok(vec![])
    }

    pub async fn std_out(
        &mut self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((_, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("std_out"));
        };

        let [arg] = args.as_slice() else {
            return Err(illegal_argument_error("std_out"));
        };

        let str = self.pretty_printer.build_string_from_message(arg);
        self.print_std_out(&str)
    }

    pub async fn std_out_ack(
        mut self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("std_out_ack"));
        };

        let [arg, ack] = args.as_slice() else {
            return Err(illegal_argument_error("std_out_ack"));
        };

        let str = self.pretty_printer.build_string_from_message(arg);
        self.print_std_out(&str)?;

        let output = vec![Par::default()];
        let ret = output.clone();
        produce(&output, ack).await?;
        Ok(ret)
    }

    pub async fn std_err(
        &mut self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((_, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("std_err"));
        };

        let [arg] = args.as_slice() else {
            return Err(illegal_argument_error("std_err"));
        };

        let str = self.pretty_printer.build_string_from_message(arg);
        self.print_std_err(&str)
    }

    pub async fn std_err_ack(
        &mut self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("std_err_ack"));
        };

        let [arg, ack] = args.as_slice() else {
            return Err(illegal_argument_error("std_err_ack"));
        };

        let str = self.pretty_printer.build_string_from_message(arg);
        self.print_std_err(&str)?;

        let output = vec![Par::default()];
        let ret = output.clone();
        produce(&output, ack).await?;
        Ok(ret)
    }

    pub async fn vault_address(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("vault_address"));
        };

        let [first_par, second_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("vault_address"));
        };

        let Some(command) = RhoString::unapply(first_par) else {
            return Err(illegal_argument_error("vault_address"));
        };

        let response = match command.as_str() {
            "validate" => {
                match RhoString::unapply(second_par).map(|address| VaultAddress::parse(&address)) {
                    Some(Ok(_)) => Par::default(),
                    Some(Err(err)) => RhoString::create_par(err),
                    None => {
                        // TODO: Invalid type for address should throw error! - OLD
                        Par::default()
                    }
                }
            }

            "fromPublicKey" => match RhoByteArray::unapply(second_par).map(|public_key| {
                VaultAddress::from_public_key(&PublicKey::from_bytes(&public_key))
            }) {
                Some(Some(ra)) => RhoString::create_par(ra.to_base58()),
                _ => Par::default(),
            },

            "fromDeployerId" => {
                match RhoDeployerId::unapply(second_par).map(VaultAddress::from_deployer_id) {
                    Some(Some(ra)) => RhoString::create_par(ra.to_base58()),
                    _ => Par::default(),
                }
            }

            "fromUnforgeable" => {
                match RhoName::unapply(second_par)
                    .map(|gprivate: GPrivate| VaultAddress::from_unforgeable(&gprivate))
                {
                    Some(ra) => RhoString::create_par(ra.to_base58()),
                    None => Par::default(),
                }
            }

            _ => return Err(illegal_argument_error("vault_address")),
        };

        produce(&[response], ack).await
    }

    pub async fn deployer_id_ops(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("deployer_id_ops"));
        };

        let [first_par, second_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("deployer_id_ops"));
        };

        let Some("pubKeyBytes") = RhoString::unapply(first_par).as_deref() else {
            return Err(illegal_argument_error("deployer_id_ops"));
        };

        let response = RhoDeployerId::unapply(second_par)
            .map(RhoByteArray::create_par)
            .unwrap_or_default();

        produce(&[response], ack).await
    }

    pub async fn registry_ops(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("registry_ops"));
        };

        let [first_par, argument, ack] = args.as_slice() else {
            return Err(illegal_argument_error("registry_ops"));
        };

        let Some("buildUri") = RhoString::unapply(first_par).as_deref() else {
            return Err(illegal_argument_error("registry_ops"));
        };

        let response = RhoByteArray::unapply(argument)
            .map(|ba| {
                let hash_key_bytes = Blake2b256::hash(ba);
                RhoUri::create_par(Registry::build_uri(&hash_key_bytes))
            })
            .unwrap_or_default();

        produce(&[response], ack).await
    }

    /// Handler for the versioned registry helper URN
    /// (`rho:registry:ops:1.0.0`). Three ops:
    ///
    /// - `"buildUri"(bytes)` — identical to the legacy `rho:registry:ops`
    ///   path, exposed here so contracts that prefer the versioned form
    ///   don't need to mix surfaces. Returns the `rho:id:…` URI.
    /// - `"parseVersionedUri"(urn)` — splits a `rho:lib:…` / `rho:serve:…`
    ///   / `rho:registry:…` URN into a 5-tuple
    ///   `(namespace, service_version, pub_key, project_id, project_version)`
    ///   where the trailing three are `Nil` for the `rho:registry:` shape.
    ///   Returns `Nil` (an empty Par) on malformed input.
    /// - `"matchesVersion"((pattern, version))` — semver match. Returns
    ///   `true` iff the version satisfies the pattern (`*`, `M.*`,
    ///   `M.m.*`, or exact). Wildcards never match prereleases; an exact
    ///   pattern matches whatever it spells. Returns `false` on
    ///   malformed input rather than failing — the Rholang caller can
    ///   then skip that candidate.
    /// - `"selectBestVersion"((pattern, [version, …]))` — picks the
    ///   highest matching version string from a Rholang list. Returns
    ///   `Nil` if none match or on malformed input. Pushes the semver
    ///   ordering into Rust so the Rholang resolver doesn't have to
    ///   implement comparison itself.
    ///
    /// The legacy `registry_ops` handler is left untouched.
    pub async fn registry_ops_v1(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("registry_ops_v1"));
        };

        let [first_par, argument, ack] = args.as_slice() else {
            return Err(illegal_argument_error("registry_ops_v1"));
        };

        let response = match RhoString::unapply(first_par).as_deref() {
            Some("buildUri") => RhoByteArray::unapply(argument)
                .map(|ba| {
                    let hash_key_bytes = Blake2b256::hash(ba);
                    RhoUri::create_par(Registry::build_uri(&hash_key_bytes))
                })
                .unwrap_or_default(),

            Some("parseVersionedUri") => RhoString::unapply(argument)
                .and_then(|s| versioned_urn::parse_urn(&s))
                .map(parsed_urn_to_tuple)
                .unwrap_or_default(),

            Some("matchesVersion") => RhoBoolean::create_par(matches_version(argument)),

            Some("selectBestVersion") => select_best_version(argument),

            _ => return Err(illegal_argument_error("registry_ops_v1")),
        };

        produce(&[response], ack).await
    }

    /// Unified URN-binding dispatcher. Handles every URN shape:
    ///
    /// - **Legacy URN** (anything in `self.urn_map`, e.g. `rho:io:stdout`):
    ///   produce the stored Par on `ret`. The stored Par is the
    ///   write-only bundle around the URN's fixed channel, exactly what
    ///   `eval_new`'s fast path used to bind directly.
    /// - **Versioned URN** (`rho:lib:…`, `rho:serve:…`, `rho:registry:<ver>`):
    ///   inject `("lookupVersion", urn, Nil, *ret)` onto the v1 API
    ///   channel via the same `produce` capability we'd use to reply
    ///   to `ret` directly. The Rholang `lookupVersion` contract picks
    ///   it up, resolves the version, and produces the resulting `code`
    ///   Par on `ret`. The original requester gets the result.
    /// - **Unknown URN**: return `InterpreterError` — the deploy halts
    ///   with a runtime error, matching the design directive that
    ///   lookup failure is fatal.
    ///
    /// Once `eval_new` is rewritten (a follow-up commit) to synthesize
    /// `new tmpRet in { registryLookup!(URN, *tmpRet) | for(x <- tmpRet) { P } }`
    /// for every `new x(URN)` binding, this handler is the single
    /// dispatch point for URN resolution.
    pub async fn registry_lookup(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("registry_lookup"));
        };

        let [urn_par, ret_par] = args.as_slice() else {
            return Err(illegal_argument_error("registry_lookup"));
        };

        let urn_str = RhoString::unapply(urn_par)
            .or_else(|| RhoUri::unapply(urn_par))
            .ok_or_else(|| {
                illegal_argument_error("registry_lookup: urn arg must be a String or Uri")
            })?;

        // 1. Legacy URN: serve directly from urn_map.
        if let Some(stored) = self.urn_map.get(&urn_str) {
            let stored_clone = stored.clone();
            produce(&[stored_clone], ret_par).await?;
            return Ok(vec![]);
        }

        // 2. Versioned URN: delegate to v1Api's lookupVersion contract.
        //    The produce here goes to the v1Api channel, not to ret;
        //    the contract will produce on ret itself. We re-encode the
        //    URN as a String regardless of whether the caller sent a
        //    String or a Uri Par, because v1Api's `parseVersionedUri`
        //    only accepts String.
        if versioned_urn::parse_urn(&urn_str).is_some() {
            let msg = vec![
                RhoString::create_par("lookupVersion".to_string()),
                RhoString::create_par(urn_str.clone()),
                Par::default(), // Nil notify — the upcoming eval_new
                // rewrite will plumb a per-import
                // notify channel through here.
                ret_par.clone(),
            ];
            produce(&msg, &FixedChannels::reg_v1_internal()).await?;
            return Ok(vec![]);
        }

        // 3. Unknown URN.
        Err(InterpreterError::ReduceError(format!(
            "registry_lookup: unknown URN: {}",
            urn_str
        )))
    }

    pub async fn sys_auth_token_ops(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("sys_auth_token_ops"));
        };

        let [first_par, argument, ack] = args.as_slice() else {
            return Err(illegal_argument_error("sys_auth_token_ops"));
        };

        let Some("check") = RhoString::unapply(first_par).as_deref() else {
            return Err(illegal_argument_error("sys_auth_token_ops"));
        };

        let response = RhoBoolean::create_expr(RhoSysAuthToken::unapply(argument).is_some());
        produce(&[Par::default().with_exprs(vec![response])], ack).await
    }

    pub async fn secp256k1_verify(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        self.verify_signature_contract(contract_args, "secp256k1Verify", Box::new(Secp256k1))
            .await
    }

    pub async fn ed25519_verify(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        self.verify_signature_contract(contract_args, "ed25519Verify", Box::new(Ed25519))
            .await
    }

    pub async fn sha256_hash(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        self.hash_contract(contract_args, "sha256Hash", Box::new(Sha256Hasher::hash))
            .await
    }

    pub async fn keccak256_hash(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        self.hash_contract(contract_args, "keccak256Hash", Box::new(Keccak256::hash))
            .await
    }

    pub async fn blake2b256_hash(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        self.hash_contract(contract_args, "blake2b256Hash", Box::new(Blake2b256::hash))
            .await
    }

    pub async fn get_block_data(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
        block_data: Arc<tokio::sync::RwLock<BlockData>>,
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("get_block_data"));
        };

        let [ack] = args.as_slice() else {
            return Err(illegal_argument_error("get_block_data"));
        };

        let data = block_data.read().await;
        let output = vec![
            Par::default().with_exprs(vec![RhoNumber::create_expr(data.block_number)]),
            Par::default().with_exprs(vec![RhoNumber::create_expr(data.time_stamp)]),
            RhoByteArray::create_par(data.sender.bytes.as_ref().to_vec()),
        ];

        produce(&output, ack).await?;
        Ok(output)
    }

    pub async fn get_deploy_data(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
        deploy_data: Arc<tokio::sync::RwLock<DeployData>>,
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error(
                "get_deploy_data: invalid contract call pattern",
            ));
        };

        let [ack] = args.as_slice() else {
            return Err(illegal_argument_error(
                "get_deploy_data expects exactly 1 argument (ack channel)",
            ));
        };

        let data = deploy_data.read().await;
        let output = vec![
            Par::default().with_exprs(vec![RhoNumber::create_expr(data.timestamp)]),
            RhoDeployerId::create_par(data.deployer_id.bytes.as_ref().to_vec()),
            RhoDeployId::create_par(data.deploy_id.clone()),
        ];

        produce(&output, ack).await?;
        Ok(output)
    }

    pub async fn invalid_blocks(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
        invalid_blocks: &InvalidBlocks,
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("invalid_blocks"));
        };

        let [ack] = args.as_slice() else {
            return Err(illegal_argument_error("invalid_blocks"));
        };

        let invalid_blocks = invalid_blocks.invalid_blocks.read().await.clone();
        produce(&[invalid_blocks.clone()], ack).await?;
        Ok(vec![invalid_blocks])
    }

    pub async fn gpt4(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous_output, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("gpt4"));
        };

        let [prompt_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("gpt4"));
        };

        let Some(prompt) = RhoString::unapply(prompt_par) else {
            return Err(illegal_argument_error("gpt4"));
        };

        if is_replay {
            produce(&previous_output, ack).await?;
            return Ok(previous_output);
        }

        let openai_service = {
            let service_guard = self.openai_service.lock().await;
            service_guard.clone()
        };
        let response = match openai_service.gpt4_chat_completion(&prompt).await {
            Ok(response) => response,
            Err(e) => {
                return Err(InterpreterError::NonDeterministicProcessFailure {
                    cause: Box::new(e),
                    output_not_produced: vec![],
                });
            }
        };

        let output = vec![RhoString::create_par(response)];
        if let Err(e) = produce(&output, ack).await {
            return Err(InterpreterError::ProduceFailureWithOutput {
                cause: Box::new(e),
                output_not_produced: output.iter().map(|p| p.encode_to_vec()).collect(),
            });
        }
        Ok(output)
    }

    pub async fn dalle3(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous_output, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("dalle3"));
        };

        let [prompt_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("dalle3"));
        };

        let Some(prompt) = RhoString::unapply(prompt_par) else {
            return Err(illegal_argument_error("dalle3"));
        };

        if is_replay {
            produce(&previous_output, ack).await?;
            return Ok(previous_output);
        }

        let openai_service = {
            let service_guard = self.openai_service.lock().await;
            service_guard.clone()
        };
        let response = match openai_service.dalle3_create_image(&prompt).await {
            Ok(response) => response,
            Err(e) => {
                return Err(InterpreterError::NonDeterministicProcessFailure {
                    cause: Box::new(e),
                    output_not_produced: vec![],
                });
            }
        };

        let output = vec![RhoString::create_par(response)];
        if let Err(e) = produce(&output, ack).await {
            return Err(InterpreterError::ProduceFailureWithOutput {
                cause: Box::new(e),
                output_not_produced: output.iter().map(|p| p.encode_to_vec()).collect(),
            });
        }
        Ok(output)
    }

    pub async fn text_to_audio(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous_output, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("text_to_audio"));
        };

        let [input_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("text_to_audio"));
        };

        let Some(input) = RhoString::unapply(input_par) else {
            return Err(illegal_argument_error("text_to_audio"));
        };

        if is_replay {
            produce(&previous_output, ack).await?;
            return Ok(previous_output);
        }

        let openai_service = {
            let service_guard = self.openai_service.lock().await;
            service_guard.clone()
        };
        let audio_path = format!("audio_{}.mp3", uuid::Uuid::new_v4());
        let audio_bytes = match openai_service
            .create_audio_speech(&input, &audio_path)
            .await
        {
            Ok(bytes) => bytes,
            Err(e) => {
                return Err(InterpreterError::NonDeterministicProcessFailure {
                    cause: Box::new(e),
                    output_not_produced: vec![],
                });
            }
        };

        let output = vec![RhoByteArray::create_par(audio_bytes)];
        if let Err(e) = produce(&output, ack).await {
            return Err(InterpreterError::ProduceFailureWithOutput {
                cause: Box::new(e),
                output_not_produced: output.iter().map(|p| p.encode_to_vec()).collect(),
            });
        }
        Ok(output)
    }

    pub async fn ollama_chat(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous_output, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("ollama_chat"));
        };

        let [model_par, prompt_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("ollama_chat"));
        };

        if is_replay {
            produce(&previous_output, ack).await?;
            return Ok(previous_output);
        }

        let Some(model) = RhoString::unapply(model_par) else {
            return Err(illegal_argument_error(
                "ollama_chat: model must be a string",
            ));
        };

        let Some(prompt) = RhoString::unapply(prompt_par) else {
            return Err(illegal_argument_error(
                "ollama_chat: prompt must be a string",
            ));
        };

        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: prompt,
        }];

        let ollama_service = {
            let service_guard = self.ollama_service.lock().await;
            service_guard.clone()
        };
        let response = match ollama_service.chat(Some(&model), messages).await {
            Ok(response) => response,
            Err(e) => {
                tracing::error!(error = ?e, "Ollama chat request failed");
                return Err(InterpreterError::NonDeterministicProcessFailure {
                    cause: Box::new(e),
                    output_not_produced: vec![],
                });
            }
        };

        let output = vec![RhoString::create_par(response)];
        if let Err(e) = produce(&output, ack).await {
            return Err(InterpreterError::ProduceFailureWithOutput {
                cause: Box::new(e),
                output_not_produced: output.iter().map(|p| p.encode_to_vec()).collect(),
            });
        }
        Ok(output)
    }

    pub async fn ollama_generate(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous_output, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("ollama_generate"));
        };

        let [model_par, prompt_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("ollama_generate"));
        };

        if is_replay {
            produce(&previous_output, ack).await?;
            return Ok(previous_output);
        }

        let Some(model) = RhoString::unapply(model_par) else {
            return Err(illegal_argument_error(
                "ollama_generate: model must be a string",
            ));
        };

        let Some(prompt) = RhoString::unapply(prompt_par) else {
            return Err(illegal_argument_error(
                "ollama_generate: prompt must be a string",
            ));
        };

        let ollama_service = {
            let service_guard = self.ollama_service.lock().await;
            service_guard.clone()
        };
        let response = match ollama_service.generate(Some(&model), &prompt).await {
            Ok(response) => response,
            Err(e) => {
                tracing::error!(error = ?e, "Ollama generate request failed");
                return Err(InterpreterError::NonDeterministicProcessFailure {
                    cause: Box::new(e),
                    output_not_produced: vec![],
                });
            }
        };

        let output = vec![RhoString::create_par(response)];
        if let Err(e) = produce(&output, ack).await {
            return Err(InterpreterError::ProduceFailureWithOutput {
                cause: Box::new(e),
                output_not_produced: output.iter().map(|p| p.encode_to_vec()).collect(),
            });
        }
        Ok(output)
    }

    pub async fn ollama_models(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous_output, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("ollama_models"));
        };

        let [ack] = args.as_slice() else {
            return Err(illegal_argument_error("ollama_models"));
        };

        if is_replay {
            produce(&previous_output, ack).await?;
            return Ok(previous_output);
        }

        let ollama_service = {
            let service_guard = self.ollama_service.lock().await;
            service_guard.clone()
        };
        let models = match ollama_service.list_models().await {
            Ok(models) => models,
            Err(e) => {
                tracing::error!(error = ?e, "Ollama models list request failed");
                return Err(InterpreterError::NonDeterministicProcessFailure {
                    cause: Box::new(e),
                    output_not_produced: vec![],
                });
            }
        };

        let models_par_list: Vec<Par> = models.into_iter().map(RhoString::create_par).collect();
        let list_expr = Expr {
            expr_instance: Some(ExprInstance::EListBody(models::rhoapi::EList {
                ps: models_par_list,
                locally_free: BitSet::default(),
                connective_used: false,
                remainder: None,
            })),
        };
        let output = vec![Par::default().with_exprs(vec![list_expr])];

        if let Err(e) = produce(&output, ack).await {
            return Err(InterpreterError::ProduceFailureWithOutput {
                cause: Box::new(e),
                output_not_produced: output.iter().map(|p| p.encode_to_vec()).collect(),
            });
        }
        Ok(output)
    }

    pub async fn grpc_tell(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((_produce, is_replay, previous_output, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("grpc_tell"));
        };

        // Handle replay case
        if is_replay {
            tracing::debug!("grpcTell (replay): args: {:?}", args);
            return Ok(previous_output);
        }

        // Handle normal case - expecting clientHost, clientPort, notificationPayload
        // grpcTell is a fire-and-forget mechanism with no ack channel (arity = 3)
        match args.as_slice() {
            [client_host_par, client_port_par, notification_payload_par] => {
                match (
                    RhoString::unapply(client_host_par),
                    RhoNumber::unapply(client_port_par),
                    RhoString::unapply(notification_payload_par),
                ) {
                    (Some(client_host), Some(client_port), Some(notification_payload)) => {
                        // Convert client_port from i64 to u64
                        let port = if client_port < 0 {
                            return Err(InterpreterError::BugFoundError(
                                "Invalid port number: must be non-negative".to_string(),
                            ));
                        } else {
                            client_port as u64
                        };

                        // Use GrpcClientService abstraction for proper NoOp handling on observer nodes
                        match self
                            .grpc_client_service
                            .tell(&client_host, port, &notification_payload)
                            .await
                        {
                            Ok(_) => {
                                tracing::debug!(
                                    "grpcTell: successfully sent to {}:{}",
                                    client_host,
                                    port
                                );
                                Ok(vec![Par::default()])
                            }
                            Err(e) => {
                                tracing::warn!("GrpcClient error: {}", e);
                                Err(InterpreterError::NonDeterministicProcessFailure {
                                    cause: Box::new(InterpreterError::BugFoundError(format!(
                                        "gRPC client error: {}",
                                        e
                                    ))),
                                    output_not_produced: vec![],
                                })
                            }
                        }
                    }
                    _ => {
                        tracing::warn!("grpcTell: invalid argument types: {:?}", args);
                        Err(illegal_argument_error("grpc_tell"))
                    }
                }
            }
            _ => {
                tracing::warn!(
                    "grpcTell: isReplay {} invalid arguments (expected 3): {:?}",
                    is_replay,
                    args
                );
                Err(illegal_argument_error("grpc_tell"))
            }
        }
    }

    pub async fn dev_null(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        if self.is_contract_call().unapply(contract_args).is_none() {
            return Err(illegal_argument_error("dev_null"));
        }

        Ok(vec![])
    }

    /// Execution abort system process.
    ///
    /// Terminates the current Rholang computation immediately when called.
    /// This allows users to explicitly halt program execution, useful for
    /// error handling and controlled termination scenarios.
    ///
    /// Usage in Rholang:
    ///   - `@"rho:execution:abort"!(Nil)` - abort with no reason
    ///   - `@"rho:execution:abort"!("reason")` - abort with a reason string
    ///
    /// Note: The abort process accepts exactly one argument (arity: 1).
    /// Pass `Nil` for no reason, or a descriptive value for debugging.
    ///
    /// @return Never returns - raises UserAbortError to terminate execution
    pub async fn abort(
        &mut self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((_, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(InterpreterError::UserAbortError);
        };

        // Log the abort reason for debugging
        if let Some(arg) = args.first() {
            let str = self.pretty_printer.build_string_from_message(arg);
            tracing::error!(abort_args = %str, "Rholang contract execution aborted");
        }

        Err(InterpreterError::UserAbortError)
    }

    /*
     * The following functions below can be removed once rust-casper calls create_rho_runtime.
     * Until then, they must remain in the rholang directory to avoid circular dependencies.
     */

    // See casper/src/test/scala/coop/rchain/casper/helper/TestResultCollector.scala
    // TODO remove this once Rust node will be completed ( this stuff already moved under Casper, double check related files)
    pub async fn handle_message(
        &self,
        message: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let mut printer = PrettyPrinter::new();

        fn clue_msg(clue: String, attempt: i64) -> String {
            format!("{} (test attempt: {})", clue, attempt)
        }

        if let Some((produce, _, _, assert_par)) = self.is_contract_call().unapply(message) {
            if let Some((test_name, attempt, assertion, clue, ack_channel)) =
                IsAssert::unapply(assert_par.clone())
            {
                if let Some((expected_or_unexpected, equals_or_not_equals_str, actual)) =
                    IsComparison::unapply(assertion.clone())
                {
                    if equals_or_not_equals_str == "==" {
                        let assertion = RhoTestAssertion::RhoAssertEquals {
                            test_name,
                            expected: expected_or_unexpected.clone(),
                            actual: actual.clone(),
                            clue: clue.clone(),
                        };

                        let output = vec![new_gbool_par(assertion.is_success(), Vec::new(), false)];
                        produce(&output, &ack_channel).await?;

                        assert_eq!(
                            printer.build_string_from_message(&actual),
                            printer.build_string_from_message(&expected_or_unexpected),
                            "{}",
                            clue_msg(clue, attempt)
                        );

                        assert_eq!(
                            actual,
                            expected_or_unexpected,
                            "{}",
                            clue_msg(clue, attempt)
                        );
                        Ok(output)
                    } else if equals_or_not_equals_str == "!=" {
                        let assertion = RhoTestAssertion::RhoAssertNotEquals {
                            test_name,
                            unexpected: expected_or_unexpected.clone(),
                            actual: actual.clone(),
                            clue: clue.clone(),
                        };

                        let output = vec![new_gbool_par(assertion.is_success(), Vec::new(), false)];
                        produce(&output, &ack_channel).await?;

                        assert_ne!(
                            printer.build_string_from_message(&actual),
                            printer.build_string_from_message(&expected_or_unexpected),
                            "{}",
                            clue_msg(clue, attempt)
                        );

                        assert_ne!(
                            actual,
                            expected_or_unexpected,
                            "{}",
                            clue_msg(clue, attempt)
                        );
                        Ok(output)
                    } else {
                        Err(illegal_argument_error("handle_message"))
                    }
                } else if let Some(condition) = RhoBoolean::unapply(&assertion) {
                    let output = vec![new_gbool_par(condition, Vec::new(), false)];
                    produce(&output, &ack_channel).await?;

                    assert!(condition, "{}", clue_msg(clue, attempt));
                    Ok(output)
                } else {
                    let output = vec![new_gbool_par(false, Vec::new(), false)];
                    produce(&output, &ack_channel).await?;

                    Err(InterpreterError::BugFoundError(format!(
                        "Failed to evaluate assertion: {:?}",
                        assertion
                    )))
                }
            } else if let Some(_) = IsSetFinished::unapply(assert_par) {
                Ok(vec![])
            } else {
                Err(illegal_argument_error("handle_message"))
            }
        } else {
            Err(illegal_argument_error("handle_message"))
        }
    }

    // See casper/src/test/scala/coop/rchain/casper/helper/RhoLoggerContract.scala

    pub async fn std_log(
        &mut self,
        message: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        if let Some((_, _, _, args)) = self.is_contract_call().unapply(message) {
            match args.as_slice() {
                [log_level_par, par] => {
                    if let Some(log_level) = RhoString::unapply(log_level_par) {
                        let msg = self.pretty_printer.build_string_from_message(par);

                        match log_level.as_str() {
                            "trace" => {
                                tracing::trace!("{}", msg);
                                Ok(vec![])
                            }
                            "debug" => {
                                tracing::debug!("{}", msg);
                                Ok(vec![])
                            }
                            "info" => {
                                tracing::info!("{}", msg);
                                Ok(vec![])
                            }
                            "warn" => {
                                tracing::warn!("{}", msg);
                                Ok(vec![])
                            }
                            "error" => {
                                tracing::error!("{}", msg);
                                Ok(vec![])
                            }
                            _ => Err(illegal_argument_error("std_log")),
                        }
                    } else {
                        Err(illegal_argument_error("std_log"))
                    }
                }
                _ => Err(illegal_argument_error("std_log")),
            }
        } else {
            Err(illegal_argument_error("std_log"))
        }
    }

    // See casper/src/test/scala/coop/rchain/casper/helper/DeployerIdContract.scala

    pub async fn deployer_id_make(
        &mut self,
        message: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        if let Some((produce, _, _, args)) = self.is_contract_call().unapply(message) {
            match args.as_slice() {
                [deployer_id_par, key_par, ack_channel] => {
                    if let (Some(deployer_id_str), Some(public_key)) = (
                        RhoString::unapply(deployer_id_par),
                        RhoByteArray::unapply(key_par),
                    ) {
                        if deployer_id_str == "deployerId" {
                            let output = vec![RhoDeployerId::create_par(public_key)];
                            produce(&output, ack_channel).await?;
                            Ok(output)
                        } else {
                            Err(illegal_argument_error("deployer_id_make"))
                        }
                    } else {
                        Err(illegal_argument_error("deployer_id_make"))
                    }
                }
                _ => Err(illegal_argument_error("deployer_id_make")),
            }
        } else {
            Err(illegal_argument_error("deployer_id_make"))
        }
    }

    // See casper/src/test/scala/coop/rchain/casper/helper/Secp256k1SignContract.scala

    pub async fn secp256k1_sign(
        &mut self,
        message: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        if let Some((produce, _, _, args)) = self.is_contract_call().unapply(message) {
            match args.as_slice() {
                [hash_par, sk_par, ack_channel] => {
                    if let (Some(hash), Some(secret_key)) = (
                        RhoByteArray::unapply(hash_par),
                        RhoByteArray::unapply(sk_par),
                    ) {
                        if secret_key.len() != 32 {
                            return Err(InterpreterError::BugFoundError(format!(
                                "Invalid private key length: must be 32 bytes, got {}",
                                secret_key.len()
                            )));
                        }

                        let signing_key =
                            SigningKey::from_slice(&secret_key).expect("Invalid private key");

                        let signature: Signature = signing_key
                            .sign_prehash(&hash)
                            .expect("Failed to sign prehash");
                        let der_bytes = signature.to_der().as_bytes().to_vec();

                        let result_par = new_gbytearray_par(der_bytes, Vec::new(), false);

                        let output = vec![result_par];
                        produce(&output, ack_channel).await?;
                        Ok(output)
                    } else {
                        Err(illegal_argument_error("secp256k1_sign"))
                    }
                }
                _ => Err(illegal_argument_error("secp256k1_sign")),
            }
        } else {
            Err(illegal_argument_error("secp256k1_sign"))
        }
    }

    // See casper/src/test/scala/coop/rchain/casper/helper/SysAuthTokenContract.scala

    pub async fn sys_auth_token_make(
        &mut self,
        message: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        if let Some((produce, _, _, args)) = self.is_contract_call().unapply(message) {
            match args.as_slice() {
                [ack_channel] => {
                    let auth_token = new_gsys_auth_token_par(Vec::new(), false);

                    let output = vec![auth_token];
                    produce(&output, ack_channel).await?;
                    Ok(output)
                }
                _ => Err(illegal_argument_error("sys_auth_token_make")),
            }
        } else {
            Err(illegal_argument_error("sys_auth_token_make"))
        }
    }

    //See casper/src/test/scala/coop/rchain/casper/helper/BlockDataContract.scala

    pub async fn block_data_set(
        &mut self,
        message: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        if let Some((produce, _, _, args)) = self.is_contract_call().unapply(message) {
            match args.as_slice() {
                [key_par, value_par, ack_channel] => {
                    if let Some(key) = RhoString::unapply(key_par) {
                        match key.as_str() {
                            "sender" => {
                                if let Some(public_key_bytes) = RhoByteArray::unapply(value_par) {
                                    let mut block_data = self.block_data.write().await;
                                    block_data.sender = PublicKey {
                                        bytes: public_key_bytes.clone().into(),
                                    };
                                    drop(block_data);

                                    let result_par = vec![Par::default()];
                                    produce(&result_par, ack_channel).await?;
                                    Ok(result_par)
                                } else {
                                    Err(illegal_argument_error("block_data_set"))
                                }
                            }
                            "blockNumber" => {
                                if let Some(block_number) = RhoNumber::unapply(value_par) {
                                    let mut block_data = self.block_data.write().await;
                                    block_data.block_number = block_number;
                                    drop(block_data);

                                    let result_par = vec![Par::default()];
                                    produce(&result_par, ack_channel).await?;
                                    Ok(result_par)
                                } else {
                                    Err(illegal_argument_error("block_data_set"))
                                }
                            }
                            _ => Err(illegal_argument_error("block_data_set")),
                        }
                    } else {
                        Err(illegal_argument_error("block_data_set"))
                    }
                }
                _ => Err(illegal_argument_error("block_data_set")),
            }
        } else {
            Err(illegal_argument_error("block_data_set"))
        }
    }

    // See casper/src/test/scala/coop/rchain/casper/helper/CasperInvalidBlocksContract.scala

    pub async fn casper_invalid_blocks_set(
        &self,
        message: (Vec<ListParWithRandom>, bool, Vec<Par>),
        invalid_blocks: &InvalidBlocks,
    ) -> Result<Vec<Par>, InterpreterError> {
        if let Some((produce, _, _, args)) = self.is_contract_call().unapply(message) {
            match args.as_slice() {
                [new_invalid_blocks_par, ack_channel] => {
                    let mut invalid_blocks_lock = invalid_blocks.invalid_blocks.write().await;
                    *invalid_blocks_lock = new_invalid_blocks_par.clone();

                    let result_par = vec![Par::default()];
                    produce(&result_par, ack_channel).await?;
                    Ok(result_par)
                }
                _ => Err(illegal_argument_error("casper_invalid_blocks_set")),
            }
        } else {
            Err(illegal_argument_error("casper_invalid_blocks_set"))
        }
    }

    // ChromaDB section start
    #[cfg(feature = "chromadb")]
    pub async fn chroma_create_collection(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("chroma_create_collection"));
        };

        let [collection_name_par, ignore_or_update_if_exists_par, metadata_par, ack] =
            args.as_slice()
        else {
            return Err(illegal_argument_error("chroma_create_collection"));
        };

        let (Some(collection_name), Some(ignore_or_update_if_exists), Some(metadata)) = (
            RhoString::unapply(collection_name_par),
            RhoBoolean::unapply(ignore_or_update_if_exists_par),
            // It can either be nil, or a metadata map.
            if metadata_par.is_nil() {
                Some(None)
            } else {
                <Metadata as Extractor>::unapply(metadata_par).map(Some)
            },
        ) else {
            return Err(illegal_argument_error("chroma_create_collection"));
        };

        self.chromadb_service
            .create_collection(&collection_name, ignore_or_update_if_exists, metadata)
            .await?;

        let output = vec![Par::default()];
        produce(&output, ack).await?;
        Ok(output)
    }

    #[cfg(feature = "chromadb")]
    pub async fn chroma_get_collection_meta(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous_output, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("chroma_get_collection_meta"));
        };

        let [collection_name_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("chroma_get_collection_meta"));
        };
        let Some(collection_name) = RhoString::unapply(collection_name_par) else {
            return Err(illegal_argument_error("chroma_get_collection_meta"));
        };

        // Common piece of code.
        if is_replay {
            produce(&previous_output, ack).await?;
            return Ok(previous_output);
        }

        let meta = self
            .chromadb_service
            .get_collection_meta(&collection_name)
            .await?;
        let result_par = match meta {
            None => RhoNil::create_par(),
            Some(inner) => inner.into(),
        };

        let output = vec![result_par];
        produce(&output, &ack).await?;
        Ok(output)
    }

    #[cfg(feature = "chromadb")]
    pub async fn chroma_upsert_entries(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("chroma_upsert_entries"));
        };

        let [collection_name_par, entries_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("chroma_upsert_entries"));
        };
        let (Some(collection_name), Some(entries)) = (
            RhoString::unapply(collection_name_par),
            <CollectionEntries as Extractor>::unapply(entries_par),
        ) else {
            return Err(illegal_argument_error("chroma_upsert_entries"));
        };

        self.chromadb_service
            .upsert_entries(&collection_name, entries)
            .await?;

        let result_par = RhoString::create_par(collection_name);
        let output = vec![result_par];
        produce(&output, ack).await?;
        Ok(output)
    }

    #[cfg(feature = "chromadb")]
    pub async fn chroma_query(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous_output, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("chroma_query"));
        };

        let [collection_name_par, doc_texts_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("chroma_query"));
        };
        let (Some(collection_name), Some(doc_texts)) = (
            RhoString::unapply(collection_name_par),
            <Vec<RhoString> as Extractor>::unapply(doc_texts_par),
        ) else {
            return Err(illegal_argument_error("chroma_query"));
        };

        // Common piece of code.
        if is_replay {
            produce(&previous_output, ack).await?;
            return Ok(previous_output);
        }

        let res = self
            .chromadb_service
            .query(
                &collection_name,
                doc_texts.iter().map(|s| s.as_ref()).collect(),
            )
            .await?;

        let result_par_vec: Vec<Par> = res.into_iter().map(Into::into).collect();
        let result_par = RhoList::create_par(result_par_vec);

        let output = vec![result_par];
        produce(&output, &ack).await?;
        Ok(output)
    }

    #[cfg(feature = "chromadb")]
    pub async fn chroma_delete_documents(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("chroma_delete_documents"));
        };

        let [collection_name_par, doc_ids_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("chroma_delete_documents"));
        };
        let (Some(collection_name), Some(doc_ids)) = (
            RhoString::unapply(collection_name_par),
            <Vec<RhoString> as Extractor>::unapply(doc_ids_par),
        ) else {
            return Err(illegal_argument_error("chroma_delete_documents"));
        };

        self.chromadb_service
            .delete_documents(&collection_name, doc_ids)
            .await?;

        let result_par = RhoString::create_par(collection_name);
        let output = vec![result_par];
        produce(&output, ack).await?;
        Ok(output)
    }

    // ChromaDB section end

    // ----- File I/O native primitives (FIP 2026-02-06) -------------------

    /// `nativeOpen(path: String, mode: String) -> [true, fd] | [false, code, msg]`.
    ///
    /// Opens a regular file with fopen-style semantics per
    /// `crate::rust::interpreter::io::mode::open_options_for`. On
    /// success, stashes the `tokio::fs::File` in the runtime's
    /// `FileHandleTable` under a freshly-issued `i64` fd and returns
    /// `[true, fd]`. On failure, returns `[false, FSERR_*, msg]`.
    ///
    /// This is an *internal* URN (`rho:io:fs:native:1.0.0/open`).
    /// User-facing code goes through the `Fs` agent, which pre-
    /// canonicalizes the path, translates the symbolic mode string
    /// per `chmod` conventions, and wraps the tuple result in the
    /// agent's try/catch shape.
    pub async fn native_open(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        use crate::rust::interpreter::io::{mode, response};

        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("native_open"));
        };
        let [path_par, mode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("native_open"));
        };
        let (Some(path_str), Some(mode_str)) =
            (RhoString::unapply(path_par), RhoString::unapply(mode_par))
        else {
            return Err(illegal_argument_error("native_open"));
        };

        let response_par = match mode::open_options_for(&mode_str) {
            None => response::err(
                response::FSERR_BAD_ARG,
                format!("unknown mode {mode_str:?}"),
            ),
            Some(opts) => match opts.open(&path_str).await {
                Err(e) => response::from_io_error(e),
                Ok(file) => {
                    // If canonicalization fails (e.g. mode "wx" created
                    // a file whose parent isn't canonicalizable for some
                    // exotic reason), fall back to the caller's path
                    // string. The powerbox already pre-canonicalizes
                    // paths on the way in, so this is a defense-in-depth
                    // path for direct callers.
                    let canonical = tokio::fs::canonicalize(&path_str)
                        .await
                        .unwrap_or_else(|_| std::path::PathBuf::from(&path_str));
                    let fd = self.file_handles.insert(file, canonical, mode_str).await;
                    response::ok(vec![
                        crate::rust::interpreter::rho_type::RhoNumber::create_par(fd),
                    ])
                }
            },
        };

        let output = vec![response_par];
        produce(&output, ack).await?;
        Ok(output)
    }

    /// `nativeClose(fd: Int) -> [true] | [false, code, msg]`.
    ///
    /// Removes the fd from the `FileHandleTable`. Any outstanding
    /// `Arc<FileHandle>` clones held by concurrent handlers keep the
    /// underlying `tokio::fs::File` alive until they drop.
    /// Idempotent from the caller's perspective: a second close on
    /// the same fd returns `[false, FSERR_CLOSED, ...]` rather than
    /// erroring the deploy.
    pub async fn native_close(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        use crate::rust::interpreter::io::response;

        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("native_close"));
        };
        let [fd_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("native_close"));
        };
        let Some(fd) = RhoNumber::unapply(fd_par) else {
            return Err(illegal_argument_error("native_close"));
        };

        let response_par = match self.file_handles.remove(fd).await {
            Some(_) => response::ok(vec![]),
            None => response::err(response::FSERR_CLOSED, format!("fd {fd} is not open")),
        };

        let output = vec![response_par];
        produce(&output, ack).await?;
        Ok(output)
    }

    /// `nativeRead(fd: Int, n: Int) -> [true, bytes] | [false, code, msg]`.
    ///
    /// Reads up to `n` bytes from the file's current position and
    /// advances the position by the number of bytes actually read.
    /// EOF at the current position returns `[true, ""]` (empty
    /// ByteArray); a short read followed by EOF returns whatever was
    /// read.
    ///
    /// The read loops on the underlying `tokio::fs::File` until the
    /// buffer is full or an EOF is observed. This gives users the
    /// "read n bytes" semantics they generally expect, rather than
    /// the POSIX "one syscall may return fewer bytes" surface.
    pub async fn native_read(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        use tokio::io::AsyncReadExt;

        use crate::rust::interpreter::io::response;

        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("native_read"));
        };
        let [fd_par, n_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("native_read"));
        };
        let (Some(fd), Some(n)) = (RhoNumber::unapply(fd_par), RhoNumber::unapply(n_par)) else {
            return Err(illegal_argument_error("native_read"));
        };

        // Per-call ceiling on the buffer we allocate. Set below the
        // point where a hostile deploy could exhaust node memory via
        // `nativeRead(fd, i64::MAX)`. Callers wanting larger reads
        // issue multiple calls or go through the agent-layer
        // `lines`/`text` methods (which iterate internally).
        const MAX_READ_BYTES: i64 = 64 * 1024 * 1024;

        let response_par = if n < 0 {
            response::err(response::FSERR_BAD_ARG, format!("negative read length {n}"))
        } else if n > MAX_READ_BYTES {
            response::err(
                response::FSERR_BAD_ARG,
                format!("read length {n} exceeds MAX_READ_BYTES {MAX_READ_BYTES}"),
            )
        } else {
            match self.file_handles.get(fd).await {
                None => response::err(response::FSERR_CLOSED, format!("fd {fd} is not open")),
                Some(handle) => {
                    let mut buf = vec![0u8; n as usize];
                    let mut file = handle.file.lock().await;
                    let mut total = 0usize;
                    let mut io_err: Option<std::io::Error> = None;
                    while total < buf.len() {
                        match file.read(&mut buf[total..]).await {
                            Ok(0) => break,
                            Ok(k) => total += k,
                            Err(e) => {
                                io_err = Some(e);
                                break;
                            }
                        }
                    }
                    drop(file);
                    match io_err {
                        Some(e) => response::from_io_error(e),
                        None => {
                            buf.truncate(total);
                            response::ok(vec![RhoByteArray::create_par(buf)])
                        }
                    }
                }
            }
        };

        let output = vec![response_par];
        produce(&output, ack).await?;
        Ok(output)
    }

    /// `nativeWrite(fd: Int, bytes: ByteArray) -> [true, nWritten] | [false, code, msg]`.
    ///
    /// Writes all of `bytes` at the file's current position and
    /// advances the position by the number of bytes written. Loops
    /// through short writes so the returned `nWritten` is the full
    /// buffer length on success. Errors surface via `FSERR_*`.
    pub async fn native_write(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        use tokio::io::AsyncWriteExt;

        use crate::rust::interpreter::io::response;

        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("native_write"));
        };
        let [fd_par, bytes_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("native_write"));
        };
        let (Some(fd), Some(bytes)) =
            (RhoNumber::unapply(fd_par), RhoByteArray::unapply(bytes_par))
        else {
            return Err(illegal_argument_error("native_write"));
        };

        let response_par = match self.file_handles.get(fd).await {
            None => response::err(response::FSERR_CLOSED, format!("fd {fd} is not open")),
            Some(handle) => {
                let mut file = handle.file.lock().await;
                match file.write_all(&bytes).await {
                    Ok(()) => response::ok(vec![RhoNumber::create_par(bytes.len() as i64)]),
                    Err(e) => response::from_io_error(e),
                }
            }
        };

        let output = vec![response_par];
        produce(&output, ack).await?;
        Ok(output)
    }

    /// `nativeSeek(fd: Int, offset: Int, whence: String) -> [true, newPos] | [false, code, msg]`.
    ///
    /// `whence` is `"set"`, `"cur"`, or `"end"` (FIP §"Positional").
    /// `"set"` requires a non-negative offset; `"cur"` and `"end"`
    /// accept any signed offset. Returns the new absolute position.
    pub async fn native_seek(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        use std::io::SeekFrom;

        use tokio::io::AsyncSeekExt;

        use crate::rust::interpreter::io::response;

        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("native_seek"));
        };
        let [fd_par, offset_par, whence_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("native_seek"));
        };
        let (Some(fd), Some(offset), Some(whence)) = (
            RhoNumber::unapply(fd_par),
            RhoNumber::unapply(offset_par),
            RhoString::unapply(whence_par),
        ) else {
            return Err(illegal_argument_error("native_seek"));
        };

        let seek_from = match whence.as_str() {
            "set" => {
                if offset < 0 {
                    Err(response::err(
                        response::FSERR_BAD_ARG,
                        format!("seek 'set' requires non-negative offset, got {offset}"),
                    ))
                } else {
                    Ok(SeekFrom::Start(offset as u64))
                }
            }
            "cur" => Ok(SeekFrom::Current(offset)),
            "end" => Ok(SeekFrom::End(offset)),
            other => Err(response::err(
                response::FSERR_BAD_ARG,
                format!("unknown seek whence {other:?}"),
            )),
        };

        let response_par = match seek_from {
            Err(err_par) => err_par,
            Ok(sf) => match self.file_handles.get(fd).await {
                None => response::err(response::FSERR_CLOSED, format!("fd {fd} is not open")),
                Some(handle) => {
                    let mut file = handle.file.lock().await;
                    match file.seek(sf).await {
                        Ok(pos) => response::ok(vec![RhoNumber::create_par(pos as i64)]),
                        Err(e) => response::from_io_error(e),
                    }
                }
            },
        };

        let output = vec![response_par];
        produce(&output, ack).await?;
        Ok(output)
    }

    /// `nativeTell(fd: Int) -> [true, pos] | [false, code, msg]`.
    ///
    /// Reports the current position without moving it, implemented
    /// as a seek of `Current(0)` since `tokio::fs::File` does not
    /// expose a dedicated `stream_position` on stable.
    pub async fn native_tell(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        use std::io::SeekFrom;

        use tokio::io::AsyncSeekExt;

        use crate::rust::interpreter::io::response;

        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("native_tell"));
        };
        let [fd_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("native_tell"));
        };
        let Some(fd) = RhoNumber::unapply(fd_par) else {
            return Err(illegal_argument_error("native_tell"));
        };

        let response_par = match self.file_handles.get(fd).await {
            None => response::err(response::FSERR_CLOSED, format!("fd {fd} is not open")),
            Some(handle) => {
                let mut file = handle.file.lock().await;
                match file.seek(SeekFrom::Current(0)).await {
                    Ok(pos) => response::ok(vec![RhoNumber::create_par(pos as i64)]),
                    Err(e) => response::from_io_error(e),
                }
            }
        };

        let output = vec![response_par];
        produce(&output, ack).await?;
        Ok(output)
    }

    /// `nativeSize(fd: Int) -> [true, nBytes] | [false, code, msg]`.
    ///
    /// Returns the file's current size via `metadata()`. Independent
    /// of the file position, so a caller can query size without
    /// disturbing an in-progress positional read/write.
    pub async fn native_size(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        use crate::rust::interpreter::io::response;

        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("native_size"));
        };
        let [fd_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("native_size"));
        };
        let Some(fd) = RhoNumber::unapply(fd_par) else {
            return Err(illegal_argument_error("native_size"));
        };

        let response_par = match self.file_handles.get(fd).await {
            None => response::err(response::FSERR_CLOSED, format!("fd {fd} is not open")),
            Some(handle) => {
                let file = handle.file.lock().await;
                match file.metadata().await {
                    Ok(meta) => response::ok(vec![RhoNumber::create_par(meta.len() as i64)]),
                    Err(e) => response::from_io_error(e),
                }
            }
        };

        let output = vec![response_par];
        produce(&output, ack).await?;
        Ok(output)
    }

    /// `nativeTruncate(fd: Int, n: Int) -> [true] | [false, code, msg]`.
    ///
    /// Sets the file size to `n` bytes. Growing zero-pads per POSIX
    /// `ftruncate`. Requires the fd to have been opened in a
    /// writeable mode; otherwise the host returns
    /// `PermissionDenied`, which the response layer maps to
    /// `FSERR_PERM`.
    pub async fn native_truncate(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        use crate::rust::interpreter::io::response;

        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("native_truncate"));
        };
        let [fd_par, n_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("native_truncate"));
        };
        let (Some(fd), Some(n)) = (RhoNumber::unapply(fd_par), RhoNumber::unapply(n_par)) else {
            return Err(illegal_argument_error("native_truncate"));
        };

        // Per-call ceiling on the size a single truncate can request.
        // On sparse-file filesystems (ext4, apfs) `set_len(n)` is
        // O(1) at any n, so a hostile deploy calling
        // `nativeTruncate(fd, i64::MAX)` would report success. On
        // non-sparse filesystems the same call fills the disk. Cap
        // at 16 GiB per call as a defensive ceiling; per-deploy
        // quotas belong in the powerbox layer.
        const MAX_TRUNCATE_BYTES: i64 = 16 * 1024 * 1024 * 1024;

        let response_par = if n < 0 {
            response::err(
                response::FSERR_BAD_ARG,
                format!("negative truncate length {n}"),
            )
        } else if n > MAX_TRUNCATE_BYTES {
            response::err(
                response::FSERR_BAD_ARG,
                format!("truncate length {n} exceeds MAX_TRUNCATE_BYTES {MAX_TRUNCATE_BYTES}"),
            )
        } else {
            match self.file_handles.get(fd).await {
                None => response::err(response::FSERR_CLOSED, format!("fd {fd} is not open")),
                Some(handle) => {
                    let file = handle.file.lock().await;
                    match file.set_len(n as u64).await {
                        Ok(()) => response::ok(vec![]),
                        Err(e) => response::from_io_error(e),
                    }
                }
            }
        };

        let output = vec![response_par];
        produce(&output, ack).await?;
        Ok(output)
    }

    /// `nativeFlush(fd: Int) -> [true] | [false, code, msg]`.
    ///
    /// Forces a durable write via `sync_all`, which fsyncs both data
    /// and metadata. The FIP promises "durable write" not "flushed to
    /// kernel", so `sync_all` is the right primitive rather than
    /// `AsyncWriteExt::flush`.
    pub async fn native_flush(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        use crate::rust::interpreter::io::response;

        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("native_flush"));
        };
        let [fd_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("native_flush"));
        };
        let Some(fd) = RhoNumber::unapply(fd_par) else {
            return Err(illegal_argument_error("native_flush"));
        };

        let response_par = match self.file_handles.get(fd).await {
            None => response::err(response::FSERR_CLOSED, format!("fd {fd} is not open")),
            Some(handle) => {
                let file = handle.file.lock().await;
                match file.sync_all().await {
                    Ok(()) => response::ok(vec![]),
                    Err(e) => response::from_io_error(e),
                }
            }
        };

        let output = vec![response_par];
        produce(&output, ack).await?;
        Ok(output)
    }

    /// `nativeStat(path: String) -> [true, statRecord] | [false, code, msg]`.
    ///
    /// Reports metadata for the given path per the FIP TODO 5 record
    /// shape. The record includes `name`, `kind`, `size` (files only),
    /// `mode`, and `mtime`/`ctime`/`atime`. `owner`/`group` land in
    /// the `nativeChown` slice where NSS lookup is introduced.
    ///
    /// Uses `symlink_metadata` (i.e. does not follow symlinks) so a
    /// dangling symlink surfaces as `kind: "symlink"` rather than
    /// `FSERR_NOT_FOUND`. Callers who want follow-symlink semantics
    /// call `openFile` and then observe via the fd.
    pub async fn native_stat(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        use crate::rust::interpreter::io::{response, stat};
        use crate::rust::interpreter::rho_type::RhoMap;

        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("native_stat"));
        };
        let [path_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("native_stat"));
        };
        let Some(path_str) = RhoString::unapply(path_par) else {
            return Err(illegal_argument_error("native_stat"));
        };

        let path = std::path::Path::new(&path_str);
        let response_par = match tokio::fs::symlink_metadata(path).await {
            Err(e) => response::from_io_error(e),
            Ok(meta) => {
                let basename = stat::basename_of(path);
                let record = stat::stat_record(&basename, &meta);
                response::ok(vec![RhoMap::create_par(record)])
            }
        };

        let output = vec![response_par];
        produce(&output, ack).await?;
        Ok(output)
    }

    /// `nativeEntries(dirPath: String) -> [true, [entry, ...]] | [false, code, msg]`.
    ///
    /// Reads the given directory and returns one record per entry,
    /// sorted lexicographically by `name` (UTF-8 byte order) so the
    /// result is deterministic across nodes per the FIP TODO 5
    /// promise. Each record has the same shape as `nativeStat`; the
    /// `name` field is the basename only, never containing `/`.
    pub async fn native_entries(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        use crate::rust::interpreter::io::{response, stat};
        use crate::rust::interpreter::rho_type::{RhoList, RhoMap};

        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("native_entries"));
        };
        let [path_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("native_entries"));
        };
        let Some(path_str) = RhoString::unapply(path_par) else {
            return Err(illegal_argument_error("native_entries"));
        };

        let response_par = match tokio::fs::read_dir(&path_str).await {
            Err(e) => response::from_io_error(e),
            Ok(mut rd) => {
                let mut collected: Vec<(String, std::fs::Metadata)> = Vec::new();
                let mut err: Option<std::io::Error> = None;
                loop {
                    match rd.next_entry().await {
                        Err(e) => {
                            err = Some(e);
                            break;
                        }
                        Ok(None) => break,
                        Ok(Some(entry)) => {
                            let name = match entry.file_name().into_string() {
                                Ok(s) => s,
                                // Non-UTF-8 filename: skip. The FIP
                                // treats paths as `String`, so we
                                // have no way to represent it and
                                // dropping is preferable to erroring
                                // the whole listing.
                                Err(_) => continue,
                            };
                            match entry.metadata().await {
                                Err(e) => {
                                    err = Some(e);
                                    break;
                                }
                                Ok(meta) => collected.push((name, meta)),
                            }
                        }
                    }
                }
                match err {
                    Some(e) => response::from_io_error(e),
                    None => {
                        collected.sort_by(|a, b| a.0.cmp(&b.0));
                        let entries: Vec<Par> = collected
                            .into_iter()
                            .map(|(name, meta)| RhoMap::create_par(stat::stat_record(&name, &meta)))
                            .collect();
                        response::ok(vec![RhoList::create_par(entries)])
                    }
                }
            }
        };

        let output = vec![response_par];
        produce(&output, ack).await?;
        Ok(output)
    }

    /// `nativeExists(path: String) -> [true, bool] | [false, code, msg]`.
    ///
    /// Reports whether the path exists as a filesystem entry, WITHOUT
    /// following symlinks. A symlink present at `path` reports
    /// `true` regardless of whether its target exists -- consistent
    /// with `nativeStat`, which uses `symlink_metadata` and reports
    /// such a path as `kind: "symlink"`.
    ///
    /// The alternative (`tokio::fs::try_exists`, which follows
    /// symlinks) would let two adjacent primitives disagree on the
    /// same path: a caller could use `exists` for the "is there
    /// anything at this path?" check and then `stat` for details,
    /// and get contradictory answers when the path is a dangling
    /// symlink or a symlink to an in-quarantine target. Uniform
    /// non-follow semantics avoids that footgun.
    ///
    /// Callers who want follow-symlink semantics open the file (via
    /// `nativeOpen`, which uses `OpenOptions::open` and does follow)
    /// and observe via the resulting fd.
    pub async fn native_exists(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        use crate::rust::interpreter::io::response;

        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("native_exists"));
        };
        let [path_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("native_exists"));
        };
        let Some(path_str) = RhoString::unapply(path_par) else {
            return Err(illegal_argument_error("native_exists"));
        };

        let response_par = match tokio::fs::symlink_metadata(&path_str).await {
            Ok(_) => response::ok(vec![RhoBoolean::create_par(true)]),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                response::ok(vec![RhoBoolean::create_par(false)])
            }
            Err(e) => response::from_io_error(e),
        };

        let output = vec![response_par];
        produce(&output, ack).await?;
        Ok(output)
    }

    /// `nativeRename(from: String, to: String) -> [true] | [false, code, msg]`.
    ///
    /// Atomic rename per POSIX `rename(2)`. A cross-filesystem rename
    /// returns `FSERR_CROSS_DEVICE` per the FIP §"rename" note so
    /// callers who want copy semantics do it explicitly with
    /// `nativeCopyFile` + `nativeRemoveFile`.
    ///
    /// EXDEV is 18 on both Linux and macOS (and the BSDs, and all
    /// other POSIX-y hosts the FIP targets); we check `raw_os_error`
    /// against that number directly rather than pulling in `libc`
    /// for a single constant. `io::ErrorKind::CrossesDevices` would
    /// let us drop this, but it's nightly-only.
    pub async fn native_rename(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        use crate::rust::interpreter::io::response;

        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("native_rename"));
        };
        let [from_par, to_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("native_rename"));
        };
        let (Some(from_str), Some(to_str)) =
            (RhoString::unapply(from_par), RhoString::unapply(to_par))
        else {
            return Err(illegal_argument_error("native_rename"));
        };

        let response_par = match tokio::fs::rename(&from_str, &to_str).await {
            Ok(()) => response::ok(vec![]),
            Err(e) => {
                if e.raw_os_error() == Some(18) {
                    response::err(
                        response::FSERR_CROSS_DEVICE,
                        format!("rename {from_str:?} -> {to_str:?}: {e}"),
                    )
                } else {
                    response::from_io_error(e)
                }
            }
        };

        let output = vec![response_par];
        produce(&output, ack).await?;
        Ok(output)
    }

    /// `nativeCopyFile(from: String, to: String) -> [true, nBytes] | [false, code, msg]`.
    ///
    /// Copies file contents from `from` to `to`, replacing `to` if
    /// it already exists. Returns the number of bytes copied.
    /// Distinct from `nativeRename`: `copyFile` produces an
    /// independent inode and is fine across filesystems.
    pub async fn native_copy_file(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        use crate::rust::interpreter::io::response;

        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("native_copy_file"));
        };
        let [from_par, to_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("native_copy_file"));
        };
        let (Some(from_str), Some(to_str)) =
            (RhoString::unapply(from_par), RhoString::unapply(to_par))
        else {
            return Err(illegal_argument_error("native_copy_file"));
        };

        let response_par = match tokio::fs::copy(&from_str, &to_str).await {
            Ok(n) => response::ok(vec![RhoNumber::create_par(n as i64)]),
            Err(e) => response::from_io_error(e),
        };

        let output = vec![response_par];
        produce(&output, ack).await?;
        Ok(output)
    }

    /// `nativeRemoveFile(path: String) -> [true] | [false, code, msg]`.
    ///
    /// Unlinks a regular file. Fails with `FSERR_PERM` (via the
    /// host's `PermissionDenied`) when called on a directory --
    /// callers use `nativeRemoveDir` for those.
    pub async fn native_remove_file(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        use crate::rust::interpreter::io::response;

        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("native_remove_file"));
        };
        let [path_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("native_remove_file"));
        };
        let Some(path_str) = RhoString::unapply(path_par) else {
            return Err(illegal_argument_error("native_remove_file"));
        };

        let response_par = match tokio::fs::remove_file(&path_str).await {
            Ok(()) => response::ok(vec![]),
            Err(e) => response::from_io_error(e),
        };

        let output = vec![response_par];
        produce(&output, ack).await?;
        Ok(output)
    }

    /// `nativeRemoveDir(path: String, recursive: Bool) -> [true] | [false, code, msg]`.
    ///
    /// Removes a directory. When `recursive` is true, uses
    /// `remove_dir_all` (best-effort atomic per tokio's docs) so
    /// non-empty trees can be dropped in one call. When false,
    /// uses `remove_dir`, which requires the directory to be empty
    /// and returns `FSERR_IO` otherwise.
    pub async fn native_remove_dir(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        use crate::rust::interpreter::io::response;

        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("native_remove_dir"));
        };
        let [path_par, recursive_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("native_remove_dir"));
        };
        let (Some(path_str), Some(recursive)) = (
            RhoString::unapply(path_par),
            RhoBoolean::unapply(recursive_par),
        ) else {
            return Err(illegal_argument_error("native_remove_dir"));
        };

        let result = if recursive {
            tokio::fs::remove_dir_all(&path_str).await
        } else {
            tokio::fs::remove_dir(&path_str).await
        };
        let response_par = match result {
            Ok(()) => response::ok(vec![]),
            Err(e) => response::from_io_error(e),
        };

        let output = vec![response_par];
        produce(&output, ack).await?;
        Ok(output)
    }

    /// `nativeChmod(path: String, modeBits: Int) -> [true] | [false, code, msg]`.
    ///
    /// Sets the file's permission bits. `modeBits` is the low 9 bits
    /// (0..=0o777); the higher setuid/setgid/sticky bits are not
    /// exposed per FIP TODO 5. Any value outside that range returns
    /// `FSERR_BAD_ARG`.
    ///
    /// The Rholang agent layer parses the symbolic `"rwxr-xr-x"`
    /// string (FIP TODO 8) and passes the resulting integer down;
    /// keeping string parsing in Rholang lets the native handler
    /// stay a thin syscall bridge.
    pub async fn native_chmod(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        use crate::rust::interpreter::io::response;

        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("native_chmod"));
        };
        let [path_par, mode_bits_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("native_chmod"));
        };
        let (Some(path_str), Some(mode_bits)) = (
            RhoString::unapply(path_par),
            RhoNumber::unapply(mode_bits_par),
        ) else {
            return Err(illegal_argument_error("native_chmod"));
        };

        let response_par = if !(0..=0o777).contains(&mode_bits) {
            response::err(
                response::FSERR_BAD_ARG,
                format!("chmod bits must be in 0..=0o777, got {mode_bits:o}"),
            )
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(mode_bits as u32);
                match tokio::fs::set_permissions(&path_str, perms).await {
                    Ok(()) => response::ok(vec![]),
                    Err(e) => response::from_io_error(e),
                }
            }
            #[cfg(not(unix))]
            {
                // FIP targets macOS + Linux. On non-Unix hosts the
                // most we can do is toggle the readonly bit, which
                // is a poor approximation and would surprise users;
                // refuse instead.
                let _ = &path_str;
                response::err(
                    response::FSERR_UNSUPPORTED,
                    "chmod is not supported on this platform".to_string(),
                )
            }
        };

        let output = vec![response_par];
        produce(&output, ack).await?;
        Ok(output)
    }

    /// `nativeQuarantine(rootPath: String, relPath: String) -> [true, canonPath] | [false, code, msg]`.
    ///
    /// Resolves `relPath` against `rootPath` and confirms the
    /// canonical result is underneath `rootPath`. Used by the
    /// Rholang `Dir` agent before dispatching a path-taking native
    /// so the quarantine enforcement lives on the Rust side where
    /// the syscalls are.
    ///
    /// `rootPath` must already be a canonical absolute path (the
    /// powerbox canonicalizes it at boot). See
    /// `io::path::canonicalize_and_quarantine` for the escape /
    /// symlink / missing-tail semantics.
    pub async fn native_quarantine(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        use crate::rust::interpreter::io::{path as pathq, response};

        let Some((produce, _, _, args)) = self.is_contract_call().unapply(contract_args) else {
            return Err(illegal_argument_error("native_quarantine"));
        };
        let [root_par, rel_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("native_quarantine"));
        };
        let (Some(root_str), Some(rel_str)) =
            (RhoString::unapply(root_par), RhoString::unapply(rel_par))
        else {
            return Err(illegal_argument_error("native_quarantine"));
        };

        let root_path = std::path::Path::new(&root_str);
        let response_par = match pathq::canonicalize_and_quarantine(root_path, &rel_str) {
            Ok(canonical) => {
                let s = canonical.to_string_lossy().into_owned();
                response::ok(vec![RhoString::create_par(s)])
            }
            Err(e) => response::err(e.code(), e.message()),
        };

        let output = vec![response_par];
        produce(&output, ack).await?;
        Ok(output)
    }
}

// See casper/src/test/scala/coop/rchain/casper/helper/RhoSpec.scala

pub fn test_framework_contracts() -> Vec<Definition> {
    vec![
        Definition {
            urn: "rho:test:assertAck".to_string(),
            fixed_channel: byte_name(101),
            arity: 5,
            body_ref: 101,
            handler: {
                Box::new(|ctx| {
                    let sp = ctx.system_processes.clone();
                    Box::new(move |args| {
                        let sp = sp.clone();
                        Box::pin(async move { sp.handle_message(args).await })
                    })
                })
            },
            remainder: None,
        },
        Definition {
            urn: "rho:test:testSuiteCompleted".to_string(),
            fixed_channel: byte_name(102),
            arity: 1,
            body_ref: 102,
            handler: Box::new(|ctx| {
                let sp = ctx.system_processes.clone();
                Box::new(move |args| {
                    let sp = sp.clone();
                    Box::pin(async move { sp.handle_message(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:io:stdlog".to_string(),
            fixed_channel: byte_name(103),
            arity: 2,
            body_ref: 103,
            handler: Box::new(|ctx| {
                let sp = ctx.system_processes.clone();
                Box::new(move |args| {
                    let mut sp = sp.clone();
                    Box::pin(async move { sp.std_log(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:test:deployerId:make".to_string(),
            fixed_channel: byte_name(104),
            arity: 3,
            body_ref: 104,
            handler: Box::new(|ctx| {
                let sp = ctx.system_processes.clone();
                Box::new(move |args| {
                    let mut sp = sp.clone();
                    Box::pin(async move { sp.deployer_id_make(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:test:crypto:secp256k1Sign".to_string(),
            fixed_channel: byte_name(105),
            arity: 3,
            body_ref: 105,
            handler: Box::new(|ctx| {
                let sp = ctx.system_processes.clone();
                Box::new(move |args| {
                    let mut sp = sp.clone();
                    Box::pin(async move { sp.secp256k1_sign(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "sys:test:authToken:make".to_string(),
            fixed_channel: byte_name(106),
            arity: 1,
            body_ref: 106,
            handler: Box::new(|ctx| {
                let sp = ctx.system_processes.clone();
                Box::new(move |args| {
                    let mut sp = sp.clone();
                    Box::pin(async move { sp.sys_auth_token_make(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:test:block:data:set".to_string(),
            fixed_channel: byte_name(107),
            arity: 3,
            body_ref: 107,
            handler: Box::new(|ctx| {
                let sp = ctx.system_processes.clone();
                Box::new(move |args| {
                    let mut sp = sp.clone();
                    Box::pin(async move { sp.block_data_set(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:test:casper:invalidBlocks:set".to_string(),
            fixed_channel: byte_name(108),
            arity: 2,
            body_ref: 108,
            handler: Box::new(|ctx| {
                let sp = ctx.system_processes.clone();
                let invalid_blocks = ctx.invalid_blocks.clone();
                Box::new(move |args| {
                    let sp = sp.clone();
                    let invalid_blocks = invalid_blocks.clone();
                    Box::pin(
                        async move { sp.casper_invalid_blocks_set(args, &invalid_blocks).await },
                    )
                })
            }),
            remainder: None,
        },
    ]
}

// See casper/src/test/scala/coop/rchain/casper/helper/TestResultCollector.scala

struct IsAssert;

impl IsAssert {
    pub fn unapply(p: Vec<Par>) -> Option<(String, i64, Par, String, Par)> {
        match p.as_slice() {
            [test_name_par, attempt_par, assertion_par, clue_par, ack_channel_par] => {
                if let (Some(test_name), Some(attempt), Some(clue)) = (
                    RhoString::unapply(test_name_par),
                    RhoNumber::unapply(attempt_par),
                    RhoString::unapply(clue_par),
                ) {
                    Some((
                        test_name,
                        attempt,
                        assertion_par.clone(),
                        clue,
                        ack_channel_par.clone(),
                    ))
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

struct IsComparison;

impl IsComparison {
    pub fn unapply(p: Par) -> Option<(Par, String, Par)> {
        if let Some(expr) = single_expr(&p) {
            match expr.expr_instance.unwrap() {
                ExprInstance::ETupleBody(etuple) => match etuple.ps.as_slice() {
                    [expected_par, operator_par, actual_par] => RhoString::unapply(operator_par)
                        .map(|operator| (expected_par.clone(), operator, actual_par.clone())),
                    _ => None,
                },

                _ => None,
            }
        } else {
            None
        }
    }
}

struct IsSetFinished;

impl IsSetFinished {
    pub fn unapply(p: Vec<Par>) -> Option<bool> {
        match p.as_slice() {
            [has_finished_par] => {
                RhoBoolean::unapply(has_finished_par).map(|has_finished| has_finished)
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum RhoTestAssertion {
    RhoAssertTrue {
        test_name: String,
        is_success: bool,
        clue: String,
    },

    RhoAssertEquals {
        test_name: String,
        expected: Par,
        actual: Par,
        clue: String,
    },

    RhoAssertNotEquals {
        test_name: String,
        unexpected: Par,
        actual: Par,
        clue: String,
    },
}

impl RhoTestAssertion {
    pub fn is_success(&self) -> bool {
        match self {
            RhoTestAssertion::RhoAssertTrue { is_success, .. } => *is_success,
            RhoTestAssertion::RhoAssertEquals {
                expected, actual, ..
            } => actual == expected,
            RhoTestAssertion::RhoAssertNotEquals {
                unexpected, actual, ..
            } => actual != unexpected,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_deterministic_ops_covers_every_fileio_body_ref() {
        // Every fileio native primitive must be in the non-det set
        // so the `FailedNonDeterministicCall` path (dispatch.rs)
        // captures the lead node's result and replays it on
        // followers. Missing entries would let a consensus deploy
        // that touches the filesystem diverge between nodes.
        //
        // This test asserts the complete list; if a new NATIVE_*
        // BodyRef is added, it must land here too or this test
        // breaks (intentionally -- forcing a decision).
        let expected: &[i64] = &[
            BodyRefs::NATIVE_OPEN,
            BodyRefs::NATIVE_CLOSE,
            BodyRefs::NATIVE_READ,
            BodyRefs::NATIVE_WRITE,
            BodyRefs::NATIVE_SEEK,
            BodyRefs::NATIVE_TELL,
            BodyRefs::NATIVE_SIZE,
            BodyRefs::NATIVE_TRUNCATE,
            BodyRefs::NATIVE_FLUSH,
            BodyRefs::NATIVE_STAT,
            BodyRefs::NATIVE_ENTRIES,
            BodyRefs::NATIVE_EXISTS,
            BodyRefs::NATIVE_RENAME,
            BodyRefs::NATIVE_COPY_FILE,
            BodyRefs::NATIVE_REMOVE_FILE,
            BodyRefs::NATIVE_REMOVE_DIR,
            BodyRefs::NATIVE_CHMOD,
            BodyRefs::NATIVE_QUARANTINE,
        ];
        let ops = non_deterministic_ops();
        for &r in expected {
            assert!(
                ops.contains(&r),
                "fileio BodyRef {r} missing from non_deterministic_ops"
            );
        }
    }

    #[test]
    fn non_deterministic_ops_still_covers_prior_ai_and_grpc_refs() {
        // Regression guard: the fileio addition must not have
        // dropped the pre-existing entries.
        let ops = non_deterministic_ops();
        for &r in &[
            BodyRefs::GPT4,
            BodyRefs::DALLE3,
            BodyRefs::TEXT_TO_AUDIO,
            BodyRefs::OLLAMA_CHAT,
            BodyRefs::OLLAMA_GENERATE,
            BodyRefs::OLLAMA_MODELS,
            BodyRefs::GRPC_TELL,
            BodyRefs::CHROMA_QUERY,
        ] {
            assert!(ops.contains(&r), "pre-existing BodyRef {r} missing");
        }
    }
}
