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
pub mod grpc_client_service;
pub mod host_work;
pub mod interpreter;
pub mod io;
pub mod matcher;
pub mod merging;
pub mod metrics_constants;
pub mod ollama_service;
pub mod openai_service;
pub mod pretty_printer;
pub mod reduce;
pub mod registry;
pub mod rho_runtime;
pub mod rho_source;
pub mod rho_type;
pub mod storage;
pub mod substitute;
pub mod system_processes;
pub mod test_utils;
pub mod util;

pub fn unwrap_option_safe<A>(opt: Option<A>) -> Result<A, InterpreterError> {
    opt.ok_or_else(missing_field::<A>)
}

/// `unwrap_option_safe` for a field that is only read.
pub fn unwrap_option_ref<A>(opt: &Option<A>) -> Result<&A, InterpreterError> {
    opt.as_ref().ok_or_else(missing_field::<A>)
}

fn missing_field<A>() -> InterpreterError {
    InterpreterError::UndefinedRequiredProtobufFieldError(format!(
        "{:?}",
        std::any::type_name::<A>()
    ))
}
