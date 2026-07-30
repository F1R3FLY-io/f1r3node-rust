use errors::InterpreterError;

pub mod accounting;
#[cfg(feature = "chromadb")]
pub mod chromadb_service;
#[cfg(not(feature = "chromadb"))]
#[path = "chromadb_service_stub.rs"]
pub mod chromadb_service;
pub mod compiler;
pub mod contract_call;
pub mod deploy_parameters;
pub mod dispatch;
pub mod env;
pub mod errors;
pub mod external_services;
pub mod fused_pathmap_chain;
pub mod grpc_client_service;
pub mod guard;
pub mod interpreter;
pub mod matcher;
pub mod metering;
pub mod merging;
pub mod metrics_constants;
pub mod ollama_service;
pub mod openai_service;
pub mod pretty_printer;
/// The printer's recursive oracle twin. `cfg(test)`: never built for production.
/// Its own text cites `739368a4` block by block; `rholang/tests/normalize_oracle_provenance.rs`
/// re-derives every block from git and compares it byte for byte.
#[cfg(test)]
pub mod pretty_printer_oracle;
pub mod reduce;
pub mod registry;
/// ★ Why a term rests. A **pull-based**, store-read-only analysis that gives a
/// resting send its reason without giving it a voice on the consensus path — see
/// the module's §4 for the invisibility argument, which is structural rather than
/// intentional.
pub mod rest_diagnosis;
pub mod rho_runtime;
pub mod rho_type;
pub mod storage;
pub mod substitute;
pub mod substitute_combine;
pub mod substitute_drive;
#[cfg(test)]
pub mod substitute_oracle;
pub mod system_processes;
pub mod test_utils;
pub mod util;

pub fn unwrap_option_safe<A: Clone + std::fmt::Debug>(
    opt: Option<A>,
) -> Result<A, InterpreterError> {
    opt.ok_or_else(|| {
        InterpreterError::UndefinedRequiredProtobufFieldError(format!(
            "{:?}",
            std::any::type_name::<A>()
        ))
    })
}
