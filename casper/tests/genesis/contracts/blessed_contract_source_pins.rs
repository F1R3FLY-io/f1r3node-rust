//! ★★ **Every blessed contract's SIGNED BYTES, pinned — and the pin set is DERIVED, not listed.**
//!
//! Editing a file under `casper/src/main/resources/` moves genesis. This file makes that move
//! announce itself in the same commit that causes it, for **all thirteen** embedded contracts, rather
//! than for the one that happened to be edited when someone last thought of it.
//!
//! # ⚠ Why the RAW SOURCE BYTES are the right pin — they are not a proxy, they are the artifact
//!
//! It is tempting to treat the normalized `Par` as "the real consensus quantity" and the file's text
//! as mere presentation. That is **wrong**, and the code says so:
//!
//! ```text
//!   Registry.rho  ──include_str!──▶  embedded_rho::REGISTRY
//!                                          │
//!                                          ▼   CompiledRholangSource::new(code, …)
//!                                    .code = THE RAW TEXT     .term = normalized Par
//!                                          │
//!                                          ▼   standard_deploys.rs:129-137
//!                                    DeployData { term: compiled_source.code, … }
//!                                          │
//!                          ┌───────────────┴────────────────┐
//!                          ▼                                ▼
//!                 Signed::create(…)                  compute_genesis re-normalizes
//!                 → deploy SIGNATURE                 → post_state_hash
//!                 → block body → block_hash
//! ```
//!
//! `DeployData.term` is the **source text**, so the bytes of these files are signed, gossiped, and
//! stored in the genesis block body. A comment-only edit therefore *does* move the deploy signature
//! and the block hash — it is only the *post-state* that comments cannot reach. Pinning the bytes is
//! consequently the strictest of the three and the one with no false negatives.
//!
//! # ⚠ Why the NORMALIZED TERM is NOT pinned here, and what was measured instead
//!
//! It should be, and it will be — but not in this commit, because the tree cannot currently certify
//! a value. Measured 2026-07-30:
//!
//! | | pinned by `719f2432` | measured now | length |
//! |---|---|---|---|
//! | `NonNegativeNumber.rho` normalized `Par` | `a537547892a0006b…` | `eb17e6a37e7e3ccb…` | **2652 both** |
//!
//! `NonNegativeNumber.rho` is **unmodified** — `git status` shows `Registry.rho` as the only dirty
//! file under `casper/src/main/resources/` — so this drift is not a contract edit. Identical length
//! with a different digest means the *bytes* of the encoded term moved, not its size, which is the
//! signature of a field-level change rather than a structural one. The two candidate causes, neither
//! excluded without a clean-tree build:
//!
//! 1. the `models/` wire-schema work landed since `719f2432` (`959a123a`, `b162440b`, `c709fbfa`,
//!    `0eac9c3a`, `b228545f`, `1eb65221`, `88e492d7`, `bb81b75f`, `d630af54`), which generates the
//!    prost encoding the digest is taken over; and
//! 2. the **uncommitted** normalizer work in `rholang/src/rust/interpreter/compiler/` — 23 files,
//!    735 insertions, of which 25 diff lines touch `locally_free` / `connective_used`, exactly the
//!    `Par` fields that would change bytes without changing length.
//!
//! ⇒ **Re-blessing that digest here would launder an unlanded change through a consensus pin**, so it
//! is deliberately not done. `genesis_overflow_guard_shape.rs`'s single normalized-term cell is left
//! RED so the drift keeps announcing itself, and it is reported to the owner as a finding of its own.
//! When the normalizer work lands, that cell should be re-derived and this file should grow a
//! `NormalizedTerm` column beside the byte pins.
//!
//! # ⚠ And why NOT the genesis post-state hash — MEASURED, and it is the stronger warning
//!
//! `e3a4494b` published a before/after table of `post_state_hash`; `719f2432` retracted it as
//! run-varying. This file adds a sharper measurement of the same defect:
//!
//! | scope | result | source |
//! |---|---|---|
//! | six independent builds, ONE process | **agree** (1 distinct value) | `genesis_vaults_order_determinism::genesis_post_state_hash_is_identical_across_independent_builds`, PASS in 88 s |
//! | four independent nextest PROCESSES | **DISAGREE** — 4 distinct values: `1083c5e0…`, `64088270…`, `a8f6ee11…`, `979b2fbc…` | the `--no-capture` log of `tree_hash_map_delete_restores_never_set`, four cells |
//!
//! Each of those four processes built from `build_genesis_parameters_with_defaults(None, None)`, whose
//! every component is a static key pair or a sorted derivation of one
//! (`casper/tests/util/genesis_builder.rs:166-211`), and `do_build_genesis` consumes nothing but
//! `parameters.clone()`. So the inputs were identical and the outputs were not.
//!
//! ⚠ **"Reproducible within a process, not across processes" is a much narrower target than the
//! earlier finding suggested**, and it rules things out: a `HashMap` iteration order would vary
//! *within* a process too (Rust's `RandomState` re-seeds per map instance), so the surviving suspect
//! is state that is randomised **once per process** and then reused. Root-causing it needs a clean
//! tree and files this work item does not own; it is reported, not fixed. **No genesis post-state
//! hash may be blessed until it is.**

use std::collections::{BTreeMap, BTreeSet};

use casper::rust::genesis::contracts::embedded_rho;
use crypto::rust::hash::blake2b256::Blake2b256;
use rholang::rust::interpreter::compiler::compiler::Compiler;

/// `embedded_rho.rs` as TEXT, so the derived completeness check reads the same file the constants
/// come from and cannot describe a stale version of it.
const EMBEDDED_RHO_SOURCE: &str =
    include_str!("../../../src/rust/genesis/contracts/embedded_rho.rs");

/// One row of the pin table: a blessed contract, and the digest of the bytes genesis signs.
struct Pin {
    /// The `embedded_rho` constant's name — the key the derived completeness check matches on.
    constant: &'static str,
    /// The resource file name, which must equal the tail of the constant's `include_str!` path.
    resource: &'static str,
    source: &'static str,
    /// `blake2b256` of `source.as_bytes()`, hex.
    digest: &'static str,
    /// `source.len()`. A second, independent coordinate: two different texts with one digest would be
    /// a blake2b collision, but a digest transcribed into the wrong row is an ordinary mistake, and
    /// the length catches that.
    length: usize,
}

/// ★ THE PIN TABLE. Its completeness is enforced by
/// [`the_pin_table_covers_exactly_the_embedded_contracts`]: a blessed contract added to
/// `embedded_rho` without a row here FAILS, and a row left behind by a contract that is no longer
/// embedded fails too. That is what makes this a derived set rather than a mirror that can drift.
fn pins() -> Vec<Pin> {
    vec![
        Pin {
            constant: "REGISTRY",
            resource: "Registry.rho",
            source: embedded_rho::REGISTRY,
            digest: "5e9660ca03b041d20b4a6b33e15c89f6885cce5ffbefeb6bfa73d3c2dd0f7726",
            length: 27707,
        },
        Pin {
            constant: "LIST_OPS",
            resource: "ListOps.rho",
            source: embedded_rho::LIST_OPS,
            digest: "ed72b1408de9a48491b3eac532466d5a1ddca8b2103fc71158c41be86fbe9571",
            length: 16922,
        },
        Pin {
            constant: "EITHER",
            resource: "Either.rho",
            source: embedded_rho::EITHER,
            digest: "c3883297da1b71bbb48f086cea518f915c1b08eb01144e115fc5bfb86cf5a151",
            length: 12034,
        },
        Pin {
            constant: "NON_NEGATIVE_NUMBER",
            resource: "NonNegativeNumber.rho",
            source: embedded_rho::NON_NEGATIVE_NUMBER,
            digest: "1df954a04ce64b6a350338653ea1f147dbe5100b95a63831a23a77e4bc33c862",
            length: 4567,
        },
        Pin {
            constant: "MAKE_MINT",
            resource: "MakeMint.rho",
            source: embedded_rho::MAKE_MINT,
            digest: "7ac55f98be1fb6360b3a4a97ec56380d692ffc0bef7058e61c65f400e750a5d3",
            length: 11187,
        },
        Pin {
            constant: "AUTH_KEY",
            resource: "AuthKey.rho",
            source: embedded_rho::AUTH_KEY,
            digest: "155f6db44ae9a2aa98c66227d5b3ae79508797e88ef497fe1eaadb838bbbf565",
            length: 4439,
        },
        Pin {
            constant: "SYSTEM_VAULT",
            resource: "SystemVault.rho",
            source: embedded_rho::SYSTEM_VAULT,
            digest: "5767bf0cb35a70c4e776ae0554a42fc17ff219612c91e08fba47bec30c2ece3b",
            length: 15023,
        },
        Pin {
            constant: "MULTI_SIG_SYSTEM_VAULT",
            resource: "MultiSigSystemVault.rho",
            source: embedded_rho::MULTI_SIG_SYSTEM_VAULT,
            digest: "f114b243acd7524f21eed93fe0046943763f1a88dabb6da4613a2dd5f86a8b1b",
            length: 14594,
        },
        Pin {
            constant: "STACK",
            resource: "Stack.rho",
            source: embedded_rho::STACK,
            digest: "1840b1204bd1e06393c5db659ee79fa0802d5839cc5ec591254347960f4d2649",
            length: 3866,
        },
        Pin {
            constant: "TOKEN_METADATA",
            resource: "TokenMetadata.rhox",
            source: embedded_rho::TOKEN_METADATA,
            digest: "7a7f3ca311fcfd4d5e51c772e079900a9c25502aabe6bd67ade073922ed8af4c",
            length: 2915,
        },
        Pin {
            constant: "POS",
            resource: "PoS.rhox",
            source: embedded_rho::POS,
            digest: "60dd2ac79373406065ef58731d2859e41cd2cffcc94181254b8f40e43086827e",
            length: 94081,
        },
        Pin {
            constant: "CAPABILITIES_REGISTRY",
            resource: "CapabilitiesRegistry.rhox",
            source: embedded_rho::CAPABILITIES_REGISTRY,
            digest: "1577390b7abb4e61afc8558683da5efa230e30f41a5439167a4471e3bfbdab69",
            length: 9769,
        },
        Pin {
            constant: "EXCHANGE",
            resource: "Exchange.rhox",
            source: embedded_rho::EXCHANGE,
            digest: "d367e84f3c7b46b3d8c373ca9370a8344bec1332b99e4b345ae8d44b47b10a53",
            length: 5423,
        },
    ]
}

/// Every `pub const NAME: &str = … include_str!("…/Resource.rho…")` declared in `embedded_rho.rs`,
/// mapped to the resource file its path ends in.
///
/// Deliberately a hand-rolled scan and not a new regex dependency: the shape is fixed by the module's
/// own one-constant-per-resource convention, and
/// [`the_scan_of_embedded_rho_finds_the_constants_and_invents_none`] is its non-vacuity floor.
fn declared_constants(source: &str) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    // `split` on the declaration keyword bounds each constant's text by the next declaration, so a
    // constant whose initializer is missing cannot borrow the following one's path.
    for statement in source.split("pub const ").skip(1) {
        let Some(name_end) = statement.find(':') else {
            continue;
        };
        let name = statement[..name_end].trim();
        let Some(macro_at) = statement.find("include_str!(\"") else {
            continue;
        };
        let path_start = macro_at + "include_str!(\"".len();
        let Some(path_len) = statement[path_start..].find('"') else {
            continue;
        };
        let path = &statement[path_start..path_start + path_len];
        let resource = path.rsplit('/').next().unwrap_or(path);
        found.insert(name.to_string(), resource.to_string());
    }
    found
}

fn measure(pin: &Pin) -> (String, usize) {
    let bytes = pin.source.as_bytes();
    (hex::encode(Blake2b256::hash(bytes.to_vec())), bytes.len())
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// The floors — each closes a way the pin could pass for the wrong reason
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// ★ **FLOOR for the scanner.** A scan that found nothing would make the completeness check below
/// satisfiable by an empty table; a scan that globbed the resource directory would demand pins for
/// files genesis never deploys.
#[test]
fn the_scan_of_embedded_rho_finds_the_constants_and_invents_none() {
    let declared = declared_constants(EMBEDDED_RHO_SOURCE);

    assert!(
        declared.len() >= 13,
        "★ FLOOR: `embedded_rho.rs` declares at least thirteen blessed-contract constants; the \
         scanner found {}. Found: {declared:#?}",
        declared.len(),
    );
    assert_eq!(
        declared.get("NON_NEGATIVE_NUMBER").map(String::as_str),
        Some("NonNegativeNumber.rho"),
        "★ FLOOR: the scanner must pair a constant with the RESOURCE its `include_str!` names, or \
         the completeness check below compares names against nothing. Found: {declared:#?}",
    );
    assert_eq!(
        declared.get("POS").map(String::as_str),
        Some("PoS.rhox"),
        "★ FLOOR: `.rhox` templates must be found too — they are deployed after substitution and \
         are every bit as blessed. Found: {declared:#?}",
    );

    // ★ The negative control. `match_example.rho` and `RegistryRealLifeTest.rho` sit in
    // `main/resources` but are NOT embedded; a scanner that read the directory instead of
    // `embedded_rho.rs` would pick them up and this file would demand pins for non-genesis files.
    let resources: BTreeSet<&str> = declared.values().map(String::as_str).collect();
    for not_blessed in ["match_example.rho", "RegistryRealLifeTest.rho"] {
        assert!(
            !resources.contains(not_blessed),
            "★ FLOOR: {not_blessed} lives in `main/resources` but genesis does not deploy it. The \
             scanner must read `embedded_rho.rs`, not the directory. Found: {declared:#?}",
        );
    }
}

/// ★★ **The pin table names EXACTLY the embedded blessed contracts.**
///
/// This is the cell that makes the set derived. Watched RED by deleting the `EXCHANGE` row: it names
/// the missing constant rather than merely reporting a count mismatch.
#[test]
fn the_pin_table_covers_exactly_the_embedded_contracts() {
    let declared = declared_constants(EMBEDDED_RHO_SOURCE);
    let pins = pins();

    let pinned: BTreeMap<String, String> = pins
        .iter()
        .map(|p| (p.constant.to_string(), p.resource.to_string()))
        .collect();

    let missing: Vec<&String> = declared.keys().filter(|k| !pinned.contains_key(*k)).collect();
    let stale: Vec<&String> = pinned.keys().filter(|k| !declared.contains_key(*k)).collect();

    assert!(
        missing.is_empty() && stale.is_empty(),
        "★★ the pin table and `embedded_rho.rs` disagree about WHICH contracts genesis deploys.\n\
         embedded but NOT pinned: {missing:?}\n\
         pinned but NOT embedded: {stale:?}\n\
         ⇒ every blessed contract's signed bytes must be pinned, or editing one moves the genesis \
         deploy signature and the block hash with nothing to announce it.",
    );

    // ...and each row must name the resource its own constant includes, so a row cannot be pinned
    // against a different file's bytes while still looking complete.
    assert_eq!(
        pinned, declared,
        "★★ a pin row names a different resource than its constant's `include_str!` does",
    );
}

/// ★★ **FLOOR: every blessed `.rho` still normalizes.**
///
/// A blessed contract that will not compile is a genesis *failure*, not a hash change, and it would
/// otherwise be invisible to a byte pin — the bytes would match perfectly right up to the ceremony.
///
/// ⚠ The converse is deliberately NOT asserted. The obvious-looking claim "a `$$macro$$` template
/// cannot normalize until substitution" was written here first and **measured false**:
/// `CapabilitiesRegistry.rhox` normalizes as-is. Its holes sit where the grammar accepts the
/// unsubstituted text, so the template parses and means something — something other than what it will
/// mean after substitution. The `.rhox` status is therefore *reported*, not asserted, and the reader
/// is told which templates are in which state.
#[test]
fn every_blessed_rho_normalizes_and_the_rhox_status_is_reported() {
    let mut broken: Vec<String> = Vec::new();
    let mut template_status: Vec<String> = Vec::new();

    for pin in pins() {
        let outcome = Compiler::source_to_adt(pin.source);
        match (pin.resource.ends_with(".rhox"), outcome) {
            (false, Err(e)) => broken.push(format!("{} does NOT normalize: {e:?}", pin.resource)),
            (false, Ok(_)) => {}
            (true, Ok(_)) => template_status
                .push(format!("  {} normalizes UNSUBSTITUTED", pin.resource)),
            (true, Err(_)) => template_status.push(format!(
                "  {} requires substitution before it normalizes",
                pin.resource
            )),
        }
    }

    assert!(
        broken.is_empty(),
        "★★ a blessed `.rho` does not normalize — genesis would FAIL, not merely change hash: \
         {broken:#?}",
    );

    // Reported, not asserted: the split was measured and is not what it was assumed to be.
    println!("★ `.rhox` template normalization status:\n{}", template_status.join("\n"));
    assert!(
        !template_status.is_empty(),
        "★ FLOOR: there must be `.rhox` templates to report on, or this cell says nothing",
    );
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// The pin
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// ★★ **Every blessed contract's signed bytes still match.**
///
/// Reports **all** drift in one run with a ready-to-paste table, rather than stopping at the first
/// row. That distinction carries a diagnosis: one row moved means a contract was edited; every row
/// moving at once would mean something changed how the bytes are read.
///
/// ⚠ **This cell is EXPECTED to go red on any blessed-contract edit — that is its job.** Paste the
/// printed `digest` / `length` lines into the matching row IN THE SAME COMMIT that edits the
/// contract, and file the `docs/consensus/consensus-change-register.md` entry that such a change
/// owes.
#[test]
fn every_blessed_contract_source_is_pinned() {
    let mut drifted: Vec<String> = Vec::new();

    for pin in pins() {
        let (digest, length) = measure(&pin);
        if (digest.as_str(), length) != (pin.digest, pin.length) {
            drifted.push(format!(
                "  {} ({})\n    was    {} / {}\n    is     {} / {}\n\
                 \n            digest: \"{}\",\n            length: {},\n",
                pin.constant, pin.resource, pin.digest, pin.length, digest, length, digest, length,
            ));
        }
    }

    assert!(
        drifted.is_empty(),
        "★★ {} blessed contract source(s) CHANGED. `DeployData.term` IS this text \
         (`standard_deploys.rs:129-137`), so the genesis deploy signature and the block hash moved, \
         and if the change reaches the term the post-state moved too.\n\
         That is a consensus-visible change and it owes a \
         `docs/consensus/consensus-change-register.md` entry.\n\n{}",
        drifted.len(),
        drifted.join("\n"),
    );
}
