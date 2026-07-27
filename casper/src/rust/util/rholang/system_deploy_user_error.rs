// See casper/src/main/scala/coop/rchain/casper/util/rholang/SystemDeployUserError.scala

use std::fmt;

use models::rhoapi::Par;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::pretty_printer::PrettyPrinter;

#[derive(Debug)]
pub struct SystemDeployUserError {
    pub error_message: String,
}

impl SystemDeployUserError {
    pub fn new(error_message: String) -> Self { Self { error_message } }
}

/**
 * Fatal error - node should exit on these errors.
 */
#[derive(Debug, Clone, PartialEq)]
pub enum SystemDeployPlatformFailure {
    UnexpectedResult(Vec<Par>),
    UnexpectedSystemErrors(Vec<InterpreterError>),
    GasRefundFailure(String),
    ConsumeFailed,
}

/// Render a system deploy's unexpected result for
/// [`SystemDeployUserError::error_message`].
///
/// ★ **This is a consensus render.** Its bytes become
/// `ProcessedSystemDeploy::Failed { error_msg }` in the block, and
/// `ReplayRuntimeOps::replay_system_deploy_internal` compares them for byte
/// equality against the string the replaying validator computes. A difference
/// is `ReplayFailure::system_deploy_error_mismatch` — a rejected block — so
/// every input to this function must come from the block, and nothing here may
/// read the process environment.
///
/// It used to build the printer with `PrettyPrinter::new()`, which trims every
/// node of the render — the finished string *and* each sub-render inside a
/// catching scope — to `PRETTY_PRINTER_OUTPUT_TRIM_AFTER`. Two validators with
/// different settings of that operator-tunable variable therefore produced
/// different `error_message` bytes for the same failing system deploy. It now
/// builds the printer with `PrettyPrinter::for_consensus()`, whose budget is a
/// compile-time constant.
/// `casper/tests/system_deploy_error_message_determinism.rs` is the gate.
///
/// The other two arms were already environment-independent and are unchanged:
/// `[]` is a literal, and the many-`Par` arm formats with `Debug`, which the
/// printer's budget does not reach.
fn show_seq_par(pars: &[Par]) -> String {
    match pars {
        [] => "Nil".to_string(),
        [single] => {
            let mut pretty_printer = PrettyPrinter::for_consensus();
            pretty_printer.build_channel_string(single)
        }
        _ => format!(
            "({})",
            pars.iter()
                .map(|p| format!("{:?}", p))
                .collect::<Vec<_>>()
                .join(",\n ")
        ),
    }
}

impl std::error::Error for SystemDeployPlatformFailure {}

impl fmt::Display for SystemDeployPlatformFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SystemDeployPlatformFailure::UnexpectedResult(results) => {
                write!(f, "Unable to proceed with {}", show_seq_par(results))
            }
            SystemDeployPlatformFailure::UnexpectedSystemErrors(errors) => {
                write!(f, "Caught errors in Rholang interpreter {:?}", errors)
            }
            SystemDeployPlatformFailure::GasRefundFailure(msg) => {
                write!(f, "Unable to refund remaining gas ({})", msg)
            }
            SystemDeployPlatformFailure::ConsumeFailed => {
                write!(f, "Unable to consume results of system deploy")
            }
        }
    }
}

impl From<SystemDeployPlatformFailure> for SystemDeployUserError {
    fn from(failure: SystemDeployPlatformFailure) -> Self {
        Self {
            error_message: format!("Platform failure: {}", failure),
        }
    }
}
