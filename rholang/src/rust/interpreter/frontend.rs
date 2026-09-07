use std::collections::HashMap;

use models::rhoapi::Par;

use super::compiler::compiler::Compiler;

pub const PREPARED_PROGRAM_ABI_V1: u16 = 1;

pub struct PreparedProgram {
    process: Par,
}

impl PreparedProgram {
    pub fn from_normalized(process: Par) -> Self { Self { process } }

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
    frontend.prepare(source, environment)
}

pub(crate) struct LegacyCompilerFrontend;

impl ProgramFrontend for LegacyCompilerFrontend {
    fn abi_version(&self) -> u16 { PREPARED_PROGRAM_ABI_V1 }

    fn prepare(
        &self,
        source: &str,
        environment: HashMap<String, Par>,
    ) -> Result<PreparedProgram, PreparationError> {
        Compiler::source_to_adt_with_normalizer_env(source, environment)
            .map(PreparedProgram::from_normalized)
            .map_err(|error| PreparationError::new(error.to_string()))
    }
}
