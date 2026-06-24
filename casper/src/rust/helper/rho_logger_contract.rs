// See casper/src/test/scala/coop/rchain/casper/helper/RhoLoggerContract.scala

use models::rhoapi::{ListParWithRandom, Par};
use rholang::rust::interpreter::contract_call::ContractCall;
use rholang::rust::interpreter::errors::{illegal_argument_error, InterpreterError};
use rholang::rust::interpreter::pretty_printer::PrettyPrinter;
use rholang::rust::interpreter::rho_type::RhoString;
use rholang::rust::interpreter::system_processes::ProcessContext;

pub struct RhoLoggerContract;

impl RhoLoggerContract {
    pub async fn std_log(
        ctx: ProcessContext,
        message: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let mut pretty_printer = PrettyPrinter::new();

        let is_contract_call = ContractCall {
            space: ctx.space.clone(),
            dispatcher: ctx.dispatcher.clone(),
        };

        if let Some((_, _, _, args)) = is_contract_call.unapply(message) {
            match args.as_slice() {
                [log_level_par, par] => {
                    if let Some(log_level) = RhoString::unapply(log_level_par) {
                        let msg = pretty_printer.build_string_from_message(par);

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
}
