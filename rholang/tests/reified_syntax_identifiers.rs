// P3b test of the identifier rule of plan decision 27.
//
// A param key or a theory name that a decoder accepts must be one that the frontend can
// produce. `models::rust::reified::validation` holds the two word lists, measured on parser
// `6548aec`. This test reads every word-shaped string token of the `grammar.js` of the parser
// that this build uses. For each word `w`, the v3 parser must read `x!(d; w=1)` as a send whose
// only param has the key `w` exactly when `is_param_key(w)` holds, and must read `free w()` as
// a reference to the theory `w` exactly when `is_theory_name(w)` holds. The builtin theories
// `Nat`, `Int`, `String`, `Bool` and `Any` must be accepted.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use models::rust::reified::encoding::is_identifier;
use models::rust::reified::validation::{
    is_param_key, is_theory_name, REJECTED_PARAM_KEYS, REJECTED_THEORY_NAMES,
};
use rholang_parser::ast::{AnnProc, Hyperparam, Proc};
use rholang_parser::RholangParser;
use serde_json::Value;
use validated::Validated;

const PARSER_PACKAGE: &str = "rholang-parser";
const BUILTIN_THEORIES: [&str; 5] = ["Nat", "Int", "String", "Bool", "Any"];
/// The 32 keywords of grammar `02cef80` and the two contextual keywords of the v3 grammar.
const KEYWORDS: [&str; 34] = [
    "Bool",
    "ByteArray",
    "Int",
    "Nil",
    "Set",
    "String",
    "Uri",
    "agent",
    "and",
    "bundle",
    "bundle0",
    "catch",
    "constructor",
    "contract",
    "default",
    "else",
    "false",
    "for",
    "free",
    "if",
    "in",
    "let",
    "match",
    "matches",
    "method",
    "new",
    "not",
    "or",
    "private",
    "select",
    "true",
    "try",
    "use",
    "where",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the rholang crate has a workspace parent")
        .to_path_buf()
}

/// The `grammar.js` of the parser that this build uses. Cargo names the manifest of the
/// `rholang-parser` package, a path under the local `[patch]` or a git checkout after the
/// final pin, and the grammar lies in the sibling `rholang-tree-sitter` crate.
fn grammar_path() -> PathBuf {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--offline", "--locked"])
        .current_dir(workspace_root())
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata prints JSON");
    let manifests: Vec<&str> = metadata["packages"]
        .as_array()
        .expect("cargo metadata lists the packages")
        .iter()
        .filter(|package| package["name"] == PARSER_PACKAGE)
        .filter_map(|package| package["manifest_path"].as_str())
        .collect();
    let manifest = match manifests.as_slice() {
        [manifest] => Path::new(manifest),
        other => panic!("expected one {PARSER_PACKAGE} package, found {other:?}"),
    };
    manifest
        .parent()
        .and_then(Path::parent)
        .expect("the parser crate lies in a repository directory")
        .join("rholang-tree-sitter")
        .join("grammar.js")
}

/// The index after the regular expression literal whose body starts at `index`, flags included.
fn skip_regex(chars: &[char], mut index: usize) -> usize {
    let mut in_class = false;
    loop {
        match chars.get(index) {
            Some('\\') => index += 2,
            Some('[') => {
                in_class = true;
                index += 1;
            }
            Some(']') => {
                in_class = false;
                index += 1;
            }
            Some('/') if !in_class => break,
            Some(_) => index += 1,
            None => panic!("grammar.js holds an unterminated regular expression"),
        }
    }
    index += 1;
    while chars.get(index).is_some_and(char::is_ascii_alphabetic) {
        index += 1;
    }
    index
}

/// The contents of every string literal of the JavaScript source `source`. Comments and
/// regular expression literals are skipped. `grammar.js` has no division, so every other `/`
/// starts a regular expression literal.
fn js_string_literals(source: &str) -> Vec<String> {
    let chars: Vec<char> = source.chars().collect();
    let mut literals = Vec::new();
    let mut index = 0;
    while let Some(&current) = chars.get(index) {
        match (current, chars.get(index + 1).copied()) {
            ('/', Some('/')) => {
                while chars.get(index).is_some_and(|character| *character != '\n') {
                    index += 1;
                }
            }
            ('/', Some('*')) => {
                index += 2;
                while !(chars.get(index) == Some(&'*') && chars.get(index + 1) == Some(&'/')) {
                    assert!(
                        index < chars.len(),
                        "grammar.js holds an unterminated block comment"
                    );
                    index += 1;
                }
                index += 2;
            }
            ('/', _) => index = skip_regex(&chars, index + 1),
            (quote @ ('\'' | '"' | '`'), _) => {
                let mut literal = String::new();
                index += 1;
                loop {
                    match chars.get(index) {
                        Some('\\') => {
                            literal.extend(chars.get(index + 1));
                            index += 2;
                        }
                        Some(character) if *character == quote => break,
                        Some(character) => {
                            literal.push(*character);
                            index += 1;
                        }
                        None => panic!("grammar.js holds an unterminated string literal"),
                    }
                }
                index += 1;
                literals.push(literal);
            }
            _ => index += 1,
        }
    }
    literals
}

/// True when the parser reads `x!(d; word=1)` as a send whose only param has the key `word`.
fn parses_as_param_key(word: &str) -> bool {
    let source = format!("x!(d; {word}=1)");
    let parser = RholangParser::new();
    let parsed = parser.parse(&source);
    match &parsed {
        Validated::Good(procs) => match procs.as_slice() {
            [AnnProc {
                proc:
                    Proc::Send {
                        params: Some(params),
                        ..
                    },
                ..
            }] => match params.as_slice() {
                [Hyperparam::Named { key, value }] => {
                    key.name == word && matches!(value.proc, Proc::LongLiteral(1))
                }
                _ => false,
            },
            _ => false,
        },
        Validated::Fail(_) => false,
    }
}

/// True when the parser reads `free word()` as a reference to the theory `word`.
fn parses_as_theory_name(word: &str) -> bool {
    let source = format!("free {word}()");
    let parser = RholangParser::new();
    let parsed = parser.parse(&source);
    match &parsed {
        Validated::Good(procs) => match procs.as_slice() {
            [AnnProc {
                proc: Proc::FreeTheory { name, args },
                ..
            }] => name.name == word && args.is_empty(),
            _ => false,
        },
        Validated::Fail(_) => false,
    }
}

#[test]
fn reified_grammar_words_parse_exactly_as_the_identifier_validators_accept() {
    let path = grammar_path();
    let grammar = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let words: BTreeSet<String> = js_string_literals(&grammar)
        .into_iter()
        .filter(|literal| is_identifier(literal))
        .collect();
    for word in KEYWORDS
        .iter()
        .chain(&REJECTED_PARAM_KEYS)
        .chain(&REJECTED_THEORY_NAMES)
    {
        assert!(
            words.contains(*word),
            "{word} is not a word-shaped string token of {}",
            path.display()
        );
    }
    let mut mismatches = Vec::new();
    for word in words.iter().map(String::as_str).chain(BUILTIN_THEORIES) {
        let (parsed_key, valid_key) = (parses_as_param_key(word), is_param_key(word));
        if parsed_key != valid_key {
            mismatches.push(format!(
                "x!(d; {word}=1): the parser reads the key {parsed_key}, is_param_key says {valid_key}"
            ));
        }
        let (parsed_theory, valid_theory) = (parses_as_theory_name(word), is_theory_name(word));
        if parsed_theory != valid_theory {
            mismatches.push(format!(
                "free {word}(): the parser reads the theory {parsed_theory}, is_theory_name says {valid_theory}"
            ));
        }
    }
    for builtin in BUILTIN_THEORIES {
        assert!(
            parses_as_theory_name(builtin) && is_theory_name(builtin),
            "the builtin theory {builtin} is not accepted"
        );
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} grammar words disagree with the validators:\n{}",
        mismatches.len(),
        words.len(),
        mismatches.join("\n")
    );
}
