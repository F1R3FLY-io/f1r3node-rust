use std::collections::HashMap;

use models::rhoapi::Par;
use models::rust::rholang::protobuf_encoder;

// Changed by the merge of feature/cost-accounted-rho (clippy gate): only
// the disabled `LegacyCompilerFrontend` below used this import.
// use super::compiler::compiler::Compiler;

pub const PREPARED_PROGRAM_ABI_V1: u16 = 1;
pub const PREPARATION_BINDING_V1: u16 = 1;

pub struct PreparedProgram {
    process: Par,
    source_binding: Option<PreparedSourceBinding>,
}

struct PreparedSourceBinding {
    source: Vec<u8>,
    language_identity: Vec<u8>,
    output: Vec<u8>,
}

impl PreparedProgram {
    pub fn from_normalized(process: Par) -> Self {
        Self {
            process,
            source_binding: None,
        }
    }

    pub fn from_normalized_with_binding(
        process: Par,
        source: Vec<u8>,
        language_identity: Vec<u8>,
        output: Vec<u8>,
    ) -> Result<Self, PreparationError> {
        if protobuf_encoder::encode_to_vec(&process) != output {
            return Err(PreparationError::new(
                "Prepared program output does not match its source binding",
            ));
        }
        Ok(Self {
            process,
            source_binding: Some(PreparedSourceBinding {
                source,
                language_identity,
                output,
            }),
        })
    }

    pub fn source_binding(&self) -> Option<(&[u8], &[u8], &[u8])> {
        self.source_binding.as_ref().map(|binding| {
            (
                binding.source.as_slice(),
                binding.language_identity.as_slice(),
                binding.output.as_slice(),
            )
        })
    }

    pub fn binding_version(&self) -> Option<u16> {
        self.source_binding.as_ref().map(|_| PREPARATION_BINDING_V1)
    }

    pub fn as_par(&self) -> &Par { &self.process }

    pub fn into_par(self) -> Par { self.process }

    pub fn abi_version(&self) -> u16 { PREPARED_PROGRAM_ABI_V1 }
}

#[derive(Debug, thiserror::Error)]
#[error("{diagnostic}")]
pub struct PreparationError {
    diagnostic: String,
}

impl PreparationError {
    pub fn new(diagnostic: impl Into<String>) -> Self {
        Self {
            diagnostic: diagnostic.into(),
        }
    }
}

pub trait ProgramFrontend: Send + Sync {
    fn abi_version(&self) -> u16;

    fn language_identity(&self) -> Option<&[u8]> { None }

    fn prepare(
        &self,
        source: &str,
        environment: HashMap<String, Par>,
    ) -> Result<PreparedProgram, PreparationError>;
}

pub fn prepare_program(
    frontend: &dyn ProgramFrontend,
    source: &str,
    environment: HashMap<String, Par>,
) -> Result<PreparedProgram, PreparationError> {
    let actual = frontend.abi_version();
    if actual != PREPARED_PROGRAM_ABI_V1 {
        return Err(PreparationError::new(format!(
            "Unsupported frontend ABI: expected {PREPARED_PROGRAM_ABI_V1}, got {actual}"
        )));
    }
    let expected_language = frontend.language_identity();
    let prepared = frontend.prepare(source, environment)?;
    match (expected_language, prepared.source_binding()) {
        (None, None) => {}
        (Some(_), None) => {
            return Err(PreparationError::new(
                "Frontend requires a prepared program source binding",
            ));
        }
        (None, Some(_)) => {
            return Err(PreparationError::new(
                "Frontend does not declare a language identity for the prepared program",
            ));
        }
        (Some(expected), Some((bound_source, bound_language, _))) => {
            if bound_source != source.as_bytes() {
                return Err(PreparationError::new(
                    "Prepared program source does not match the requested source",
                ));
            }
            if bound_language != expected {
                return Err(PreparationError::new(
                    "Prepared program language identity does not match the frontend",
                ));
            }
        }
    }
    Ok(prepared)
}

// Changed by the merge of feature/cost-accounted-rho (clippy gate): no code
// constructs this frontend since the checkpoint merge e5102e945. That merge
// moved `inj_attempt` to `ProgramInput::Source`, which parses through
// `Compiler::source_to_adt_with_normalizer_env` and maps a failure to the
// same `ParserError` text.
// pub(crate) struct LegacyCompilerFrontend;
//
// impl ProgramFrontend for LegacyCompilerFrontend {
//     fn abi_version(&self) -> u16 { PREPARED_PROGRAM_ABI_V1 }
//
//     fn prepare(
//         &self,
//         source: &str,
//         environment: HashMap<String, Par>,
//     ) -> Result<PreparedProgram, PreparationError> {
//         Compiler::source_to_adt_with_normalizer_env(source, environment)
//             .map(PreparedProgram::from_normalized)
//             .map_err(|error| PreparationError::new(error.to_string()))
//     }
// }
