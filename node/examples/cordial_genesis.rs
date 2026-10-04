use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;

use clap::Parser;
use cordial_consensus::Chain;
use cordial_rholang::initialize_runtime;
use node::rust::consensus::cordial::ChainConfig;

#[derive(Parser)]
struct Options {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    output: PathBuf,
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let options = Options::parse();
    eyre::ensure!(
        !options.output.exists(),
        "Refusing to overwrite an existing chain configuration"
    );
    let mut bytes = Vec::new();
    File::open(&options.input)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    eyre::ensure!(
        bytes.len() <= 1024 * 1024,
        "Chain configuration exceeds size limit"
    );
    let mut config: ChainConfig = serde_json::from_slice(&bytes)?;
    config.chain = Chain::new(config.chain)?.spec().clone();
    let scratch = tempfile::tempdir()?;
    let (mut manager, runtime, root) =
        initialize_runtime(scratch.path(), &config.chain, &config.genesis).await?;
    drop(runtime);
    manager.shutdown().await?;
    drop(manager);
    eyre::ensure!(
        config.chain.execution_genesis == [0; 32] || config.chain.execution_genesis == root,
        "Existing genesis pin does not match the configuration"
    );
    config.chain.execution_genesis = root;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&options.output)?;
    output.write_all(&serde_json::to_vec_pretty(&config)?)?;
    output.sync_all()?;
    if let Some(parent) = options
        .output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        File::open(parent)?.sync_all()?;
    }
    println!("Pinned execution genesis: {}", hex::encode(root));
    println!("Chain configuration: {}", options.output.display());
    Ok(())
}
