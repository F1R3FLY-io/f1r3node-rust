use std::fs;
use std::process::Command;

use regex::Regex;
use yaml_rust2::scanner::{Scanner, Token, TokenType};
use yaml_rust2::yaml::Hash;
use yaml_rust2::{Yaml, YamlLoader};

const MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;
const MAX_FILES: u64 = 3;

fn field<'a>(value: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    value.as_hash()?.get(&Yaml::String(key.to_owned()))
}

fn resolve(value: Yaml, depth: usize) -> Result<Yaml, String> {
    if depth > 32 {
        return Err("YAML nesting exceeds the inspection limit.".into());
    }
    match value {
        Yaml::Hash(mut map) => {
            let mut result = Hash::new();
            if let Some(merge) = map.remove(&Yaml::String("<<".into())) {
                let sources = match merge {
                    Yaml::Hash(_) => vec![merge],
                    Yaml::Array(sources) => sources,
                    _ => return Err("A YAML merge must contain mappings.".into()),
                };
                for source in sources.into_iter().rev() {
                    match resolve(source, depth + 1)? {
                        Yaml::Hash(source) => result.extend(source),
                        _ => return Err("A YAML merge must contain mappings.".into()),
                    }
                }
            }
            for (key, value) in map {
                result.insert(key, resolve(value, depth + 1)?);
            }
            Ok(Yaml::Hash(result))
        }
        Yaml::Array(values) => values
            .into_iter()
            .map(|value| resolve(value, depth + 1))
            .collect::<Result<Vec<_>, _>>()
            .map(Yaml::Array),
        Yaml::Alias(_) | Yaml::BadValue => Err("A YAML value cannot be resolved.".into()),
        value => Ok(value),
    }
}

fn parse(text: &str) -> Result<Yaml, String> {
    if text.len() > 1_048_576 {
        return Err("The Compose source exceeds the inspection limit.".into());
    }
    for Token(mark, token) in Scanner::new(text.chars()) {
        if let TokenType::Tag(handle, suffix) = token {
            let prefix = text
                .lines()
                .nth(mark.line().saturating_sub(1))
                .and_then(|line| line.get(..mark.col()))
                .unwrap_or("")
                .trim();
            if handle != "!!" && !(handle == "!" && suffix == "override" && prefix == "ports:") {
                return Err("Compose tags require an explicit inspection rule.".into());
            }
        }
    }
    let mut docs = YamlLoader::load_from_str(text).map_err(|error| error.to_string())?;
    if docs.len() != 1 {
        return Err("A Compose source must contain one YAML document.".into());
    }
    let document = resolve(docs.remove(0), 0)?;
    if document.as_hash().is_none() {
        return Err("A Compose source must be a mapping.".into());
    }
    if field(&document, "include").is_some() {
        return Err("Compose includes require an explicit inspection rule.".into());
    }
    Ok(document)
}

fn bytes(value: &Yaml) -> Option<u64> {
    let value = value.as_str()?;
    let (digits, factor) = match value.as_bytes().last()? {
        b'k' | b'K' => (&value[..value.len() - 1], 1024),
        b'm' | b'M' => (&value[..value.len() - 1], 1024 * 1024),
        b'g' | b'G' => (&value[..value.len() - 1], 1024 * 1024 * 1024),
        _ => (value, 1),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u64>().ok()?.checked_mul(factor)
}

fn is_node(service: &Yaml) -> bool {
    let image = field(service, "image")
        .and_then(Yaml::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if [
        "f1r3fly-rust",
        "f1r3node",
        "rchain/rnode",
        "f1r3fly_image",
        "f1r3fly_rust_image",
    ]
    .iter()
    .any(|name| image.contains(name))
    {
        return true;
    }
    field(service, "command")
        .and_then(Yaml::as_vec)
        .is_some_and(|command| {
            let first = command.first().and_then(Yaml::as_str).unwrap_or("");
            (first == "run" || first.starts_with("--"))
                && command.iter().any(|arg| arg.as_str() == Some("run"))
        })
}

fn check(document: &Yaml) -> Result<usize, String> {
    let Some(services) = field(document, "services") else {
        return Ok(0);
    };
    let services = services
        .as_hash()
        .ok_or("Compose services must be a mapping.")?;
    let mut checked = 0;
    for (name, service) in services {
        let name = name.as_str().ok_or("A service name must be a string.")?;
        if service.as_hash().is_none() || field(service, "extends").is_some() {
            return Err(format!(
                "{name}: Service inheritance requires an explicit inspection rule."
            ));
        }
        if !is_node(service) {
            continue;
        }
        let fail = |reason: &str| format!("{name}: {reason}");
        let logging =
            field(service, "logging").ok_or_else(|| fail("Container logging is missing."))?;
        if field(logging, "driver").and_then(Yaml::as_str) != Some("json-file") {
            return Err(fail("The bounded json-file driver is required."));
        }
        let options =
            field(logging, "options").ok_or_else(|| fail("Logging options are missing."))?;
        let size = field(options, "max-size").and_then(bytes);
        let count = field(options, "max-file")
            .and_then(Yaml::as_str)
            .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|value| value.parse::<u64>().ok());
        if !size.is_some_and(|size| (1..=MAX_FILE_BYTES).contains(&size))
            || !count.is_some_and(|count| (1..=MAX_FILES).contains(&count))
        {
            return Err(fail(
                "Positive literal log caps must not exceed 100 MiB per file and three files.",
            ));
        }
        let command = field(service, "command")
            .and_then(Yaml::as_vec)
            .ok_or_else(|| fail("The node command must be an argument list."))?;
        let args = command
            .iter()
            .map(|arg| {
                arg.as_str()
                    .ok_or_else(|| fail("Node arguments must be strings."))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let run = args
            .iter()
            .position(|arg| *arg == "run")
            .ok_or_else(|| fail("The node run subcommand is missing."))?;
        let mut sinks = Vec::new();
        for (index, arg) in args.iter().enumerate() {
            if let Some(sink) = arg.strip_prefix("--log-sink=") {
                sinks.push((index, Some(sink)));
            } else if *arg == "--log-sink" {
                sinks.push((index, args.get(index + 1).copied()));
            }
        }
        if sinks.len() != 1 || !matches!(sinks[0], (index, Some("stdout" | "file")) if index < run)
        {
            return Err(fail(
                "Select one explicit log sink before the run subcommand.",
            ));
        }
        checked += 1;
    }
    Ok(checked)
}

fn overlay(base: &mut Yaml, update: Yaml) {
    if let (Yaml::Hash(base), Yaml::Hash(update)) = (&mut *base, &update) {
        for (key, value) in update {
            if let Some(current) = base.get_mut(key) {
                overlay(current, value.clone());
            } else {
                base.insert(key.clone(), value.clone());
            }
        }
    } else {
        *base = update;
    }
}

fn candidate(text: &str) -> bool {
    if text.len() > 1_048_576 {
        return true;
    }
    match YamlLoader::load_from_str(text) {
        Ok(documents) => documents.into_iter().any(|document| {
            resolve(document, 0).map_or(true, |document| {
                field(&document, "services").is_some() || field(&document, "include").is_some()
            })
        }),
        Err(_) => Regex::new(r#"(?m)^\s*(?:services|["']services["']|include)\s*:|^\s*\{[^\n]*(?:services|["']services["'])\s*:"#)
            .unwrap()
            .is_match(text),
    }
}

#[test]
fn repository_compose_nodes_have_single_sinks_and_container_caps() {
    let root = super::root();
    let paths = Command::new("git")
        .current_dir(&root)
        .args(["ls-files", "-z", "--", "*.yml", "*.yaml"])
        .output()
        .unwrap();
    assert!(
        paths.status.success(),
        "Cannot enumerate repository YAML files."
    );
    let paths = String::from_utf8(paths.stdout).unwrap();
    let mut checked = 0;
    let mut failures = Vec::new();
    for path in paths.split('\0').filter(|path| !path.is_empty()) {
        let source = fs::read_to_string(root.join(path)).unwrap();
        if !candidate(&source) {
            continue;
        }
        match parse(&source).and_then(|document| check(&document)) {
            Ok(count) => checked += count,
            Err(error) => failures.push(format!("{path}: {error}")),
        }
    }
    assert!(
        checked >= 12 || !failures.is_empty(),
        "The node deployment inventory is incomplete."
    );
    assert!(
        failures.is_empty(),
        "Unsafe node deployments:\n{}",
        failures.join("\n")
    );
}

#[test]
fn ci_port_overlays_preserve_node_log_policy() {
    for (base, update, expected) in [
        ("docker/shard.yml", "docker/ci-ports.shard.yml", 5),
        ("docker/standalone.yml", "docker/ci-ports.standalone.yml", 1),
    ] {
        let root = super::root();
        let mut document = parse(&fs::read_to_string(root.join(base)).unwrap()).unwrap();
        overlay(
            &mut document,
            parse(&fs::read_to_string(root.join(update)).unwrap()).unwrap(),
        );
        assert_eq!(check(&document).unwrap(), expected, "{base} + {update}");
    }
}

const VALID: &str = "x-node: &node\n  image: f1r3flyindustries/f1r3fly-rust:latest\n  command: [--log-sink=stdout, run]\n  logging:\n    driver: json-file\n    options: {max-size: 100m, max-file: '3'}\nservices:\n  renamed-node:\n    <<: *node\n";

#[test]
fn yaml_anchors_and_new_node_names_are_checked() {
    assert_eq!(check(&parse(VALID).unwrap()).unwrap(), 1);
    assert_eq!(
        check(&parse(&VALID.replace("stdout", "file")).unwrap()).unwrap(),
        1
    );
    assert_eq!(
        check(&parse(&VALID.replace("--log-sink=stdout", "--log-sink, stdout")).unwrap()).unwrap(),
        1
    );
    let inline = "{'services': {'new-node': {image: 'custom-image', command: [--log-sink=stdout, run], logging: {driver: json-file, options: {max-size: 1k, max-file: '1'}}}}}";
    assert!(candidate(inline));
    assert_eq!(check(&parse(inline).unwrap()).unwrap(), 1);
    let escaped = inline.replace("'services'", r#""\x73ervices""#);
    assert!(candidate(&escaped));
    assert_eq!(check(&parse(&escaped).unwrap()).unwrap(), 1);
    let uncapped = escaped.replace("max-file: '1'", "ignored-count: '1'");
    assert!(candidate(&uncapped));
    assert!(check(&parse(&uncapped).unwrap()).is_err());
    assert_eq!(
        check(&parse("services: {collector: {image: 'prom/prometheus'}}").unwrap()).unwrap(),
        0
    );
}

#[test]
fn unsafe_log_policy_mutations_are_refused() {
    let mutations = [
        ("  logging:", "  ignored-logging:"),
        ("driver: json-file", "driver: none"),
        ("driver: json-file", "driver: local"),
        ("options:", "ignored-options:"),
        ("max-size:", "ignored-size:"),
        ("max-file:", "ignored-count:"),
        ("100m", "0"),
        ("100m", "'-1m'"),
        ("100m", "101m"),
        ("100m", "18446744073709551615g"),
        ("100m", "'${LOG_SIZE:-100m}'"),
        ("100m", "false"),
        ("'3'", "'0'"),
        ("'3'", "'-1'"),
        ("'3'", "'4'"),
        ("'3'", "'18446744073709551616'"),
        ("'3'", "'${LOG_COUNT:-3}'"),
        ("'3'", "null"),
        ("'3'", "true"),
        ("'3'", "[]"),
        ("--log-sink=stdout, run", "run"),
        ("--log-sink=stdout, run", "--log-sink=both, run"),
        ("--log-sink=stdout, run", "run, --log-sink=stdout"),
        (
            "--log-sink=stdout, run",
            "--log-sink=stdout, --log-sink=file, run",
        ),
        ("--log-sink=stdout, run", "--log-sink, run"),
        ("--log-sink=stdout, run", "--log-sink='stdout', run"),
    ];
    for (old, new) in mutations {
        let changed = VALID.replace(old, new);
        assert_ne!(changed, VALID);
        assert!(
            parse(&changed)
                .and_then(|document| check(&document))
                .is_err(),
            "Mutation accepted: {old} -> {new}"
        );
    }
}

#[test]
fn malformed_yaml_and_unmodeled_compose_features_are_refused() {
    for text in [
        "services: {node: [}",
        "services: {}\nservices: {}",
        "services: {}\n---\nservices: {}",
        "services: []",
        "services: {node: null}",
        "include: other.yml\nservices: {}",
        "services: {node: {extends: other}}",
        "services: {node: {<<: invalid}}",
        "services: {node: *missing}",
        "services: {node: {logging: !reset null}}",
        "services: {node: {logging: !override {}}}",
    ] {
        assert!(
            parse(text).and_then(|document| check(&document)).is_err(),
            "Unsupported source accepted: {text}"
        );
    }
    assert!(parse(&" ".repeat(1_048_577)).is_err());
}

#[test]
fn yaml_merge_precedence_and_service_overrides_are_checked() {
    let source =
        format!("{VALID}  overridden:\n    <<: *node\n    logging: {{driver: json-file}}\n");
    assert!(check(&parse(&source).unwrap()).is_err());
    let source =
        format!("{VALID}  overridden:\n    <<: *node\n    command: [--log-sink=both, run]\n");
    assert!(check(&parse(&source).unwrap()).is_err());
    let source = "x-safe: &safe {logging: {driver: json-file, options: {max-size: 100m, max-file: '3'}}}\nx-unsafe: &unsafe {logging: {driver: none}}\nservices:\n  node:\n    <<: [*safe, *unsafe]\n    image: custom\n    command: [--log-sink=stdout, run]\n";
    assert_eq!(check(&parse(source).unwrap()).unwrap(), 1);
    assert!(
        check(&parse(&source.replace("[*safe, *unsafe]", "[*unsafe, *safe]")).unwrap()).is_err()
    );
}

#[test]
fn compose_overlay_log_overrides_are_checked() {
    let mut document = parse(VALID).unwrap();
    overlay(
        &mut document,
        parse("services:\n  renamed-node:\n    ports: !override [40400:40400]\n").unwrap(),
    );
    assert_eq!(check(&document).unwrap(), 1);
    for patch in [
        "logging: null",
        "logging: {driver: none}",
        "logging: {options: {max-file: '0'}}",
        "logging: {options: {max-size: 101m}}",
        "command: [run]",
        "command: [--log-sink=both, run]",
    ] {
        let mut document = parse(VALID).unwrap();
        overlay(
            &mut document,
            parse(&format!("services:\n  renamed-node:\n    {patch}\n")).unwrap(),
        );
        assert!(check(&document).is_err(), "Overlay accepted: {patch}");
    }
}
