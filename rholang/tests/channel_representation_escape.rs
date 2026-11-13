use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const PAR_SOURCE: &str = "models/src/rust/par.rs";
const MODELS_CRATE_ROOT: &str = "models/src/lib.rs";
const MODELS_RUST_MODULE: &str = "models/src/rust/mod.rs";
const RHOLANG_CRATE_ROOT: &str = "rholang/src/lib.rs";
const PAR_SCHEMA: &str = "models/src/main/protobuf/RhoTypes.proto";

const TYPE_KEYWORDS: &[&str] = &["struct", "enum", "union", "type", "trait"];

const SCANNED_ROOTS: &[(&str, usize)] = &[
    ("models/src", 100),
    ("rholang/src", 100),
    ("rspace++/src", 100),
];
const MINIMUM_DECLARATIONS: usize = 1500;

const PAR_COMPONENT_TYPES: [&str; 10] = [
    "Send",
    "Receive",
    "New",
    "Expr",
    "Match",
    "GUnforgeable",
    "Bundle",
    "Connective",
    "If",
    "CostTokenStack",
];

const COMPONENT_VECTOR_HOLDERS: &[(&str, &str, &str)] = &[
    (
        "models/src/rust/par.rs",
        "HumanReadableParImage",
        "private serde image of a human-readable Par; into_par inserts every family into a ParBag",
    ),
    (
        "models/src/rust/rholang/bincode_decoder.rs",
        "Machine",
        "private state of the bincode decoder; one value stack per decoded message type",
    ),
    (
        "rholang/src/rust/interpreter/substitute_drive.rs",
        "SubWork",
        "work item of the substitution driver; rest is the remaining input of one resumable fold",
    ),
    (
        "rholang/src/rust/interpreter/substitute_drive.rs",
        "SubKont",
        "continuation of the substitution driver; rest is the remaining input of its fold",
    ),
];

const FIXTURE_FILE: &str = "fixture";
const FIXTURE_SOURCE: &str = concat!(
    "// struct InLineComment { sends: Vec<Send>, receives: Vec<Receive> }\n",
    "/* struct InBlockComment { sends: Vec<Send>, receives: Vec<Receive> } */\n",
    "const TEXT: &str = \"struct InString { sends: Vec<Send>, receives: Vec<Receive> }\";\n",
    "struct OneFamily { sends: Vec<Send>, more_sends: Vec<Send>, bytes: Vec<u8> }\n",
    "fn combine(sends: Vec<Send>, receives: Vec<Receive>) {}\n",
    "pub struct LegacyParShadow {\n",
    "    pub sends: Vec<Send>,\n",
    "    pub receives: Vec<Receive>,\n",
    "}\n",
    "enum TupleShadow { Both(Vec<models::rhoapi::Expr>, Vec<Connective>) }\n",
);

struct Declaration {
    keyword: &'static str,
    name: String,
    body: String,
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rholang has a workspace parent")
        .to_path_buf()
}

fn read_source(relative: &str) -> String {
    std::fs::read_to_string(workspace_root().join(relative))
        .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

fn rust_sources_below(root: &Path) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("cannot list {}: {error}", directory.display()));
        for entry in entries {
            let path = entry
                .unwrap_or_else(|error| panic!("cannot list {}: {error}", directory.display()))
                .path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name != "target") {
                    pending.push(path);
                }
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push(path);
            }
        }
    }
    sources.sort();
    sources
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$') || !byte.is_ascii()
}

fn leading(bytes: &[u8], accepts: impl Fn(u8) -> bool) -> usize {
    bytes.iter().take_while(|byte| accepts(**byte)).count()
}

fn masked_span_end(raw: &[u8], start: usize) -> Option<usize> {
    let rest = &raw[start..];
    if rest.starts_with(b"//") {
        return Some(start + leading(rest, |byte| byte != b'\n'));
    }
    if rest.starts_with(b"/*") {
        let mut depth = 1usize;
        let mut end = start + 2;
        while end < raw.len() && depth != 0 {
            if raw[end..].starts_with(b"/*") {
                depth += 1;
                end += 2;
            } else if raw[end..].starts_with(b"*/") {
                depth -= 1;
                end += 2;
            } else {
                end += 1;
            }
        }
        return Some(end);
    }
    match rest[0] {
        b'r' => {
            let hashes = leading(&rest[1..], |byte| byte == b'#');
            let quote = start + 1 + hashes;
            if raw.get(quote) != Some(&b'"') {
                return None;
            }
            let mut end = quote + 1;
            while end < raw.len() {
                if raw[end] == b'"' && leading(&raw[end + 1..], |byte| byte == b'#') >= hashes {
                    return Some(end + 1 + hashes);
                }
                end += 1;
            }
            Some(raw.len())
        }
        b'"' => {
            let mut end = start + 1;
            while end < raw.len() {
                match raw[end] {
                    b'\\' => end += 2,
                    b'"' => return Some(end + 1),
                    _ => end += 1,
                }
            }
            Some(raw.len())
        }
        b'\'' => {
            let mut end = start + 1;
            if raw.get(end) == Some(&b'\\') {
                end += 2;
                end += leading(raw.get(end..)?, |byte| byte != b'\'' && byte != b'\n');
            } else {
                end += 1;
                end += leading(raw.get(end..)?, |byte| byte & 0xc0 == 0x80);
            }
            (raw.get(end) == Some(&b'\'')).then_some(end + 1)
        }
        _ => None,
    }
}

fn structural_text(source: &str) -> String {
    let raw = source.as_bytes();
    let mut structural = raw.to_vec();
    let mut index = 0usize;
    while index < raw.len() {
        let Some(end) = masked_span_end(raw, index) else {
            index += 1;
            continue;
        };
        for byte in &mut structural[index..end] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
        index = end;
    }
    String::from_utf8(structural).expect("masking replaces whole comments and literals by spaces")
}

fn body_end(code: &[u8], start: usize) -> usize {
    let (mut parentheses, mut brackets, mut braces) = (0usize, 0usize, 0usize);
    for (offset, byte) in code[start..].iter().enumerate() {
        match byte {
            b'(' => parentheses += 1,
            b')' => parentheses = parentheses.saturating_sub(1),
            b'[' => brackets += 1,
            b']' => brackets = brackets.saturating_sub(1),
            b'{' => braces += 1,
            b'}' if braces <= 1 => return start + offset + 1,
            b'}' => braces -= 1,
            b';' if braces + parentheses + brackets == 0 => return start + offset + 1,
            _ => {}
        }
    }
    code.len()
}

fn declarations(code: &str, keywords: &[&'static str]) -> Vec<Declaration> {
    let bytes = code.as_bytes();
    let mut found = Vec::new();
    for keyword in keywords.iter().copied() {
        for (position, _) in code.match_indices(keyword) {
            let after = position + keyword.len();
            let bounded = (position == 0 || !is_identifier_byte(bytes[position - 1]))
                && bytes.get(after).is_some_and(u8::is_ascii_whitespace);
            if !bounded {
                continue;
            }
            let name_start = after + leading(&bytes[after..], |byte| byte.is_ascii_whitespace());
            let name_end = name_start + leading(&bytes[name_start..], is_identifier_byte);
            if name_end == name_start {
                continue;
            }
            found.push(Declaration {
                keyword,
                name: code[name_start..name_end].to_owned(),
                body: code[name_end..body_end(bytes, name_end)].to_owned(),
            });
        }
    }
    found
}

fn generic_arguments<'a>(code: &'a str, type_name: &str) -> Vec<&'a str> {
    let bytes = code.as_bytes();
    let mut arguments = Vec::new();
    for (position, _) in code.match_indices(type_name) {
        if position > 0 && is_identifier_byte(bytes[position - 1]) {
            continue;
        }
        let mut open = position + type_name.len();
        open += leading(&bytes[open..], |byte| byte.is_ascii_whitespace());
        if bytes[open..].starts_with(b"::") {
            open += 2;
        }
        if bytes.get(open) != Some(&b'<') {
            continue;
        }
        let mut depth = 0usize;
        for (offset, byte) in bytes[open..].iter().enumerate() {
            match byte {
                b'<' => depth += 1,
                b'>' if bytes[open + offset - 1] != b'-' => {
                    depth -= 1;
                    if depth == 0 {
                        arguments.push(code[open + 1..open + offset].trim());
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    arguments
}

fn imported_paths(code: &str) -> Vec<(String, String)> {
    let bytes = code.as_bytes();
    let mut imports = Vec::new();
    for (position, _) in code.match_indices("use") {
        let after = position + 3;
        let bounded = (position == 0 || !is_identifier_byte(bytes[position - 1]))
            && bytes.get(after).is_some_and(u8::is_ascii_whitespace);
        if !bounded {
            continue;
        }
        let end = code[after..]
            .find(';')
            .map_or(code.len(), |offset| after + offset);
        let mut prefixes: Vec<String> = Vec::new();
        let mut leaf = String::new();
        for symbol in code[after..end].chars().chain(std::iter::once(',')) {
            match symbol {
                '{' => {
                    let outer = prefixes.last().map_or("", String::as_str);
                    let prefix = format!("{outer}{}", leaf.trim());
                    prefixes.push(prefix);
                    leaf.clear();
                }
                ',' | '}' => {
                    let text = leaf.trim();
                    if !text.is_empty() {
                        let (target, alias) = text
                            .split_once(" as ")
                            .map_or((text, None), |(target, alias)| (target.trim(), Some(alias)));
                        let prefix = prefixes.last().map_or("", String::as_str);
                        let path = format!("{prefix}{target}");
                        let name = alias
                            .or_else(|| path.rsplit("::").next())
                            .map_or_else(String::new, |name| name.trim().to_owned());
                        imports.push((name, path));
                    }
                    leaf.clear();
                    if symbol == '}' {
                        prefixes.pop();
                    }
                }
                _ => leaf.push(if symbol.is_whitespace() { ' ' } else { symbol }),
            }
        }
    }
    imports
}

fn par_bindings(code: &str) -> Vec<String> {
    let imported = imported_paths(code)
        .into_iter()
        .filter(|(name, _)| name == "Par")
        .map(|(_, path)| format!("use {path}"));
    let declared = declarations(code, TYPE_KEYWORDS)
        .into_iter()
        .filter(|declaration| declaration.name == "Par")
        .map(|declaration| format!("{} Par", declaration.keyword));
    imported.chain(declared).collect()
}

fn after_group(text: &str, open: u8, close: u8) -> &str {
    let mut depth = 0usize;
    for (offset, byte) in text.bytes().enumerate() {
        if byte == open {
            depth += 1;
        } else if byte == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return &text[offset + 1..];
            }
        }
    }
    ""
}

fn named_fields(body: &str) -> Vec<(String, String)> {
    let open = body.find('{').map_or(body.len(), |position| position + 1);
    let close = body
        .rfind('}')
        .map_or(body.len(), |position| position.max(open));
    let mut fields = Vec::new();
    let mut depth = 0usize;
    let mut field_start = open;
    let separators = body.bytes().enumerate().take(close).skip(open);
    for (position, byte) in separators.chain(std::iter::once((close, b','))) {
        match byte {
            b'<' | b'(' | b'[' | b'{' => depth += 1,
            b'>' | b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                let mut field = body[field_start..position].trim();
                while field.starts_with("#[") {
                    field = after_group(field, b'[', b']').trim_start();
                }
                let visibility = field
                    .strip_prefix("pub")
                    .filter(|rest| rest.starts_with(['(', ' ']));
                if let Some(rest) = visibility.map(str::trim_start) {
                    field = if rest.starts_with('(') {
                        after_group(rest, b'(', b')').trim_start()
                    } else {
                        rest
                    };
                }
                if let Some((name, kind)) = field.split_once(':') {
                    fields.push((name.trim().to_owned(), kind.trim().to_owned()));
                }
                field_start = position + 1;
            }
            _ => {}
        }
    }
    fields
}

fn par_message_component_types() -> Vec<String> {
    read_source(PAR_SCHEMA)
        .lines()
        .map(str::trim)
        .skip_while(|line| *line != "message Par {")
        .skip(1)
        .take_while(|line| *line != "}")
        .filter_map(|line| line.strip_prefix("repeated "))
        .filter_map(|field| field.split_whitespace().next())
        .map(str::to_owned)
        .collect()
}

fn component_vector_holders(
    file: &str,
    source: &str,
) -> (usize, BTreeMap<(String, String), String>) {
    let declared = declarations(&structural_text(source), &["struct", "enum"]);
    let mut holders = BTreeMap::new();
    for declaration in &declared {
        let components: BTreeSet<&str> = generic_arguments(&declaration.body, "Vec")
            .into_iter()
            .map(|element| element.rsplit("::").next().map_or(element, str::trim))
            .filter(|element| PAR_COMPONENT_TYPES.contains(element))
            .collect();
        if components.len() >= 2 {
            let components = components.into_iter().collect::<Vec<_>>().join(", ");
            holders.insert((file.to_owned(), declaration.name.clone()), components);
        }
    }
    (declared.len(), holders)
}

fn unlisted_holders(holders: &BTreeMap<(String, String), String>) -> Vec<String> {
    holders
        .iter()
        .filter(|((file, name), _)| {
            !COMPONENT_VECTOR_HOLDERS
                .iter()
                .any(|(listed_file, listed_name, _)| listed_file == file && listed_name == name)
        })
        .map(|((file, name), components)| format!("{file}: {name} holds vectors of {components}"))
        .collect()
}

#[test]
fn par_struct_has_exactly_the_component_bag_and_two_cached_summaries() {
    let code = structural_text(&read_source(PAR_SOURCE));
    assert_eq!(
        par_bindings(&code),
        ["struct Par"],
        "{PAR_SOURCE} must declare exactly one type named Par, a struct"
    );
    let par = declarations(&code, &["struct"])
        .into_iter()
        .find(|declaration| declaration.name == "Par")
        .expect("the struct Par was found above");
    let fields = named_fields(&par.body);
    let names: Vec<&str> = fields.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        ["components", "locally_free", "connective_used"],
        "{PAR_SOURCE}: struct Par must have exactly these fields, found {fields:?}"
    );
    assert_eq!(
        fields[0].1, "ParBag",
        "{PAR_SOURCE}: the components of Par must be one ParBag, found {fields:?}"
    );
}

#[test]
fn rspace_instances_take_the_bag_backed_par_as_their_channel_type() {
    let rholang_code = structural_text(&read_source(RHOLANG_CRATE_ROOT));
    for space in ["RSpace", "ReplayRSpace"] {
        let channel_types: Vec<&str> = generic_arguments(&rholang_code, space)
            .into_iter()
            .map(|arguments| arguments.split(',').next().map_or(arguments, str::trim))
            .collect();
        assert!(
            !channel_types.is_empty(),
            "{RHOLANG_CRATE_ROOT} instantiates no {space}<...>"
        );
        assert!(
            channel_types.iter().all(|channel| *channel == "Par"),
            "{RHOLANG_CRATE_ROOT}: every {space}<...> must take Par as its channel type, found \
             {channel_types:?}"
        );
    }
    assert_eq!(
        par_bindings(&rholang_code),
        ["use models::rhoapi::Par"],
        "{RHOLANG_CRATE_ROOT} must bind the name Par only by importing models::rhoapi::Par"
    );

    let models_code = structural_text(&read_source(MODELS_CRATE_ROOT));
    let rhoapi_modules: Vec<Declaration> = declarations(&models_code, &["mod"])
        .into_iter()
        .filter(|declaration| declaration.name == "rhoapi")
        .collect();
    assert_eq!(
        rhoapi_modules.len(),
        1,
        "{MODELS_CRATE_ROOT} must declare exactly one module rhoapi"
    );
    assert_eq!(
        par_bindings(&rhoapi_modules[0].body),
        ["use crate::rust::par::Par"],
        "{MODELS_CRATE_ROOT}: rhoapi must bind Par only by re-exporting crate::rust::par::Par"
    );

    let module_code = structural_text(&read_source(MODELS_RUST_MODULE));
    let module_lines: Vec<&str> = module_code.lines().map(str::trim).collect();
    let par_modules: Vec<usize> = (0..module_lines.len())
        .filter(|index| module_lines[*index].ends_with("mod par;"))
        .collect();
    assert_eq!(
        par_modules.len(),
        1,
        "{MODELS_RUST_MODULE} must declare the module par exactly once"
    );
    let par_module = par_modules[0];
    assert_eq!(
        module_lines[par_module], "pub mod par;",
        "{MODELS_RUST_MODULE} must declare the public module par in its own file"
    );
    assert!(
        par_module == 0 || !module_lines[par_module - 1].starts_with("#["),
        "{MODELS_RUST_MODULE}: an attribute on `pub mod par;` could move the module away from \
         {PAR_SOURCE}"
    );

    assert_eq!(
        TypeId::of::<models::rhoapi::Par>(),
        TypeId::of::<models::rust::par::Par>(),
        "models::rhoapi::Par must be the struct of {PAR_SOURCE}"
    );
}

#[test]
fn every_type_with_two_par_component_vectors_is_listed_with_its_role() {
    let (fixture_declarations, fixture_holders) =
        component_vector_holders(FIXTURE_FILE, FIXTURE_SOURCE);
    assert_eq!(
        fixture_declarations, 3,
        "the fixture declares three types outside comments and string literals"
    );
    assert_eq!(
        unlisted_holders(&fixture_holders),
        [
            "fixture: LegacyParShadow holds vectors of Receive, Send",
            "fixture: TupleShadow holds vectors of Connective, Expr",
        ],
        "the scanner must report exactly the two shadow types of the fixture as unlisted"
    );

    assert_eq!(
        par_message_component_types(),
        PAR_COMPONENT_TYPES,
        "{PAR_SCHEMA}: the repeated fields of message Par are the component types scanned for"
    );

    let root = workspace_root();
    let mut sources_read = 0usize;
    let mut declarations_read = 0usize;
    let mut holders = BTreeMap::new();
    for (scanned_root, minimum_sources) in SCANNED_ROOTS {
        let sources = rust_sources_below(&root.join(scanned_root));
        assert!(
            sources.len() >= *minimum_sources,
            "{scanned_root}: found {} Rust sources, expected at least {minimum_sources}",
            sources.len()
        );
        sources_read += sources.len();
        for source in sources {
            let relative = source
                .strip_prefix(&root)
                .expect("a scanned source is below the workspace root")
                .to_string_lossy()
                .into_owned();
            let text = std::fs::read_to_string(&source)
                .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"));
            let (declared, found) = component_vector_holders(&relative, &text);
            declarations_read += declared;
            holders.extend(found);
        }
    }
    assert!(
        declarations_read >= MINIMUM_DECLARATIONS,
        "the scan found {declarations_read} struct and enum declarations, expected at least \
         {MINIMUM_DECLARATIONS}"
    );
    println!(
        "scanned {sources_read} Rust sources and {declarations_read} struct and enum declarations; \
         {} hold two or more repeated Par component vectors",
        holders.len()
    );

    let unlisted = unlisted_holders(&holders);
    assert!(
        unlisted.is_empty(),
        "a type with two or more repeated Par component vectors has no row in \
         COMPONENT_VECTOR_HOLDERS; it needs a row with its role, and it must not be a channel \
         representation:\n{}",
        unlisted.join("\n")
    );
    let stale: Vec<String> = COMPONENT_VECTOR_HOLDERS
        .iter()
        .filter(|(file, name, _)| !holders.contains_key(&((*file).to_owned(), (*name).to_owned())))
        .map(|(file, name, _)| format!("{file}: {name}"))
        .collect();
    assert!(
        stale.is_empty(),
        "COMPONENT_VECTOR_HOLDERS lists a type that no longer holds two repeated Par component \
         vectors:\n{}",
        stale.join("\n")
    );
    for (file, name, role) in COMPONENT_VECTOR_HOLDERS {
        assert!(!role.trim().is_empty(), "{file}: {name} has no role");
    }
}
