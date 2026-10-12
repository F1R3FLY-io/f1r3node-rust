#[path = "../authority_p2p.rs"]
mod p2p;
#[path = "../authority_process.rs"]
#[allow(dead_code)]
mod process;

use std::path::PathBuf;

use casper_soak::{regular, MAX_BYTES};
use eyre::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let request = PathBuf::from(
        std::env::var_os("CASPER_AUTHORITY_STEP_REQUEST")
            .ok_or_else(|| eyre::eyre!("The step request is required."))?,
    );
    let output = PathBuf::from(
        std::env::var_os("CASPER_AUTHORITY_STEP_OUTPUT")
            .ok_or_else(|| eyre::eyre!("The step output is required."))?,
    );
    p2p::execute(&regular(&request, MAX_BYTES)?, &output).await?;
    Ok(())
}
