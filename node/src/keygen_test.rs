use std::collections::HashSet;

use clap::Parser;
use colored::ColoredString;

use super::*;

struct TestConsole;

impl ConsoleIO for TestConsole {
    fn read_line(&mut self) -> Result<String> { Ok(String::new()) }

    fn read_password(&mut self, _prompt: &str) -> Result<String> { Ok("test-password".to_string()) }

    fn println_str(&mut self, _text: &str) -> Result<()> { Ok(()) }

    fn println_colored(&mut self, _text: &ColoredString) -> Result<()> { Ok(()) }

    fn update_completion(&mut self, _history: &HashSet<String>) -> Result<()> { Ok(()) }

    fn close(&mut self) -> Result<()> { Ok(()) }
}

#[test]
fn keygen_succeeds_without_a_reachable_node() -> Result<()> {
    let path = std::env::temp_dir().join(format!("f1r3-keygen-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&path)?;
    let options = Options::try_parse_from([
        "node",
        "--grpc-host",
        "127.0.0.1",
        "--grpc-port",
        "0",
        "keygen",
        path.to_str()
            .ok_or_else(|| eyre::eyre!("Invalid temporary path"))?,
    ])?;
    let rt = Builder::new_current_thread().enable_all().build()?;

    let result = run_cli(options, &rt, &mut TestConsole);
    let keys_exist = ["f1r3fly.key", "f1r3fly.pub.pem", "f1r3fly.pub.hex"]
        .iter()
        .all(|name| path.join(name).is_file());
    std::fs::remove_dir_all(&path)?;

    result?;
    assert!(keys_exist);
    Ok(())
}
