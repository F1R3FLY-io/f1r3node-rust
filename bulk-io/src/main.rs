use std::path::{Path, PathBuf};

use bulk_io::export::{export, verify_export, ExportFormat};
use bulk_io::manifest::ImportManifest;
use bulk_io::stage::{prepare, stage_id, PrepareOptions};
use bulk_io::{deploy, reconcile, source, BulkError};
use clap::{Parser, Subcommand, ValueEnum};
use rholang::rust::interpreter::io::bulk::{hex_root, parse_hex_root, verify_tree, EMPTY_ROOT};

#[derive(Parser)]
#[command(
    name = "bulk-io",
    version,
    about = "Bulk import and export for the F1R3Node shard"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Csv,
    Jsonl,
}

#[derive(Subcommand)]
enum Command {
    SourceRoot {
        file: PathBuf,
    },
    ManifestHash {
        manifest: PathBuf,
    },
    StageId {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        base_root: Option<String>,
    },
    Prepare {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        import_root: PathBuf,
        #[arg(long)]
        source: Option<PathBuf>,
        #[arg(long, default_value = ".bulk-io-cache")]
        cache: PathBuf,
        #[arg(long)]
        base_root: Option<String>,
        #[arg(long)]
        expect_root: Option<String>,
        #[arg(long)]
        oracular_dir: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
    },
    Deploy {
        #[arg(long)]
        uri: Option<String>,
        #[command(subcommand)]
        action: DeployAction,
    },
    VerifyTree {
        dir: PathBuf,
    },
    Export {
        #[arg(long)]
        tree: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, value_enum, default_value = "csv")]
        format: Format,
        #[arg(long)]
        block_hash: Option<String>,
    },
    VerifyExport {
        dir: PathBuf,
        #[arg(long)]
        expect_root: Option<String>,
    },
    Reconcile {
        #[arg(long)]
        tree: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        base: Option<PathBuf>,
    },
    Uri,
}

#[derive(Subcommand)]
enum DeployAction {
    Stage {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        base_root: Option<String>,
        #[arg(long, default_value_t = 500)]
        expires_in: i64,
    },
    Attest {
        #[arg(long)]
        stage_id: String,
        #[arg(long)]
        result_root: String,
    },
    Commit {
        #[arg(long)]
        stage_id: String,
    },
    Abort {
        #[arg(long)]
        stage_id: String,
    },
    Expire {
        #[arg(long)]
        stage_id: String,
    },
    Status {
        #[arg(long)]
        stage_id: String,
    },
    Root {
        #[arg(long)]
        namespace: String,
    },
}

fn hex_opt(s: Option<&str>, what: &str) -> Result<Option<[u8; 32]>, BulkError> {
    s.map(|v| {
        parse_hex_root(v).ok_or_else(|| {
            BulkError::Manifest(format!("{what} must be 64 lowercase hex characters"))
        })
    })
    .transpose()
}

fn print_json<T: serde::Serialize>(v: &T) {
    println!("{}", serde_json::to_string_pretty(v).expect("json"));
}

fn resolve_source(
    m: &ImportManifest,
    local: Option<&Path>,
    cache: &Path,
) -> Result<PathBuf, BulkError> {
    let expected = m.source_root();
    match local {
        Some(p) => {
            let got = source::source_root(p)?;
            if got != expected {
                return Err(BulkError::Verify(format!(
                    "{} has root {}, manifest says {}",
                    p.display(),
                    hex_root(&got),
                    hex_root(&expected)
                )));
            }
            Ok(p.to_path_buf())
        }
        None => source::fetch_verified(&m.source.locations, &expected, cache),
    }
}

fn run(cli: Cli) -> Result<(), BulkError> {
    match cli.command {
        Command::SourceRoot { file } => println!("{}", hex_root(&source::source_root(&file)?)),
        Command::ManifestHash { manifest } => {
            println!("{}", hex_root(&ImportManifest::load(&manifest)?.hash()))
        }
        Command::StageId {
            manifest,
            base_root,
        } => {
            let m = ImportManifest::load(&manifest)?;
            let base = hex_opt(base_root.as_deref(), "base root")?.unwrap_or(EMPTY_ROOT);
            println!(
                "{}",
                hex_root(&stage_id(&m.namespace, &base, &m.source_root(), &m.hash()))
            );
        }
        Command::Prepare {
            manifest,
            import_root,
            source,
            cache,
            base_root,
            expect_root,
            oracular_dir,
            dry_run,
        } => {
            let m = ImportManifest::load(&manifest)?;
            let src = resolve_source(&m, source.as_deref(), &cache)?;
            let report = prepare(&m, &src, &PrepareOptions {
                import_root: &import_root,
                expected_base: hex_opt(base_root.as_deref(), "base root")?,
                expected_result: hex_opt(expect_root.as_deref(), "expected root")?,
                oracular_dir: oracular_dir.as_deref(),
                dry_run,
            })?;
            print_json(&report);
        }
        Command::Deploy { uri, action } => {
            let uri = uri.unwrap_or_else(deploy::default_uri);
            let term = match action {
                DeployAction::Stage {
                    manifest,
                    base_root,
                    expires_in,
                } => {
                    let m = ImportManifest::load(&manifest)?;
                    let base = hex_opt(base_root.as_deref(), "base root")?.unwrap_or(EMPTY_ROOT);
                    let sid = stage_id(&m.namespace, &base, &m.source_root(), &m.hash());
                    let report = bulk_io::stage::StageReport {
                        stage_id: hex_root(&sid),
                        namespace: m.namespace.clone(),
                        base_root: hex_root(&base),
                        source_root: m.source.root.clone(),
                        manifest_hash: hex_root(&m.hash()),
                        result_root: hex_root(&EMPTY_ROOT),
                        rows: 0,
                        upserts: 0,
                        deletes: 0,
                        rejected: 0,
                        records: 0,
                        partitions: 0,
                        files: 0,
                        bytes: 0,
                        oracular_records: 0,
                        path: None,
                        dry_run: true,
                    };
                    deploy::stage(&uri, &m, &report, expires_in)?
                }
                DeployAction::Attest {
                    stage_id,
                    result_root,
                } => deploy::attest(&uri, &stage_id, &result_root)?,
                DeployAction::Commit { stage_id } => deploy::commit(&uri, &stage_id)?,
                DeployAction::Abort { stage_id } => deploy::abort(&uri, &stage_id)?,
                DeployAction::Expire { stage_id } => deploy::expire(&uri, &stage_id)?,
                DeployAction::Status { stage_id } => deploy::status(&uri, &stage_id)?,
                DeployAction::Root { namespace } => deploy::root(&uri, &namespace)?,
            };
            print!("{term}");
        }
        Command::VerifyTree { dir } => println!("{}", hex_root(&verify_tree(&dir)?)),
        Command::Export {
            tree,
            out,
            format,
            block_hash,
        } => {
            let f = match format {
                Format::Csv => ExportFormat::Csv,
                Format::Jsonl => ExportFormat::Jsonl,
            };
            print_json(&export(&tree, &out, f, block_hash)?);
        }
        Command::VerifyExport { dir, expect_root } => {
            print_json(&verify_export(
                &dir,
                hex_opt(expect_root.as_deref(), "expected root")?,
            )?);
        }
        Command::Reconcile {
            tree,
            manifest,
            source,
            base,
        } => {
            let m = ImportManifest::load(&manifest)?;
            let rep = reconcile::reconcile(&tree, &m, &source, base.as_deref())?;
            print_json(&rep);
            if !rep.ok {
                return Err(BulkError::Verify("reconciliation found differences".into()));
            }
        }
        Command::Uri => println!("{}", deploy::default_uri()),
    }
    Ok(())
}

fn main() {
    if let Err(e) = run(Cli::parse()) {
        eprintln!("bulk-io: {e}");
        std::process::exit(1);
    }
}
