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
//! # ★★ The NORMALIZED TERM is now pinned too — and the drift that blocked it is ATTRIBUTED
//!
//! An earlier revision of this module declined to pin the normalized `Par`, because
//! `NonNegativeNumber.rho`'s pinned digest had moved (`a537547892a0…` → `eb17e6a37e7e…`) at
//! **identical length, 2652 both**, with the contract file unmodified — and the tree could not then
//! certify which of three changes had moved it. Re-blessing under that uncertainty would have
//! laundered possibly-unlanded work through a consensus pin. That refusal was right, and it is now
//! **discharged by measurement rather than by waiting**.
//!
//! Identical length with a different digest is a **field-level value change in a fixed-width
//! encoding**. Three candidates carried that signature; the experiment separated all three.
//!
//! ```text
//!    git archive <ref> | tar -x  ⇒  clean tree      ⚠ + restore the held-local root [patch],
//!             │                                        parser paths rewritten to ABSOLUTE, or
//!             ▼                                        the export DOES NOT COMPILE and the
//!    blake2b256(source_to_adt(src).encode_to_vec())     baseline measures nothing
//!
//!    084c93b5^  ──▶  a537547892a0…  2652   ═══ the OLD pin, reproduced exactly
//!    084c93b5   ──▶  eb17e6a37e7e…  2652   ═══ the NEW value, reproduced exactly
//!    HEAD clean ──▶  eb17e6a37e7e…  2652
//!    HEAD dirty ──▶  eb17e6a37e7e…  2652   ═══ 51 dirty files change NOTHING
//! ```
//!
//! | candidate | verdict | the evidence that settles it |
//! |---|---|---|
//! | `084c93b5` — `util::filter_and_adjust_bitset` emitted the shifted *position* where a one-byte-per-index bitset requires the *suffix*, so every non-empty `locally_free` carried wrong VALUES at the right LENGTH | ★ **THE CAUSE** | the digest moves *exactly* across this one commit |
//! | the landed `models/` schema-codegen work — five commits touching `models/codegen/schema.rs`, `models/build.rs`, `drive.rs`, `sort_drive.rs` before it, and `9560a068` / `87ee699c` after | **EXCLUDED** | `084c93b5^` reproduces the OLD pin and `084c93b5` the NEW value, so no `models/` commit on either side is visible in this term |
//! | the then-uncommitted normalizer work in `rholang/src/rust/interpreter/compiler/` — 23 files, +735 lines, 25 of them touching `locally_free` / `connective_used` | **EXCLUDED TWICE** | the clean `HEAD` export equals the dirty tree byte for byte; and independently, 17 of the 23 files are **token-identical** to `HEAD` (pure `rustfmt` reflow), among them all four files that carry those 25 lines, while the other 6 differ only by brace-vs-expression forms and one `use` reorder |
//!
//! ⇒ the whole delta traces to **landed commits**, so the pins below encode no phantom work.
//!
//! ★★ **And the reach was ELEVEN, not one — which is why this file, not a single cell, is where the
//! normalized terms belong.** `084c93b5` changes what every binder emits for the indices its body
//! names but does not own, so it moves every blessed contract that has one. Measured across all
//! thirteen embedded contracts at `084c93b5^` → `084c93b5`: **11 of the 11 that normalize moved
//! their digest, and every one of the 11 at IDENTICAL length.** A one-contract guard would have
//! announced 1 of 11 consensus-visible moves and stayed green on the other 10.
//!
//! The one row whose *length* also moved across `084c93b5^` → `HEAD` is `REGISTRY`
//! (28068 → 28455), and that is the control: `7c0cfd0a` edited `Registry.rho` itself. A source edit
//! moves the length; a field-value change inside the encoder cannot. The two kinds of change are
//! therefore distinguishable in the table below without consulting anything else.
//!
//! # The genesis post-state instability — measured, attributed, and closed
//!
//! `e3a4494b` published a before/after table of `post_state_hash`; `719f2432` retracted it as
//! run-varying. This file adds a sharper measurement of the same defect:
//!
//! | process | RSpace scope | result | source |
//! |---|---|---|---|
//! | one | ONE, shared | **agree** (1 distinct value) | `genesis_vaults_order_determinism::genesis_post_state_hash_is_identical_across_independent_builds`, PASS in 88 s |
//! | one | SIX, one per build | **agree** (1 distinct value) | `…::genesis_post_state_hash_is_identical_across_independent_rspace_scopes`, PASS in 76.63 s |
//! | FOUR, before the fixture repair | four | **DISAGREE** — 4 distinct values: `1083c5e0…`, `64088270…`, `a8f6ee11…`, `979b2fbc…` | the `--no-capture` log of `tree_hash_map_delete_restores_never_set`, four cells |
//! | FOUR, after the fixture repair | four | **agree** — `28ca4bcf56ec1987…20a925ca` in every process | `default_genesis_post_state_hash_is_pinned_across_processes`, 4/4 independent processes, 14.58–14.82 s each |
//!
//! The historical premise that those four processes had identical inputs was false in one precise
//! place. `build_genesis_parameters_with_defaults(None, None)` read two `lazy_static!` key cohorts
//! initialized with `Secp256k1::new_key_pair()`: four default validator pairs and the extra funded
//! vault pairs. They were stable within a process and different between processes — exactly the
//! measured signature — and both cohorts reach the `Genesis` value. The sibling builder under
//! `casper/src/rust/test_utils/` also generated its extra funded-vault keys per call.
//!
//! ★★ **The first two rows are the ATTRIBUTION, and the middle one had to be added to get it.** The
//! original pair of rows differed in **two** variables at once — process *and* scope — so it could
//! not say which mattered. Rows 1 and 2 now differ in the scope alone and agree; rows 2 and 3 differ
//! in the process alone and disagree. ⇒ **the varying state is randomised ONCE PER PROCESS and then
//! reused; it is not derived from the RSpace scope, the store manager, or the `RuntimeManager`.**
//!
//! That also rules two things out on the record. `evaluate_with_term` seeds from
//! `create_from_length(128)` = `rand::thread_rng().fill(…)` (`blake2b512_random.rs:91`), i.e. fresh
//! entropy per call, which would have made the same-process builds disagree — they agree, so
//! **per-call randomness is refuted**, and #171's S3 (per-deploy RSpace event-log order) is refuted
//! with it, now including the case of a fresh store per build. A `HashMap` iteration order is
//! likewise excluded: Rust's `RandomState` re-keys per map instance, so it would vary *within* a
//! process too.
//!
//! The two builders now share one domain-separated deterministic test keyspace; the explicitly
//! random API remains random. `default_genesis_uses_the_shared_deterministic_keyspace` pins both
//! cohorts' reach into `Genesis`, and the four-process golden above proves the post-state closure.
//! The former prohibition on blessing a genesis post-state hash is therefore discharged.

use std::collections::{BTreeMap, BTreeSet};

use casper::rust::genesis::contracts::embedded_rho;
use crypto::rust::hash::blake2b256::Blake2b256;
use prost::Message;
use rholang::rust::interpreter::compiler::compiler::Compiler;

/// `embedded_rho.rs` as TEXT, so the derived completeness check reads the same file the constants
/// come from and cannot describe a stale version of it.
const EMBEDDED_RHO_SOURCE: &str =
    include_str!("../../../src/rust/genesis/contracts/embedded_rho.rs");

/// The NORMALIZED-TERM coordinates of a blessed contract.
///
/// One struct rather than two `Option` fields, so "digest pinned but length not" is **unspellable**
/// instead of merely wrong: the two coordinates are always both present or both absent.
struct NormalizedPin {
    /// `blake2b256` of `Compiler::source_to_adt(source).encode_to_vec()`, hex.
    digest: &'static str,
    /// That encoding's length. It is the coordinate that **discriminates the kind of change**: a
    /// source edit moves the length (see `REGISTRY` at `7c0cfd0a`, 28068 → 28455), whereas a
    /// field-level value change inside the encoder cannot (see all eleven rows at `084c93b5`).
    length: usize,
}

/// One row of the pin table: a blessed contract, the digest of the bytes genesis signs, and the
/// digest of the term those bytes normalize to.
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
    /// The normalized `Par`, or `None` for a source that does not normalize UNSUBSTITUTED.
    ///
    /// ⚠ `None` is not a free choice and not a place to hide a row. Which contracts are in which
    /// state is asserted against reality by [`every_blessed_normalized_term_is_pinned`]: a template
    /// that *starts* normalizing fails there until it is measured and pinned, and a `.rho` that
    /// *stops* normalizing fails there too. `None` is therefore a **typed, checked exception**,
    /// not an omission.
    normalized: Option<NormalizedPin>,
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
            normalized: Some(NormalizedPin {
                digest: "6117f1e344275d7047e1c32cf32621136edd949c71f4b69efafa3595f40d88bb",
                length: 28455,
            }),
        },
        Pin {
            constant: "LIST_OPS",
            resource: "ListOps.rho",
            source: embedded_rho::LIST_OPS,
            digest: "ed72b1408de9a48491b3eac532466d5a1ddca8b2103fc71158c41be86fbe9571",
            length: 16922,
            normalized: Some(NormalizedPin {
                digest: "7a4b5c1352b437101c69e282e2c936c31130c6f12fb9a74ba9e52fb160dfd334",
                length: 18349,
            }),
        },
        Pin {
            constant: "EITHER",
            resource: "Either.rho",
            source: embedded_rho::EITHER,
            digest: "c3883297da1b71bbb48f086cea518f915c1b08eb01144e115fc5bfb86cf5a151",
            length: 12034,
            normalized: Some(NormalizedPin {
                digest: "9f8ba7518d578d0558063e2c6a4288f4b9a49b80f299a2a38be2efb2c0b0e248",
                length: 11048,
            }),
        },
        Pin {
            constant: "NON_NEGATIVE_NUMBER",
            resource: "NonNegativeNumber.rho",
            source: embedded_rho::NON_NEGATIVE_NUMBER,
            digest: "1df954a04ce64b6a350338653ea1f147dbe5100b95a63831a23a77e4bc33c862",
            length: 4567,
            normalized: Some(NormalizedPin {
                digest: "eb17e6a37e7e3ccb54e0a142c05aaab638a0bb6cdc37f263d72722acb2889a92",
                length: 2652,
            }),
        },
        Pin {
            constant: "MAKE_MINT",
            resource: "MakeMint.rho",
            source: embedded_rho::MAKE_MINT,
            digest: "5d09c9d391762ce17191b6a4c4aa612d4d2303f91455068251e8c7928ff7520d",
            length: 11240,
            normalized: Some(NormalizedPin {
                digest: "1ac9438fd9e10cdd04857676212dadae194dc0839aaafe5631d0c4167a752070",
                length: 7347,
            }),
        },
        Pin {
            constant: "AUTH_KEY",
            resource: "AuthKey.rho",
            source: embedded_rho::AUTH_KEY,
            digest: "155f6db44ae9a2aa98c66227d5b3ae79508797e88ef497fe1eaadb838bbbf565",
            length: 4439,
            normalized: Some(NormalizedPin {
                digest: "5a0f1bcfb601ac2b2a7d56d6e0a20d965a034ad1fc601f0bd1535550882dcbc6",
                length: 1200,
            }),
        },
        Pin {
            constant: "SYSTEM_VAULT",
            resource: "SystemVault.rho",
            source: embedded_rho::SYSTEM_VAULT,
            digest: "5767bf0cb35a70c4e776ae0554a42fc17ff219612c91e08fba47bec30c2ece3b",
            length: 15023,
            normalized: Some(NormalizedPin {
                digest: "7bb242be6a4c95a8467898a4abe1947c200b6cd6b834c604a4ed25eec33951a7",
                length: 13102,
            }),
        },
        Pin {
            constant: "MULTI_SIG_SYSTEM_VAULT",
            resource: "MultiSigSystemVault.rho",
            source: embedded_rho::MULTI_SIG_SYSTEM_VAULT,
            digest: "f114b243acd7524f21eed93fe0046943763f1a88dabb6da4613a2dd5f86a8b1b",
            length: 14594,
            normalized: Some(NormalizedPin {
                digest: "660e9fe12788d10e8712c38af0b94df1c5bb75471980c07c913c507df58785b8",
                length: 14299,
            }),
        },
        Pin {
            constant: "STACK",
            resource: "Stack.rho",
            source: embedded_rho::STACK,
            digest: "1840b1204bd1e06393c5db659ee79fa0802d5839cc5ec591254347960f4d2649",
            length: 3866,
            normalized: Some(NormalizedPin {
                digest: "a99f1355d6961509f9d97d78b977c66863b7f5e5174166ec82dbd06be0e6ae2a",
                length: 3686,
            }),
        },
        Pin {
            constant: "TOKEN_METADATA",
            resource: "TokenMetadata.rhox",
            source: embedded_rho::TOKEN_METADATA,
            digest: "7a7f3ca311fcfd4d5e51c772e079900a9c25502aabe6bd67ade073922ed8af4c",
            length: 2915,
            // `$$macro$$` holes land where the grammar rejects them, so this template does
            // not normalize until substitution. Checked, not assumed —
            // `every_blessed_normalized_term_is_pinned` fails if it starts normalizing.
            normalized: None,
        },
        Pin {
            constant: "POS",
            resource: "PoS.rhox",
            source: embedded_rho::POS,
            digest: "60dd2ac79373406065ef58731d2859e41cd2cffcc94181254b8f40e43086827e",
            length: 94081,
            // `$$macro$$` holes land where the grammar rejects them, so this template does
            // not normalize until substitution. Checked, not assumed —
            // `every_blessed_normalized_term_is_pinned` fails if it starts normalizing.
            normalized: None,
        },
        Pin {
            constant: "CAPABILITIES_REGISTRY",
            resource: "CapabilitiesRegistry.rhox",
            source: embedded_rho::CAPABILITIES_REGISTRY,
            digest: "1577390b7abb4e61afc8558683da5efa230e30f41a5439167a4471e3bfbdab69",
            length: 9769,
            normalized: Some(NormalizedPin {
                digest: "9d7a65b8e0bc93f1d917fd508ff692da3acd50de92ccf15196b305403b68f3bf",
                length: 6246,
            }),
        },
        Pin {
            constant: "EXCHANGE",
            resource: "Exchange.rhox",
            source: embedded_rho::EXCHANGE,
            digest: "d367e84f3c7b46b3d8c373ca9370a8344bec1332b99e4b345ae8d44b47b10a53",
            length: 5423,
            normalized: Some(NormalizedPin {
                digest: "c1e843a86fa8f9b49641bdfc6fa4a39609b967c84c1da0c0db4f4900b72e335a",
                length: 521,
            }),
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

    let missing: Vec<&String> = declared
        .keys()
        .filter(|k| !pinned.contains_key(*k))
        .collect();
    let stale: Vec<&String> = pinned
        .keys()
        .filter(|k| !declared.contains_key(*k))
        .collect();

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
            (true, Ok(_)) => {
                template_status.push(format!("  {} normalizes UNSUBSTITUTED", pin.resource))
            }
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
    println!(
        "★ `.rhox` template normalization status:\n{}",
        template_status.join("\n")
    );
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

/// ★★ **Every blessed contract's NORMALIZED TERM still matches — all eleven that have one.**
///
/// The companion to [`every_blessed_contract_source_is_pinned`], and it catches a strictly different
/// class of change. The source pin catches edits to the *text*; this catches changes to what the
/// text *means* — a normalizer or encoder change that moves the term while every byte of every
/// resource file stays put. Neither implies the other:
///
/// | change | source pin | this pin |
/// |---|---|---|
/// | a comment added to `Either.rho` | RED (the text is signed) | green (comments do not survive normalization) |
/// | `filter_and_adjust_bitset` fixed (`084c93b5`) | green (no file touched) | **RED for 11 of 11** |
/// | `Registry.rho`'s updater fixed (`7c0cfd0a`) | RED | RED, *and the length moves too* |
///
/// ⚠ **The third row is the one that motivated this cell.** `084c93b5` moved eleven blessed
/// contracts' normalized terms at identical length while every source pin stayed green, and the only
/// instrument that noticed was a single hand-written cell for `NonNegativeNumber.rho`. Ten
/// consensus-visible moves went unannounced. The set is derived here from the same `pins()` table
/// whose completeness `the_pin_table_covers_exactly_the_embedded_contracts` already enforces against
/// `embedded_rho.rs`, so a contract cannot be added to genesis without acquiring a normalized pin —
/// the gap cannot reopen by omission.
///
/// ⚠ **`None` is CHECKED, not trusted.** A row may decline a normalized pin only by actually failing
/// to normalize. If a `$$macro$$` template starts normalizing unsubstituted, or a `.rho` stops, this
/// cell fails and says which — so `None` cannot be used to silence a row.
#[test]
fn every_blessed_normalized_term_is_pinned() {
    /// The floor: how many rows must carry a normalized pin. Eleven of the thirteen embedded
    /// contracts normalize unsubstituted (measured), so a run in which fewer are *pinned* means
    /// rows were quietly demoted to `None` and this cell has stopped guarding them.
    const MIN_PINNED: usize = 11;

    let mut drifted: Vec<String> = Vec::new();
    let mut status_changed: Vec<String> = Vec::new();
    let mut pinned = 0usize;

    for pin in pins() {
        match (Compiler::source_to_adt(pin.source), &pin.normalized) {
            (Ok(par), Some(expected)) => {
                pinned += 1;
                let bytes = par.encode_to_vec();
                let digest = hex::encode(Blake2b256::hash(bytes.clone()));
                if (digest.as_str(), bytes.len()) != (expected.digest, expected.length) {
                    drifted.push(format!(
                        "  {} ({})\n    was    {} / {}\n    is     {} / {}\n\
                         \n            normalized: Some(NormalizedPin {{\n\
                         \x20               digest: \"{}\",\n\
                         \x20               length: {},\n\
                         \x20           }}),\n",
                        pin.constant,
                        pin.resource,
                        expected.digest,
                        expected.length,
                        digest,
                        bytes.len(),
                        digest,
                        bytes.len(),
                    ));
                }
            }
            // A template that has started normalizing is a real event: it now HAS a consensus-visible
            // term, and nothing is watching it.
            (Ok(par), None) => status_changed.push(format!(
                "  {} now NORMALIZES unsubstituted, so it HAS a consensus-visible term and carries \
                 no pin for it. Measure and add one:\n\
                 \x20           normalized: Some(NormalizedPin {{\n\
                 \x20               digest: \"{}\",\n\
                 \x20               length: {},\n\
                 \x20           }}),",
                pin.resource,
                hex::encode(Blake2b256::hash(par.encode_to_vec())),
                par.encode_to_vec().len(),
            )),
            (Err(e), Some(_)) => status_changed.push(format!(
                "  {} carries a normalized pin but NO LONGER NORMALIZES — for a `.rho` that is a \
                 genesis FAILURE, not a hash change: {e:?}",
                pin.resource,
            )),
            (Err(_), None) => {}
        }
    }

    assert!(
        status_changed.is_empty(),
        "★★ a blessed contract's NORMALIZATION STATUS changed, so the set of terms this cell \
         guards is no longer the set it was derived for:\n{}",
        status_changed.join("\n"),
    );

    assert!(
        drifted.is_empty(),
        "★★ {} blessed contract(s) NORMALIZED TERM changed, so the genesis post-state changed. \
         That is the strongest consensus signal there is, and it owes a \
         `docs/consensus/consensus-change-register.md` entry.\n\
         ⚠ Read the LENGTH before concluding anything: a length that moved with the digest means a \
         SOURCE edit (cross-check `every_blessed_contract_source_is_pinned`, which must then be red \
         too); a digest that moved at CONSTANT length with every source pin green means the \
         normalizer or the encoder changed, and the change reaches every contract of that shape \
         rather than the one row someone happened to look at.\n\
         Paste the printed block into the matching row IN THE SAME COMMIT.\n\n{}",
        drifted.len(),
        drifted.join("\n"),
    );

    assert!(
        pinned >= MIN_PINNED,
        "★ FLOOR: only {pinned} of the blessed contracts carry a normalized-term pin, but \
         {MIN_PINNED} normalize unsubstituted and therefore have a consensus-visible term. Rows \
         were demoted to `normalized: None` without losing the property that makes them \
         consensus-visible, which makes this cell quieter than it looks.",
    );
}
