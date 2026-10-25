//! Bag differential corpus over PathMap (test-only).
//!
//! Every Bag operation recorded here reads or rewrites its PathMap storage:
//! endpoint insertion, removal and pruning, prefix removal, scaling through a
//! write cursor, and the merge walks of sum, join, meet and subtract. For each
//! case the manifest records the result's endpoints with their multiplicities
//! and its compact bytes (`Bag::compact_bytes`, PathMap's compact writer). It
//! was blessed on PathMap 0.2.2 before the move to 0.3.4. The default mode
//! compares it with the committed manifest and names every case whose record
//! changed.
//!
//! `BAG_NATIVE_ALGEBRA_CORPUS_BLESS=1 cargo test -p rspace_plus_plus --test
//! bag_native_algebra_corpus` rewrites
//! `tests/fixtures/bag_native_algebra_corpus.txt`.

use std::fmt::Write as _;
use std::path::PathBuf;

use rspace_plus_plus::rspace::bag::{Bag, Multiplicity};

const MANIFEST: &str = "bag_native_algebra_corpus.txt";
const BLESS: &str = "BAG_NATIVE_ALGEBRA_CORPUS_BLESS";

fn multiplicity(count: u64) -> Multiplicity {
    Multiplicity::new(count).expect("corpus multiplicities are positive")
}

fn bag(entries: &[(&[u8], u64)]) -> Bag {
    let mut result = Bag::new();
    for (path, count) in entries {
        result
            .insert_many(path, multiplicity(*count))
            .expect("corpus insertions do not overflow");
    }
    result
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(text, "{byte:02x}").expect("write to String");
    }
    text
}

#[derive(Default)]
struct Corpus {
    manifest: String,
}

impl Corpus {
    fn record(&mut self, case: &str, outcome: Result<Bag, String>) {
        writeln!(self.manifest, "case {case}").expect("write to String");
        match outcome {
            Err(error) => writeln!(self.manifest, "  error {error}").expect("write to String"),
            Ok(bag) => {
                let mut entries = String::new();
                for (path, count) in bag.iter() {
                    write!(entries, " {}x{}", hex(&path), count.get()).expect("write to String");
                }
                writeln!(self.manifest, "  entries{entries}").expect("write to String");
                writeln!(self.manifest, "  compact {}", hex(&bag.compact_bytes()))
                    .expect("write to String");
            }
        }
    }

    fn binary(&mut self, left: &(&str, Bag), right: &(&str, Bag)) {
        let (left_name, left) = left;
        let (right_name, right) = right;
        self.record(
            &format!("sum {left_name} {right_name}"),
            left.sum(right).map_err(|e| format!("{e:?}")),
        );
        self.record(&format!("join {left_name} {right_name}"), Ok(left.join(right)));
        self.record(&format!("meet {left_name} {right_name}"), Ok(left.meet(right)));
        self.record(&format!("subtract {left_name} {right_name}"), Ok(left.subtract(right)));
    }

    fn unary(&mut self, name: &str, bag: &Bag, prefixes: &[&[u8]]) {
        for factor in [0, 1, 3] {
            self.record(
                &format!("scale {name} {factor}"),
                bag.scale(factor).map_err(|e| format!("{e:?}")),
            );
        }
        let paths: Vec<Vec<u8>> = bag.iter().map(|(path, _)| path).collect();
        let absent: &[u8] = &[0xEE];
        for path in paths
            .iter()
            .map(Vec::as_slice)
            .chain(std::iter::once(absent))
        {
            let mut removed = bag.clone();
            let previous = removed.remove(path).map(Multiplicity::get);
            self.record(&format!("remove {name} {} -> {previous:?}", hex(path)), Ok(removed));
            let mut decremented = bag.clone();
            let previous = decremented
                .remove_many(path, multiplicity(1))
                .map(Multiplicity::get);
            self.record(
                &format!("remove_many {name} {} 1 -> {previous:?}", hex(path)),
                Ok(decremented),
            );
            let mut cleared = bag.clone();
            cleared.set_count(path, None);
            self.record(&format!("set_count {name} {} none", hex(path)), Ok(cleared));
            let mut raised = bag.clone();
            raised.set_count(path, Some(multiplicity(4)));
            self.record(&format!("set_count {name} {} 4", hex(path)), Ok(raised));
            let mut inserted = bag.clone();
            let outcome = inserted
                .insert(path)
                .map(|_| inserted)
                .map_err(|e| format!("{e:?}"));
            self.record(&format!("insert {name} {}", hex(path)), outcome);
        }
        for prefix in prefixes {
            let mut removed = bag.clone();
            let (endpoints, cardinality) = removed.remove_prefix(prefix);
            self.record(
                &format!("remove_prefix {name} {} -> {endpoints} {cardinality}", hex(prefix)),
                Ok(removed),
            );
        }
    }
}

fn build_corpus() -> String {
    let wide: Vec<[u8; 1]> = (0u8..12).map(|byte| [byte]).collect();
    let wide_entries: Vec<(&[u8], u64)> = wide
        .iter()
        .map(|path| (path.as_slice(), u64::from(path[0]) + 1))
        .collect();
    let wide_even_entries: Vec<(&[u8], u64)> = wide
        .iter()
        .filter(|path| path[0] % 2 == 0)
        .map(|path| (path.as_slice(), 2))
        .collect();

    let empty = ("{}", Bag::new());
    let prefix_chain = ("{01:1,0100:2,010000:3}", bag(&[(&[1], 1), (&[1, 0], 2), (&[1, 0, 0], 3)]));
    let long_only = ("{010000:5}", bag(&[(&[1, 0, 0], 5)]));
    let middle_only = ("{0100:9}", bag(&[(&[1, 0], 9)]));
    let wide_bag = ("{00..0b:i+1}", bag(&wide_entries));
    let wide_even_bag = ("{00,02..0a:2}", bag(&wide_even_entries));
    let deep = ("{09*12:1,09*6:2}", bag(&[(&[9; 12], 1), (&[9; 6], 2)]));
    let deep_tail = ("{09*12:7}", bag(&[(&[9; 12], 7)]));
    let saturated = ("{01:max}", bag(&[(&[1], u64::MAX)]));
    let prefix_chain_copy =
        ("{01:1,0100:2,010000:3}'", bag(&[(&[1], 1), (&[1, 0], 2), (&[1, 0, 0], 3)]));

    let mut corpus = Corpus::default();
    for (left, right) in [
        (&prefix_chain, &long_only),
        (&long_only, &prefix_chain),
        (&prefix_chain, &middle_only),
        (&middle_only, &prefix_chain),
        (&wide_bag, &wide_even_bag),
        (&wide_even_bag, &wide_bag),
        (&deep, &deep_tail),
        (&deep_tail, &deep),
        (&prefix_chain, &empty),
        (&empty, &prefix_chain),
        (&prefix_chain, &prefix_chain_copy),
        (&saturated, &prefix_chain),
    ] {
        corpus.binary(left, right);
    }
    corpus.unary(prefix_chain.0, &prefix_chain.1, &[&[1], &[1, 0], &[1, 0, 0], &[2]]);
    corpus.unary(wide_bag.0, &wide_bag.1, &[&[0], &[11], &[12]]);
    corpus.unary(deep.0, &deep.1, &[&[9], &[9; 6], &[9; 7], &[9; 12]]);
    corpus.unary(saturated.0, &saturated.1, &[&[1]]);
    corpus.manifest
}

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(MANIFEST)
}

#[test]
fn bag_results_match_the_blessed_pathmap_corpus() {
    let actual = build_corpus();
    assert!(actual == build_corpus(), "the corpus is deterministic in one process");
    let path = manifest_path();
    if std::env::var_os(BLESS).is_some() {
        std::fs::create_dir_all(path.parent().expect("the manifest has a directory"))
            .expect("create the fixtures directory");
        std::fs::write(&path, &actual).expect("write the corpus manifest");
        println!("BLESSED {} ({} bytes)", path.display(), actual.len());
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("missing corpus manifest {}: {error}; bless it with {BLESS}=1", path.display())
    });
    let changed: Vec<&str> = expected
        .split("\ncase ")
        .zip(actual.split("\ncase "))
        .filter(|(expected, actual)| expected != actual)
        .map(|(expected, _)| expected.lines().next().unwrap_or(""))
        .collect();
    assert!(
        expected == actual,
        "{} corpus cases changed (first lines shown; {} expected bytes, {} actual):\n{}",
        changed.len(),
        expected.len(),
        actual.len(),
        changed.join("\n")
    );
}
