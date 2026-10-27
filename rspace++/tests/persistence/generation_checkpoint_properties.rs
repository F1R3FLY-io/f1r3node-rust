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
        "IR", "CR", "LT", "DC", "CL", "RV", "TS",
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
    ];
    let rocq_source = [
        include_str!(
            "../../../formal/rocq/stack_safe_pda/theories/GenerationCheckpointPublication.v"
        ),
        include_str!("../../../formal/rocq/stack_safe_pda/theories/IncrementalLedgerTrial.v"),
        include_str!("../../../formal/rocq/stack_safe_pda/theories/IncrementalDomainComposition.v"),
        include_str!("../../../formal/rocq/stack_safe_pda/theories/CreditedClosureLedger.v"),
        include_str!("../../../formal/rocq/stack_safe_pda/theories/RetentionV2Format.v"),
        include_str!("../../../formal/rocq/stack_safe_pda/theories/RetentionV2Editor.v"),
        include_str!("../../../formal/rocq/stack_safe_pda/theories/TrustedSuccessorCapability.v"),
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

    for invariant in tla_configs
        .iter()
        .flat_map(|config| config.lines())
        .filter_map(|line| line.strip_prefix("INVARIANT "))
    {
        assert!(
            cited_tla.contains(invariant),
            "base TLA+ invariant {invariant} is absent from the exhaustive derivation matrix",
        );
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

    let executable_sources = [
        include_str!("checkpoint_codec.rs"),
        include_str!("checkpoint_wal.rs"),
        include_str!("cow_reachability_ledger_properties.rs"),
        include_str!("durable_catalog.rs"),
        include_str!("durable_publication.rs"),
        include_str!("durable_recovery.rs"),
        include_str!("durable_rspace_authority.rs"),
        include_str!("generation_checkpoint_properties.rs"),
        include_str!("incremental_reachability_properties.rs"),
        include_str!("retention_v2_editor_properties.rs"),
        include_str!("retention_v2_format_properties.rs"),
        include_str!("trusted_successor_properties.rs"),
        include_str!("wal_codec.rs"),
        include_str!("../hot_store/journal_commit.rs"),
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
