//! `mettail-elab <entry.module>` - elaborate and print the presentation.

use mettail_elab::resolve::FileResolver;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: mettail-elab <entry.module>");
        std::process::exit(2);
    }
    let p = PathBuf::from(&args[1]);
    let root = p.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
    let file = p.file_name().unwrap().to_string_lossy().to_string();
    let r = FileResolver { root };
    match mettail_elab::elaborate(&file, &r) {
        Ok(pres) => print!("{}", pres.render()),
        Err(d) => {
            eprintln!("{}: {}", file, d);
            std::process::exit(1);
        }
    }
}
