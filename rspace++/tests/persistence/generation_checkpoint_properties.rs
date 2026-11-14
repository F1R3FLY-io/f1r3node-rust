use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::path::PathBuf;

use proptest::prelude::*;

use super::checkpoint_wal::fixture_with_retained_union_cardinality;
use super::property_support::{
    InventoryMutation, PROPERTY_BINDINGS, binding_map, independent_inventory_digest,
    inventory_case, mathematical_checkpoint_union,
};
use super::support::TestSemanticProfile;
use crate::rspace::persistence::checkpoint::{
    CheckpointObjectInventoryV1, GenerationCheckpointDescriptorV1,
    GenerationCheckpointDescriptorViewV1, GenerationCheckpointErrorV1,
    OrderedObjectInventoryBuilderV1, try_visit_checkpoint_references,
};
use crate::rspace::persistence::closure::StructuralClosureLedgerV1;
use crate::rspace::persistence::durable::{
    INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1, IndexGenerationV1, ObjectIndexHeaderV1,
    PACK_HEADER_LEN_V1, PreparedGenerationCheckpointIndexV1,
};
use crate::rspace::persistence::wal::{
    GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1, GenerationCheckpointCommitViewV1,
    GenerationCheckpointPrepareRecordViewV1, PreparedGenerationCheckpointV1,
    preflight_generation_checkpoint_prepare_v1,
};
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, CanonicalEncode, CatalogSelectionV1, LogPackAddress, LogPackDecodeLimits,
    LogPackSequence, ObjectIndexFileViewV1, ObjectRefV1, PackHeaderV1, RetentionManifestView,
    SemanticProfileId, StateVersionV1, StoreId,
};

const PROFILE: SemanticProfileId = SemanticProfileId::from_array(*b"RSpace.Test.V1\0\0");

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn checkpoint_union_is_exact_disjoint_ordered_merge(case in inventory_case()) {
        let ledger = StructuralClosureLedgerV1::from_test_retained_union(&case.retained);
        let expected = mathematical_checkpoint_union(&case);
        let mut actual = Vec::new();
        try_visit_checkpoint_references(&ledger, case.manifest, |reference| {
            actual.push(reference);
            Ok::<(), Infallible>(())
        })
        .unwrap();

        prop_assert_eq!(&actual, &expected);
        prop_assert_eq!(actual.len(), case.retained.len() + 1);
        prop_assert_eq!(
            actual.iter().map(|reference| reference.encoded_len()).sum::<u64>(),
            case.retained.iter().map(|reference| reference.encoded_len()).sum::<u64>()
                + case.manifest.encoded_len()
        );
        prop_assert!(actual.windows(2).all(|pair| pair[0].digest() < pair[1].digest()));
        for retained in &case.retained {
            prop_assert_eq!(actual.iter().filter(|candidate| *candidate == retained).count(), 1);
        }
        prop_assert_eq!(actual.iter().filter(|candidate| **candidate == case.manifest).count(), 1);
    }

    #[test]
    fn manifest_digest_alias_reports_both_declared_lengths(
        case in inventory_case(),
        conflicting_len in 32_u64..1_048_576,
    ) {
        let ledger = StructuralClosureLedgerV1::from_test_retained_union(&case.retained);
        let retained = case.retained[case.retained.len() / 2];
        let alias = ObjectRefV1::new(retained.digest(), conflicting_len).unwrap();
        let error = try_visit_checkpoint_references(&ledger, alias, |_| Ok::<(), Infallible>(()))
            .unwrap_err();
        match error {
            crate::rspace::persistence::checkpoint::CheckpointReferenceVisitErrorV1::Checkpoint(
                GenerationCheckpointErrorV1::DuplicateInventoryDigest {
                    first_len,
                    second_len,
                    ..
                },
            ) => {
                prop_assert_eq!(first_len, conflicting_len);
                prop_assert_eq!(second_len, retained.encoded_len());
            }
            other => prop_assert!(false, "unexpected alias result: {other:?}"),
        }
    }

    #[test]
    fn current_closure_is_a_native_subset_of_retained_union(case in inventory_case()) {
        let current = StructuralClosureLedgerV1::from_test_retained_union(&case.current);
        let retained = StructuralClosureLedgerV1::from_test_retained_union(&case.retained);
        let expected_current = case.current.iter().copied().collect::<BTreeSet<_>>();
        let expected_retained = case.retained.iter().copied().collect::<BTreeSet<_>>();
        let mut observed_current = BTreeSet::new();
        let mut observed_retained = BTreeSet::new();
        current
            .try_visit_retained_union(|reference| {
                observed_current.insert(reference);
                Ok::<(), Infallible>(())
            })
            .unwrap();
        retained
            .try_visit_retained_union(|reference| {
                observed_retained.insert(reference);
                Ok::<(), Infallible>(())
            })
            .unwrap();

        prop_assert_eq!(&observed_current, &expected_current);
        prop_assert_eq!(&observed_retained, &expected_retained);
        prop_assert!(observed_current.is_subset(&observed_retained));
        prop_assert_eq!(observed_current.len() as u64, case.current_objects);
        prop_assert_eq!(
            observed_current
                .iter()
                .map(|reference| reference.encoded_len())
                .sum::<u64>(),
            case.current_bytes,
        );

        let checkpoint = mathematical_checkpoint_union(&case)
            .into_iter()
            .collect::<BTreeSet<_>>();
        let expected_checkpoint = observed_retained
            .iter()
            .copied()
            .chain(std::iter::once(case.manifest))
            .collect::<BTreeSet<_>>();
        prop_assert_eq!(checkpoint, expected_checkpoint);
    }

    #[test]
    fn descriptor_roundtrip_preserves_all_six_inventory_scalars(case in inventory_case()) {
        let ledger = StructuralClosureLedgerV1::from_test_retained_union(&case.retained);
        let inventory = CheckpointObjectInventoryV1::from_retained_union(&ledger, case.manifest)
            .unwrap();
        let descriptor = GenerationCheckpointDescriptorV1::from_test_inventory(
            PROFILE,
            StateVersionV1::new(17, case.retained[0]),
            case.manifest,
            case.current_objects,
            case.current_bytes,
            inventory,
        )
        .unwrap();
        let bytes = descriptor.try_canonical_bytes().unwrap();
        prop_assert_eq!(bytes.len(), 216);
        let parsed = GenerationCheckpointDescriptorViewV1::parse(&bytes).unwrap();

        prop_assert_eq!(parsed.profile_id(), PROFILE);
        prop_assert_eq!(parsed.state(), StateVersionV1::new(17, case.retained[0]));
        prop_assert_eq!(parsed.retention_manifest(), case.manifest);
        prop_assert_eq!(parsed.current_state_objects(), case.current_objects);
        prop_assert_eq!(parsed.current_state_canonical_bytes().get(), case.current_bytes);
        prop_assert_eq!(parsed.retained_union_objects(), case.retained.len() as u64);
        prop_assert_eq!(
            parsed.retained_union_canonical_bytes().get(),
            case.retained.iter().map(|reference| reference.encoded_len()).sum::<u64>()
        );
        prop_assert_eq!(parsed.data_objects(), case.retained.len() as u64 + 1);
        prop_assert_eq!(
            parsed.data_canonical_bytes().get(),
            case.retained.iter().map(|reference| reference.encoded_len()).sum::<u64>()
                + case.manifest.encoded_len()
        );
        prop_assert_eq!(
            parsed.ordered_digest(),
            independent_inventory_digest(&mathematical_checkpoint_union(&case)),
        );
    }

    #[test]
    fn generated_checkpoint_wire_extents_follow_the_formal_length_equations(
        retained_objects in 2_usize..=256,
    ) {
        let source = fixture_with_retained_union_cardinality(retained_objects);
        let store_id = StoreId::from_array([0x91; 16]);
        let sequence = LogPackSequence::new(19).unwrap();
        let prepared = PreparedGenerationCheckpointV1::<TestSemanticProfile, _>::new(
            &source,
            store_id,
            sequence,
            LogPackDecodeLimits::UNBOUNDED,
        )
        .unwrap();
        let inventory = source.descriptor.inventory();
        let data_objects = inventory.data_objects();
        let data_bytes = inventory.data_canonical_bytes().get();
        let expected_prepare = 288_u64
            .checked_add(40_u64.checked_mul(data_objects).unwrap())
            .and_then(|length| length.checked_add(data_bytes))
            .and_then(|length| length.checked_add(8))
            .unwrap();
        prop_assert_eq!(prepared.record_len(), expected_prepare);

        let prepare_address =
            LogPackAddress::new(29, PACK_HEADER_LEN_V1, prepared.record_len()).unwrap();
        let index = PreparedGenerationCheckpointIndexV1::new(
            ObjectIndexHeaderV1::new(store_id, IndexGenerationV1::FIRST),
            prepare_address,
            &prepared,
        )
        .unwrap();
        let expected_index = INDEX_HEADER_LEN_V1
            .checked_add(INDEX_RECORD_LEN_V1.checked_mul(data_objects).unwrap())
            .unwrap();
        prop_assert_eq!(index.exact_tail(), expected_index);

        let commit_offset = PACK_HEADER_LEN_V1.checked_add(expected_prepare).unwrap();
        prop_assert_eq!(prepare_address.end(), commit_offset);
        let pack_tail = commit_offset
            .checked_add(GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1)
            .unwrap();
        prop_assert_eq!(pack_tail, 128 + expected_prepare + 400);
    }

    #[test]
    fn ordered_inventory_digest_matches_independent_fold(case in inventory_case()) {
        let expected = mathematical_checkpoint_union(&case);
        let expected_bytes = expected.iter().map(|reference| reference.encoded_len()).sum();
        let mut builder = OrderedObjectInventoryBuilderV1::new(expected.len() as u64, expected_bytes);
        for reference in &expected {
            builder.push(*reference).unwrap();
        }
        let observed = builder.finish().unwrap();
        prop_assert_eq!(observed.digest(), independent_inventory_digest(&expected));
    }

    #[test]
    fn every_inventory_mutation_changes_or_rejects_authority(
        case in inventory_case(),
    ) {
        let baseline = mathematical_checkpoint_union(&case);
        let baseline_digest = independent_inventory_digest(&baseline);
        for mutation in InventoryMutation::ALL {
            let mutated = super::property_support::apply_inventory_mutation(&baseline, mutation);
            let mutated_bytes = mutated.iter().map(|reference| reference.encoded_len()).sum();
            let mut builder =
                OrderedObjectInventoryBuilderV1::new(mutated.len() as u64, mutated_bytes);
            let mut rejected = None;
            for reference in &mutated {
                if let Err(error) = builder.push(*reference) {
                    rejected = Some(error);
                    break;
                }
            }
            if rejected.is_none() {
                let mutated_digest = builder.finish().unwrap().digest();
                prop_assert_ne!(mutated_digest, baseline_digest, "mutation {:?}", mutation);
            }
        }
    }

    #[test]
    fn arbitrary_checkpoint_wire_bytes_are_total(
        bytes in proptest::collection::vec(any::<u8>(), 0..1024),
        catalog in proptest::collection::vec(any::<u8>(), CATALOG_IMAGE_LEN..=CATALOG_IMAGE_LEN),
    ) {
        let catalog: [u8; CATALOG_IMAGE_LEN] = catalog
            .try_into()
            .expect("the generator fixes the catalog image length");
        let _ = GenerationCheckpointDescriptorViewV1::parse(&bytes);
        let _ = preflight_generation_checkpoint_prepare_v1(
            &bytes,
            LogPackDecodeLimits::UNBOUNDED,
        );
        let _ = GenerationCheckpointPrepareRecordViewV1::parse(
            &bytes,
            LogPackDecodeLimits::UNBOUNDED,
        );
        let _ = GenerationCheckpointCommitViewV1::parse(&bytes);
        let _ = ObjectIndexFileViewV1::parse_exact(&bytes);
        let _ = PackHeaderV1::parse_exact(&bytes);
        let _ = RetentionManifestView::parse(&bytes);
        let _ = CatalogSelectionV1::select(&catalog);
    }

    #[test]
    fn arbitrary_checkpoint_descriptor_bytes_are_total(
        bytes in proptest::collection::vec(any::<u8>(), 0..1024),
    ) {
        let _ = GenerationCheckpointDescriptorViewV1::parse(&bytes);
    }

    #[test]
    fn arbitrary_generation_prepare_preflight_bytes_are_total(
        bytes in proptest::collection::vec(any::<u8>(), 0..1024),
    ) {
        let _ = preflight_generation_checkpoint_prepare_v1(
            &bytes,
            LogPackDecodeLimits::UNBOUNDED,
        );
    }

    #[test]
    fn arbitrary_generation_prepare_bytes_are_total(
        bytes in proptest::collection::vec(any::<u8>(), 0..1024),
    ) {
        let _ = GenerationCheckpointPrepareRecordViewV1::parse(
            &bytes,
            LogPackDecodeLimits::UNBOUNDED,
        );
    }

    #[test]
    fn arbitrary_generation_commit_bytes_are_total(
        bytes in proptest::collection::vec(any::<u8>(), 0..1024),
    ) {
        let _ = GenerationCheckpointCommitViewV1::parse(&bytes);
    }

    #[test]
    fn arbitrary_object_index_bytes_are_total(
        bytes in proptest::collection::vec(any::<u8>(), 0..1024),
    ) {
        let _ = ObjectIndexFileViewV1::parse_exact(&bytes);
    }

    #[test]
    fn arbitrary_pack_header_bytes_are_total(
        bytes in proptest::collection::vec(any::<u8>(), 0..1024),
    ) {
        let _ = PackHeaderV1::parse_exact(&bytes);
    }

    #[test]
    fn arbitrary_retention_manifest_bytes_are_total(
        bytes in proptest::collection::vec(any::<u8>(), 0..1024),
    ) {
        let _ = RetentionManifestView::parse(&bytes);
    }

    #[test]
    fn arbitrary_exact_catalog_images_are_total(
        catalog in proptest::collection::vec(any::<u8>(), CATALOG_IMAGE_LEN..=CATALOG_IMAGE_LEN),
    ) {
        let catalog: [u8; CATALOG_IMAGE_LEN] = catalog
            .try_into()
            .expect("the generator fixes the catalog image length");
        let _ = CatalogSelectionV1::select(&catalog);
    }
}

#[test]
fn dormant_checkpoint_publisher_has_no_runtime_accounting_capability() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let production_root = crate_root.join("src");
    let allowed_publisher = production_root.join("rspace/persistence/durable/publication.rs");
    let forbidden_capabilities =
        ["RuntimeBudget", "BillableKind", "reserve_comm", "cost_trace_digest", "storage_charge"];
    let mut pending = vec![production_root];
    let mut publisher_occurrences = 0_usize;
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            for capability in forbidden_capabilities {
                assert!(
                    !source.contains(capability),
                    "physical persistence source {} gained semantic-accounting capability \
                     {capability}",
                    path.display(),
                );
            }
            let occurrences = source.matches("publish_generation_checkpoint_v1").count();
            if occurrences != 0 {
                assert_eq!(
                    path, allowed_publisher,
                    "checkpoint publication escaped its dormant physical boundary",
                );
                publisher_occurrences += occurrences;
            }
        }
    }
    assert!(publisher_occurrences >= 1, "checkpoint publisher entry point disappeared");

    let manifest = std::fs::read_to_string(crate_root.join("Cargo.toml")).unwrap();
    for line in manifest.lines().map(str::trim) {
        assert!(
            !line.starts_with("rholang =") &&
                !line.starts_with("casper =") &&
                line != "package = \"rholang\"" &&
                line != "package = \"casper\"",
            "rspace_plus_plus gained a production dependency on the semantic-accounting layer: \
             {line}",
        );
    }
}

#[test]
fn model_property_manifest_is_unique_and_exhaustive() {
    const FAMILIES: &[&str] = &[
        "INV", "DES", "WIR", "WAL", "CAT", "PUB", "SEM", "RES", "NV", "HJ", "JV", "JR", "SI", "IP",
        "IR", "CR", "LT", "DC", "CL", "RV", "TS", "TH", "PX", "RI", "SW", "OG", "VR", "DI", "SG",
        "GC", "TX", "HR", "DF", "CB", "XB", "KG", "TM",
    ];
    let is_property_id = |token: &str| {
        let Some((family, index)) = token.split_once('-') else {
            return false;
        };
        FAMILIES.contains(&family) &&
            index.len() == 2 &&
            index.bytes().all(|byte| byte.is_ascii_digit())
    };
    let bindings = binding_map();
    assert_eq!(bindings.len(), PROPERTY_BINDINGS.len(), "duplicate property ID");
    let catalog = include_str!(
        "../../../docs/design/pathmap/generation-checkpoint-invariant-catalog-2026-08-28.md"
    );
    let formal_ids = PROPERTY_BINDINGS
        .iter()
        .map(|(id, _)| *id)
        .collect::<BTreeSet<_>>();
    let catalog_ids = catalog
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '-'))
        .filter(|token| is_property_id(token))
        .collect::<BTreeSet<_>>();
    assert_eq!(formal_ids, catalog_ids, "catalog and executable property IDs diverged");

    let derivation_section = catalog
        .split_once("## Formal-to-executable derivation matrix")
        .expect("catalog carries the formal derivation matrix")
        .1
        .split_once("## Generator discipline")
        .expect("formal derivation matrix has a bounded section")
        .0;
    let mut derivations = BTreeMap::<&str, (Vec<&str>, Vec<&str>, &str)>::new();
    for line in derivation_section.lines() {
        let cells = line.split('|').map(str::trim).collect::<Vec<_>>();
        if cells.len() < 7 {
            continue;
        }
        let id = cells[1];
        if !is_property_id(id) {
            continue;
        }
        let parse_symbols = |cell: &'static str| {
            cell.split(',')
                .map(|symbol| symbol.trim().trim_matches('`'))
                .filter(|symbol| !symbol.is_empty())
                .collect::<Vec<_>>()
        };
        assert!(
            derivations
                .insert(id, (parse_symbols(cells[2]), parse_symbols(cells[3]), cells[4]))
                .is_none(),
            "duplicate formal derivation row {id}",
        );
    }
    assert_eq!(
        derivations.keys().copied().collect::<BTreeSet<_>>(),
        formal_ids,
        "formal derivation rows and executable property IDs diverged",
    );

    let tla_source = [
        include_str!(
            "../../../formal/tlaplus/stack_safe_pda/RSpaceGenerationCheckpointPublication.tla"
        ),
        include_str!("../../../formal/tlaplus/stack_safe_pda/RSpaceRetentionV2Upgrade.tla"),
        include_str!(
            "../../../formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublication.tla"
        ),
        include_str!("../../../formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistory.tla"),
        include_str!("../../../formal/tlaplus/stack_safe_pda/RSpaceJournalPublicationOwner.tla"),
        include_str!("../../../formal/tlaplus/stack_safe_pda/RSpaceCompaction.tla"),
        include_str!("../../../formal/tlaplus/stack_safe_pda/RSpaceGcPendingReclaimIntent.tla"),
        include_str!("../../../formal/tlaplus/stack_safe_pda/RSpaceGcProtection.tla"),
    ]
    .concat();
    let tla_configs = [
        include_str!(
            "../../../formal/tlaplus/stack_safe_pda/RSpaceGenerationCheckpointPublication.cfg"
        ),
        include_str!("../../../formal/tlaplus/stack_safe_pda/RSpaceRetentionV2Upgrade.cfg"),
        include_str!(
            "../../../formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublication.cfg"
        ),
        include_str!("../../../formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistory.cfg"),
    ];
    let rocq_source = [
        include_str!(
            "../../../formal/rocq/stack_safe_pda/theories/GenerationCheckpointPublication.v"
        ),
        include_str!("../../../formal/rocq/stack_safe_pda/theories/PathExactIncrementalClosure.v"),
        include_str!("../../../formal/rocq/stack_safe_pda/theories/IncrementalLedgerTrial.v"),
        include_str!("../../../formal/rocq/stack_safe_pda/theories/IncrementalDomainComposition.v"),
        include_str!("../../../formal/rocq/stack_safe_pda/theories/CreditedClosureLedger.v"),
        include_str!("../../../formal/rocq/stack_safe_pda/theories/RetentionV2Format.v"),
        include_str!("../../../formal/rocq/stack_safe_pda/theories/RetentionV2Editor.v"),
        include_str!("../../../formal/rocq/stack_safe_pda/theories/TrustedSuccessorCapability.v"),
        include_str!("../../../formal/rocq/stack_safe_pda/theories/ExactStateShapeSummary.v"),
        include_str!("../../../formal/rocq/stack_safe_pda/theories/RSpaceDirectManifestIdentity.v"),
    ]
    .concat();
    let mut cited_tla = BTreeSet::new();
    let mut cited_rocq = BTreeSet::new();
    for (id, (tla_symbols, rocq_symbols, class)) in &derivations {
        assert!(matches!(*class, "D" | "P" | "E"), "unresolved derivation class {class} for {id}");
        let tla_is_executable_only = tla_symbols.as_slice() == ["EXEC_ONLY"];
        let rocq_is_executable_only = rocq_symbols.as_slice() == ["EXEC_ONLY"];
        if *class == "E" {
            assert!(
                tla_is_executable_only && rocq_is_executable_only,
                "executable-only row {id} cites a formal claim",
            );
        } else {
            assert!(
                !tla_is_executable_only || !rocq_is_executable_only,
                "derived row {id} cites no formal artifact",
            );
        }
        for symbol in tla_symbols {
            if *symbol == "EXEC_ONLY" {
                continue;
            }
            assert!(
                tla_source.contains(&format!("{symbol} ==")),
                "derivation row {id} cites missing TLA+ symbol {symbol}",
            );
            cited_tla.insert(*symbol);
        }
        for symbol in rocq_symbols {
            if *symbol == "EXEC_ONLY" {
                continue;
            }
            let declaration = if let Some((module, name)) = symbol.rsplit_once('.') {
                assert!(
                    rocq_source.contains(&format!("Module {module}.")),
                    "derivation row {id} cites missing Rocq module {module}"
                );
                name
            } else {
                symbol
            };
            assert!(
                rocq_source.contains(&format!("Theorem {declaration}")) ||
                    rocq_source.contains(&format!("Lemma {declaration}")),
                "derivation row {id} cites missing Rocq theorem/lemma {symbol}",
            );
            cited_rocq.insert(*symbol);
        }
    }

    for config in tla_configs {
        for invariant in tlc_config_invariants(config)
            .expect("a base TLC configuration follows the TLC configuration grammar")
        {
            assert!(
                cited_tla.contains(invariant),
                "base TLA+ invariant {invariant} is absent from the exhaustive derivation matrix",
            );
        }
    }
    for capstone in rocq_source.lines().filter_map(|line| {
        line.trim()
            .strip_prefix("Print Assumptions ")
            .map(|symbol| symbol.trim_end_matches('.'))
    }) {
        assert!(
            cited_rocq.contains(capstone),
            "Rocq capstone {capstone} is absent from the exhaustive derivation matrix",
        );
    }

    let repository_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rspace++ is directly below the repository root")
        .to_path_buf();
    let formal_gate =
        std::fs::read_to_string(repository_root.join("scripts/check-stack-safe-pda-proofs.sh"))
            .unwrap();
    assert!(
        formal_gate.contains("theories/PathExactIncrementalClosure.v"),
        "the integrated formal gate does not compile the path-exact closure theory cited by PX \
         rows",
    );
    assert!(
        formal_gate.contains("theories/IncrementalLedgerTrial.v"),
        "the integrated formal gate does not compile the ledger-trial theory cited by LT rows",
    );
    assert!(
        formal_gate.contains("theories/IncrementalDomainComposition.v"),
        "the integrated formal gate does not compile the composition theory cited by DC rows",
    );
    assert!(
        formal_gate.contains("theories/CreditedClosureLedger.v"),
        "the integrated formal gate does not compile the credited-closure theory cited by CL rows",
    );
    assert!(
        formal_gate.contains("theories/RetentionV2Format.v"),
        "the integrated formal gate does not compile the retention V2 theory cited by RV rows",
    );
    assert!(
        formal_gate.contains("theories/RetentionV2Editor.v"),
        "the integrated formal gate does not compile the radix editor theory cited by RV rows",
    );
    assert!(
        formal_gate.contains("theories/TrustedSuccessorCapability.v"),
        "the integrated formal gate does not compile the trusted successor theory cited by TS rows",
    );
    assert!(
        formal_gate.contains("theories/ExactStateShapeSummary.v"),
        "the integrated formal gate does not compile the shape summary theory cited by TS rows",
    );
    assert!(
        formal_gate.contains("theories/RSpaceDirectManifestIdentity.v"),
        "the integrated formal gate does not compile the direct manifest identity theory cited by \
         DI rows",
    );
    assert!(
        formal_gate.contains("check-rspace-trusted-history-model.sh"),
        "the integrated formal gate does not run the trusted history model cited by TH rows",
    );
    assert!(
        formal_gate.contains("check-rspace-gc-pending-reclaim-intent-model.sh"),
        "the integrated formal gate does not run the reclamation-intent model cited by RI rows",
    );
    for (id, (tla_symbols, _, _)) in derivations.iter().filter(|(id, _)| id.starts_with("NV-")) {
        let witness = tla_symbols
            .iter()
            .copied()
            .find(|symbol| symbol.starts_with("Never"))
            .unwrap_or_else(|| panic!("non-vacuity row {id} names no Never* witness"));
        let config_name = format!("RSpaceGenerationCheckpointPublication{witness}.cfg");
        let config = std::fs::read_to_string(
            repository_root
                .join("formal/tlaplus/stack_safe_pda")
                .join(&config_name),
        )
        .unwrap_or_else(|error| panic!("non-vacuity row {id} lacks {config_name}: {error}"));
        assert!(config.contains(&format!("INVARIANT {witness}")));
        assert!(
            formal_gate.contains(&config_name) && formal_gate.contains(witness),
            "non-vacuity row {id} is not enforced by the integrated formal gate",
        );
    }
    for (id, (tla_symbols, _, _)) in derivations.iter().filter(|(id, _)| id.starts_with("RV-")) {
        for witness in tla_symbols
            .iter()
            .copied()
            .filter(|symbol| symbol.starts_with("Never"))
        {
            let config_name = format!("RSpaceRetentionV2Upgrade{witness}.cfg");
            let config = std::fs::read_to_string(
                repository_root
                    .join("formal/tlaplus/stack_safe_pda")
                    .join(&config_name),
            )
            .unwrap_or_else(|error| panic!("retention row {id} lacks {config_name}: {error}"));
            assert!(config.contains(&format!("INVARIANT {witness}")));
            assert!(
                formal_gate.contains(&config_name) && formal_gate.contains(witness),
                "retention row {id} witness {witness} is not enforced by the integrated formal \
                 gate",
            );
        }
    }
    for (id, (tla_symbols, _, _)) in derivations.iter().filter(|(id, _)| id.starts_with("TS-")) {
        for witness in tla_symbols
            .iter()
            .copied()
            .filter(|symbol| symbol.starts_with("Never"))
        {
            let config_name = format!("RSpaceTrustedIncrementalPublication{witness}.cfg");
            let config = std::fs::read_to_string(
                repository_root
                    .join("formal/tlaplus/stack_safe_pda")
                    .join(&config_name),
            )
            .unwrap_or_else(|error| panic!("trusted row {id} lacks {config_name}: {error}"));
            assert!(config.contains(&format!("INVARIANT {witness}")));
            assert!(
                formal_gate.contains(&config_name) && formal_gate.contains(witness),
                "trusted row {id} witness {witness} is not enforced by the integrated formal gate",
            );
        }
    }
    let history_gate = std::fs::read_to_string(
        repository_root.join("scripts/check-rspace-trusted-history-model.sh"),
    )
    .expect("the trusted history model gate is readable");
    for (id, (tla_symbols, _, _)) in derivations.iter().filter(|(id, _)| id.starts_with("TH-")) {
        for witness in tla_symbols
            .iter()
            .copied()
            .filter(|symbol| symbol.starts_with("Never"))
        {
            let config_name = format!("RSpaceTrustedIncrementalHistory{witness}.cfg");
            let config = std::fs::read_to_string(
                repository_root
                    .join("formal/tlaplus/stack_safe_pda")
                    .join(&config_name),
            )
            .unwrap_or_else(|error| panic!("history row {id} lacks {config_name}: {error}"));
            assert!(config.contains(&format!("INVARIANT {witness}")));
            assert!(
                history_gate.contains(&config_name) && history_gate.contains(witness),
                "history row {id} witness {witness} is not enforced by the trusted history gate",
            );
        }
    }

    let executable_sources = [
        include_str!("checkpoint_codec.rs"),
        include_str!("checkpoint_wal.rs"),
        include_str!("cow_reachability_ledger_properties.rs"),
        include_str!("durable_catalog.rs"),
        include_str!("durable_history_repository.rs"),
        include_str!("durable_publication.rs"),
        include_str!("durable_read_handle.rs"),
        include_str!("durable_recovery.rs"),
        include_str!("durable_rspace_authority.rs"),
        include_str!("durable_state_root.rs"),
        include_str!("durable_view_reader.rs"),
        include_str!("generation_checkpoint_properties.rs"),
        include_str!("incremental_reachability_properties.rs"),
        include_str!("owner_checkpoint_view.rs"),
        include_str!("owner_incarnation.rs"),
        include_str!("path_exact_closure_properties.rs"),
        include_str!("path_provenance.rs"),
        include_str!("retention_v2_editor_properties.rs"),
        include_str!("retention_v2_format_properties.rs"),
        include_str!("shape_summary_properties.rs"),
        include_str!("trusted_history_properties.rs"),
        include_str!("trusted_scaling_properties.rs"),
        include_str!("trusted_scaling_resource.rs"),
        include_str!("trusted_successor_properties.rs"),
        include_str!("wal_codec.rs"),
        include_str!("../hot_store/journal_commit.rs"),
        include_str!("../state/atomic_state_authority.rs"),
        include_str!("durable_store_genesis.rs"),
        include_str!("gc_protection_properties.rs"),
        include_str!("transfer_export.rs"),
        include_str!("state_difference.rs"),
        include_str!("../hot_store/durable_reader_bootstrap.rs"),
        include_str!("transfer_import_staging.rs"),
        include_str!("../hot_store/cross_base_capture.rs"),
        include_str!("durable_authority_backend.rs"),
        include_str!("durable_backend_selection.rs"),
        include_str!("owner_cross_base_checkpoint.rs"),
        include_str!("kg_trait_v1.rs"),
        include_str!("../hot_store/journal_rebase.rs"),
        include_str!("owner_transition_memo.rs"),
        include_str!("transfer_import_seal.rs"),
        include_str!("transfer_export_binding.rs"),
        include_str!("../rho_path_v2_bridge.rs"),
    ]
    .concat();
    for (id, target) in PROPERTY_BINDINGS {
        assert!(!bindings[id].is_empty(), "property ID {id} has no executable refinement target");
        assert!(
            executable_sources.contains(&format!("fn {target}(")),
            "property ID {id} names missing executable target {target}",
        );
    }

    for required_family in FAMILIES {
        assert!(
            PROPERTY_BINDINGS.iter().any(|(id, _)| id
                .split_once('-')
                .is_some_and(|(family, _)| family == *required_family)),
            "formal property family {required_family} is unrepresented"
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TlcConfigDefect {
    UnterminatedComment,
    UnterminatedString,
    UnexpectedCharacter,
    ExpectedKeyword,
    MissingOperand,
    RepeatedKeyword,
    ExpectedBoolean,
    InvalidInvariantName,
    MalformedConstant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TlcConfigError {
    line: usize,
    defect: TlcConfigDefect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TlcTokenKind {
    Word,
    Text,
    Mark,
}

#[derive(Clone, Copy, Debug)]
struct TlcToken<'a> {
    image: &'a str,
    kind: TlcTokenKind,
    line: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TlcStatement {
    SingleValue,
    Invariants,
    OtherNames,
    Constants,
    CheckDeadlock,
}

fn tlc_statement(image: &str) -> Option<TlcStatement> {
    match image {
        "INIT" | "NEXT" | "SPECIFICATION" | "VIEW" | "SYMMETRY" | "TYPE" | "TYPE_CONSTRAINT" => {
            Some(TlcStatement::SingleValue)
        }
        "INVARIANT" | "INVARIANTS" => Some(TlcStatement::Invariants),
        "PROPERTY" | "PROPERTIES" | "CONSTRAINT" | "CONSTRAINTS" | "ACTION_CONSTRAINT" |
        "ACTION_CONSTRAINTS" => Some(TlcStatement::OtherNames),
        "CONSTANT" | "CONSTANTS" => Some(TlcStatement::Constants),
        "CHECK_DEADLOCK" => Some(TlcStatement::CheckDeadlock),
        _ => None,
    }
}

fn tlc_config_tokens(config: &str) -> Result<Vec<TlcToken<'_>>, TlcConfigError> {
    let bytes = config.as_bytes();
    let mut tokens = Vec::new();
    let mut line = 1_usize;
    let mut cursor = 0_usize;
    while let Some(&byte) = bytes.get(cursor) {
        let start = cursor;
        let kind = match (byte, bytes.get(cursor + 1).copied()) {
            (b'\n', _) => {
                line += 1;
                cursor += 1;
                None
            }
            (b' ' | b'\t' | b'\r' | 0x0c, _) => {
                cursor += 1;
                None
            }
            (b'\\', Some(b'*')) => {
                cursor = bytes[cursor..]
                    .iter()
                    .position(|&candidate| candidate == b'\n')
                    .map_or(bytes.len(), |offset| cursor + offset);
                None
            }
            (b'(', Some(b'*')) => {
                let opened = line;
                let mut depth = 1_usize;
                cursor += 2;
                while depth > 0 {
                    match (bytes.get(cursor).copied(), bytes.get(cursor + 1).copied()) {
                        (Some(b'('), Some(b'*')) => {
                            depth += 1;
                            cursor += 2;
                        }
                        (Some(b'*'), Some(b')')) => {
                            depth -= 1;
                            cursor += 2;
                        }
                        (Some(b'\n'), _) => {
                            line += 1;
                            cursor += 1;
                        }
                        (Some(_), _) => cursor += 1,
                        (None, _) => {
                            return Err(TlcConfigError {
                                line: opened,
                                defect: TlcConfigDefect::UnterminatedComment,
                            });
                        }
                    }
                }
                None
            }
            (b'"', _) => {
                cursor += 1;
                loop {
                    match (bytes.get(cursor).copied(), bytes.get(cursor + 1).copied()) {
                        (Some(b'"'), _) => {
                            cursor += 1;
                            break;
                        }
                        (Some(b'\\'), Some(escaped)) if escaped != b'\n' => cursor += 2,
                        (Some(b'\n') | None, _) => {
                            return Err(TlcConfigError {
                                line,
                                defect: TlcConfigDefect::UnterminatedString,
                            });
                        }
                        (Some(_), _) => cursor += 1,
                    }
                }
                Some(TlcTokenKind::Text)
            }
            (b'<', Some(b'-')) => {
                cursor += 2;
                Some(TlcTokenKind::Mark)
            }
            (b'=' | b'!' | b'[' | b']' | b'(' | b')' | b'{' | b'}' | b',', _) => {
                cursor += 1;
                Some(TlcTokenKind::Mark)
            }
            (word, _) if word.is_ascii_alphanumeric() || word == b'_' => {
                cursor += bytes[cursor..]
                    .iter()
                    .take_while(|candidate| {
                        candidate.is_ascii_alphanumeric() || **candidate == b'_'
                    })
                    .count();
                Some(TlcTokenKind::Word)
            }
            _ => {
                return Err(TlcConfigError {
                    line,
                    defect: TlcConfigDefect::UnexpectedCharacter,
                });
            }
        };
        if let Some(kind) = kind {
            tokens.push(TlcToken {
                image: &config[start..cursor],
                kind,
                line,
            });
        }
    }
    Ok(tokens)
}

fn tlc_config_value_end(tokens: &[TlcToken<'_>], start: usize) -> Option<usize> {
    let image = |position: usize| tokens.get(position).map(|token| token.image);
    match (image(start)?, image(start + 1)) {
        ("{", Some("}")) => Some(start + 2),
        ("{", _) => {
            let mut position = start + 1;
            loop {
                position = tlc_config_value_end(tokens, position)?;
                match image(position)? {
                    "," => position += 1,
                    "}" => return Some(position + 1),
                    _ => return None,
                }
            }
        }
        _ => Some(start + 1),
    }
}

fn tlc_config_constant_end(tokens: &[TlcToken<'_>], start: usize) -> Result<usize, TlcConfigError> {
    let image = |position: usize| tokens.get(position).map(|token| token.image);
    let malformed = TlcConfigError {
        line: tokens[start].line,
        defect: TlcConfigDefect::MalformedConstant,
    };
    let mut position = start + 1;
    while image(position) == Some("!") {
        match image(position + 1) {
            Some(_) => position += 2,
            None => return Err(malformed),
        }
    }
    match (image(position), image(position + 1)) {
        (Some("<-"), Some("[")) => {
            match (image(position + 2), image(position + 3), image(position + 4)) {
                (Some(_), Some("]"), Some(_)) => Ok(position + 5),
                _ => Err(malformed),
            }
        }
        (Some("<-"), Some(_)) => Ok(position + 2),
        (Some("<-"), None) | (None, _) => Err(malformed),
        (Some(first), _) => {
            if first == "(" {
                position += 1;
                loop {
                    position = tlc_config_value_end(tokens, position).ok_or(malformed)?;
                    match image(position) {
                        Some(",") => position += 1,
                        _ => break,
                    }
                }
                match image(position) {
                    Some(")") => position += 1,
                    _ => return Err(malformed),
                }
            }
            match (image(position), image(position + 1), image(position + 2), image(position + 3)) {
                (Some("="), Some("["), Some(_), Some("]")) => {
                    tlc_config_value_end(tokens, position + 4).ok_or(malformed)
                }
                (Some("="), Some("["), _, _) => Err(malformed),
                (Some("="), _, _, _) => tlc_config_value_end(tokens, position + 1).ok_or(malformed),
                _ => Err(malformed),
            }
        }
    }
}

fn tlc_config_invariants(config: &str) -> Result<Vec<&str>, TlcConfigError> {
    let tokens = tlc_config_tokens(config)?;
    let mut invariants = Vec::new();
    let mut single_valued = BTreeSet::new();
    let mut position = 0_usize;
    while let Some(keyword) = tokens.get(position) {
        let refusal = |defect| TlcConfigError {
            line: keyword.line,
            defect,
        };
        position += 1;
        match tlc_statement(keyword.image) {
            None => return Err(refusal(TlcConfigDefect::ExpectedKeyword)),
            Some(TlcStatement::SingleValue) => match tokens.get(position) {
                None => return Err(refusal(TlcConfigDefect::MissingOperand)),
                Some(_) if single_valued.insert(keyword.image) => position += 1,
                Some(_) => return Err(refusal(TlcConfigDefect::RepeatedKeyword)),
            },
            Some(TlcStatement::CheckDeadlock) => {
                match tokens.get(position).map(|token| token.image) {
                    None => return Err(refusal(TlcConfigDefect::MissingOperand)),
                    Some("TRUE" | "FALSE") if single_valued.insert(keyword.image) => position += 1,
                    Some("TRUE" | "FALSE") => {
                        return Err(refusal(TlcConfigDefect::RepeatedKeyword));
                    }
                    Some(_) => return Err(refusal(TlcConfigDefect::ExpectedBoolean)),
                }
            }
            Some(TlcStatement::Invariants) => {
                while let Some(name) = tokens
                    .get(position)
                    .filter(|token| tlc_statement(token.image).is_none())
                {
                    match (name.kind, name.image.bytes().any(|byte| byte.is_ascii_alphabetic())) {
                        (TlcTokenKind::Word, true) => invariants.push(name.image),
                        _ => {
                            return Err(TlcConfigError {
                                line: name.line,
                                defect: TlcConfigDefect::InvalidInvariantName,
                            });
                        }
                    }
                    position += 1;
                }
            }
            Some(TlcStatement::OtherNames) => {
                position += tokens[position..]
                    .iter()
                    .take_while(|token| tlc_statement(token.image).is_none())
                    .count();
            }
            Some(TlcStatement::Constants) => {
                while tokens
                    .get(position)
                    .is_some_and(|token| tlc_statement(token.image).is_none())
                {
                    position = tlc_config_constant_end(&tokens, position)?;
                }
            }
        }
    }
    Ok(invariants)
}

#[test]
fn tlc_configuration_grammar_yields_the_declared_invariants() {
    let cases: [(&str, &[&str]); 24] = [
        ("INVARIANT TypeOK", &["TypeOK"]),
        ("INVARIANT A B C\n", &["A", "B", "C"]),
        ("INVARIANT\n    A\n    B\n", &["A", "B"]),
        ("INVARIANTS A\nB\n\n\tC\r\n", &["A", "B", "C"]),
        ("INVARIANTS\n  A\nINVARIANT B\nINVARIANTS C D", &["A", "B", "C", "D"]),
        ("INVARIANT A\nINIT Init\nNEXT Next\nINVARIANT B", &["A", "B"]),
        ("INVARIANTS A B PROPERTIES Live Fair INVARIANT C", &["A", "B", "C"]),
        ("CONSTRAINT Bound\nACTION_CONSTRAINTS Step Other\nINVARIANT A\nPROPERTY Live", &["A"]),
        ("INVARIANT A\nCONSTANTS\n  N = 3\n  S = {a, b, {c}}\n  Empty = {}\nINVARIANT B", &[
            "A", "B",
        ]),
        ("INVARIANT A CHECK_DEADLOCK FALSE INVARIANT B", &["A", "B"]),
        (
            "SPECIFICATION Spec\nVIEW View\nSYMMETRY Perms\nTYPE T\nTYPE_CONSTRAINT C\nINVARIANT A",
            &["A"],
        ),
        (
            "\\* INVARIANT Hidden\nINVARIANT A \\* trailing INVARIANT Hidden\n(* INVARIANT (* \
             nested *) Hidden *)\n  B",
            &["A", "B"],
        ),
        ("(*\nINVARIANT Hidden\n*)INVARIANT(* tight *)A", &["A"]),
        ("SPECIFICATION INVARIANT\nINVARIANT A", &["A"]),
        ("CONSTANT Limit <- INVARIANT\nINVARIANT A", &["A"]),
        ("CONSTANT Mode = INVARIANTS INVARIANT A", &["A"]),
        ("CONSTANT Tag = \"INVARIANT (* \\\" x\"\nINVARIANT A", &["A"]),
        ("CONSTANTS S={a,b}N=3 Op(1,{2})=x\nINVARIANT A", &["A"]),
        ("CONSTANT N = [Mod] 3\nM <- [Mod] Replacement\nA!B!C <- D\nINVARIANT A", &["A"]),
        ("INVARIANT\nINIT Init", &[]),
        ("INVARIANTS", &[]),
        ("SPECIFICATION Spec\nCHECK_DEADLOCK TRUE", &[]),
        ("", &[]),
        ("INVARIANT INVARIANTX INIT_ NEXT1 Type_OK 2PhaseOK _hidden", &[
            "INVARIANTX",
            "INIT_",
            "NEXT1",
            "Type_OK",
            "2PhaseOK",
            "_hidden",
        ]),
    ];
    for (config, expected) in cases {
        assert_eq!(tlc_config_invariants(config).as_deref(), Ok(expected), "{config:?}");
    }
}

#[test]
fn tlc_configuration_grammar_refuses_malformed_input() {
    use TlcConfigDefect as Defect;
    let cases = [
        ("INVARIANT A\n(* open (* nested *)", 2, Defect::UnterminatedComment),
        ("CONSTANT Tag = \"open\nINVARIANT A", 1, Defect::UnterminatedString),
        ("INVARIANT A; B", 1, Defect::UnexpectedCharacter),
        ("INVARIANT A\n  \u{2227} B", 2, Defect::UnexpectedCharacter),
        ("TypeOK", 1, Defect::ExpectedKeyword),
        ("SPECIFICATION Spec Extra", 1, Defect::ExpectedKeyword),
        ("\n\nINIT", 3, Defect::MissingOperand),
        ("CHECK_DEADLOCK", 1, Defect::MissingOperand),
        ("INIT Init\nNEXT Next\nINIT Other", 3, Defect::RepeatedKeyword),
        ("CHECK_DEADLOCK TRUE\nCHECK_DEADLOCK FALSE", 2, Defect::RepeatedKeyword),
        ("CHECK_DEADLOCK maybe", 1, Defect::ExpectedBoolean),
        ("INVARIANT 42", 1, Defect::InvalidInvariantName),
        ("INVARIANT A,\n B", 1, Defect::InvalidInvariantName),
        ("INVARIANT\n\"A\"", 2, Defect::InvalidInvariantName),
        ("INVARIANT A!B", 1, Defect::InvalidInvariantName),
        ("CONSTANT N", 1, Defect::MalformedConstant),
        ("CONSTANT N <-", 1, Defect::MalformedConstant),
        ("CONSTANT A!", 1, Defect::MalformedConstant),
        ("CONSTANT\n N = {1, 2\nINVARIANT A", 2, Defect::MalformedConstant),
        ("CONSTANT N = [Mod 3", 1, Defect::MalformedConstant),
        ("CONSTANT Op(1, 2 = 3", 1, Defect::MalformedConstant),
    ];
    for (config, line, defect) in cases {
        assert_eq!(
            tlc_config_invariants(config),
            Err(TlcConfigError { line, defect }),
            "{config:?}"
        );
    }
}

#[test]
fn gc_protection_base_configurations_declare_exactly_the_eighteen_base_invariants() {
    const BASE_INVARIANTS: [&str; 18] = [
        "TypeInvariant",
        "PublicationBeforeReclamation",
        "ExactOldPinsProtected",
        "DurableExactGenerationProtected",
        "PublishedClosureProtected",
        "KnownPublicationClosureProtected",
        "KnownExactGenerationProtected",
        "OperationRootRoleQuotient",
        "KnownWalOrthogonal",
        "OverlapWitnessHistorySound",
        "ActiveObjectProtection",
        "CoupledAliasProtected",
        "WalFrontierProtected",
        "WalReclaimableMinimum",
        "ObjectOnlyPinDoesNotBlockWal",
        "WalAdmissionBounded",
        "CrashPhaseQuiescence",
        "CrashRetryAuthority",
    ];
    let model = include_str!("../../../formal/tlaplus/stack_safe_pda/RSpaceGcProtection.tla");
    for (name, config) in [
        (
            "RSpaceGcProtection.cfg",
            include_str!("../../../formal/tlaplus/stack_safe_pda/RSpaceGcProtection.cfg"),
        ),
        (
            "RSpaceGcProtectionKnown.cfg",
            include_str!("../../../formal/tlaplus/stack_safe_pda/RSpaceGcProtectionKnown.cfg"),
        ),
    ] {
        assert_eq!(
            tlc_config_invariants(config).as_deref(),
            Ok(BASE_INVARIANTS.as_slice()),
            "{name} does not declare exactly the eighteen base invariants",
        );
    }
    for invariant in BASE_INVARIANTS {
        assert!(
            model.contains(&format!("{invariant} ==")),
            "the GC protection model does not define {invariant}",
        );
    }
}

fn tlc_layout_case() -> impl Strategy<Value = (String, Vec<String>)> {
    const SINGLE_VALUED: [&str; 7] =
        ["INIT", "NEXT", "SPECIFICATION", "VIEW", "SYMMETRY", "TYPE", "TYPE_CONSTRAINT"];
    let gap = || {
        prop::sample::select(vec![
            " ",
            "\n",
            "\n    ",
            "\t",
            "\r\n",
            " \\* INVARIANT Hidden\n",
            " (* INVARIANT (* nested *) Hidden *) ",
        ])
    };
    let name = "[A-Za-z][A-Za-z0-9_]{0,6}".prop_map(|suffix| format!("Inv{suffix}"));
    let statement = (0_usize..7, prop::collection::vec((name, gap()), 0..4), gap());
    prop::collection::vec(statement, 0..6).prop_map(|statements| {
        let mut config = String::new();
        let mut expected = Vec::new();
        let mut single_valued = SINGLE_VALUED.iter();
        for (kind, names, keyword_gap) in statements {
            let keyword = match kind {
                0 => "INVARIANT",
                1 => "INVARIANTS",
                2 => "PROPERTIES",
                3 => "CONSTRAINT",
                4 => "ACTION_CONSTRAINTS",
                5 => "CONSTANTS",
                _ => single_valued
                    .next()
                    .expect("a case holds fewer statements than single-valued keywords"),
            };
            config.push_str(keyword);
            config.push_str(keyword_gap);
            match kind {
                0..=4 => {
                    for (name, name_gap) in names {
                        config.push_str(&name);
                        config.push_str(name_gap);
                        if kind < 2 {
                            expected.push(name);
                        }
                    }
                }
                5 => {
                    for (entry, (name, name_gap)) in names.into_iter().enumerate() {
                        let rendered = match entry % 3 {
                            0 => format!("{name} = 3"),
                            1 => format!("{name}!{name} <- INVARIANT"),
                            _ => format!("{name}(1, {{}}) = [{name}] {{{name}, \"INVARIANT\"}}"),
                        };
                        config.push_str(&rendered);
                        config.push_str(name_gap);
                    }
                }
                _ => {
                    config.push_str(
                        names
                            .first()
                            .map_or("INVARIANTS", |(name, _)| name.as_str()),
                    );
                    config.push_str(keyword_gap);
                }
            }
        }
        (config, expected)
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn tlc_configuration_invariants_do_not_depend_on_layout((config, expected) in tlc_layout_case()) {
        let expected = expected.iter().map(String::as_str).collect::<Vec<_>>();
        prop_assert_eq!(tlc_config_invariants(&config), Ok(expected), "{:?}", config);
    }
}
