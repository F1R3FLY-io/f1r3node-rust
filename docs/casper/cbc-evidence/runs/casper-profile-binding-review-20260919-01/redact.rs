use std::path::Path;
use std::process::Command;
use std::{env, fs};

fn digest(path: &Path) -> String {
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

fn walk(root: &Path, directory: &Path, replacements: &[(String, &str)]) {
    let mut entries: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap())
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            walk(root, &path, replacements);
        } else if kind.is_file() {
            let Ok(original) = String::from_utf8(fs::read(&path).unwrap()) else {
                continue;
            };
            let mut updated = original.clone();
            for (from, to) in replacements {
                assert!(!from.is_empty() && from != "/");
                updated = updated.replace(from, to);
            }
            if updated != original {
                let before = digest(&path);
                fs::write(&path, updated).unwrap();
                println!(
                    "{}\t{}\t{}",
                    path.strip_prefix(root).unwrap().display(),
                    before,
                    digest(&path)
                );
            }
        }
    }
}

fn main() {
    let args: Vec<_> = env::args().collect();
    assert_eq!(args.len(), 2);
    let temp = env::temp_dir()
        .to_string_lossy()
        .trim_end_matches('/')
        .to_owned();
    let alternate = if let Some(value) = temp.strip_prefix("/private") {
        value.to_owned()
    } else {
        format!("/private{temp}")
    };
    let replacements = vec![
        (
            env::current_dir().unwrap().display().to_string(),
            "[WORKSPACE_ROOT]",
        ),
        (env::var("HOME").unwrap(), "[HOME]"),
        (alternate, "[TEMP_ROOT]"),
        (temp, "[TEMP_ROOT]"),
    ];
    walk(Path::new(&args[1]), Path::new(&args[1]), &replacements);
}
