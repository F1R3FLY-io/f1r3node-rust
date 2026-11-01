"""Apply one receive-accelerator negative control to an exported tree.

Harness controls RAC-RS-01 to RAC-RS-16 and RAC-RS-25 change the reference oracle or its adoption
rule. RAC-RS-01 to RAC-RS-15 are the executable analogues of the Rocq refutations RAC-MUT-01 to
RAC-MUT-15, RAC-RS-16 leaves a removal unmaintained, and RAC-RS-25 makes adoption ignore the bound
semantic manifest. They are concrete witnesses that each named test observes the property it owns,
not production evidence. Production controls RAC-RS-17 to RAC-RS-24 change production source that
the derived receive relation depends on and are executable refinement evidence. Production
accelerator controls from RAC-RS-26 on fault the production accelerator and are killed by its
production witnesses. Every control must make its named test fail. The generator refuses to mutate
the checkout that contains it.
"""

import argparse
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REFERENCE = "rspace++/tests/support/receive_occurrence_reference.rs"
ADOPTION = "rspace++/tests/support/receive_accelerator_adoption.rs"
HOT_STORE = "rspace++/src/rspace/hot_store.rs"
RSPACE = "rspace++/src/rspace/rspace.rs"
SPEC = "rspace++/tests/receive_accelerator_spec.rs"
BINDING = "rspace++/tests/persistence/receive_accelerator_manifest_binding.rs"
RECEIVE_ACCELERATOR = "rspace++/src/rspace/receive_accelerator.rs"
RECORD_INDEX = "rspace++/src/rspace/record_index.rs"
PRODUCTION_SPEC = "rspace++/tests/receive_accelerator_production_spec.rs"
HARNESS = "concrete_witness"
PRODUCTION = "executable_refinement"

MUTATIONS = {
    "RAC-RS-01": {
        "mode": HARNESS,
        "formal": ("RAC-MUT-01",),
        "source": REFERENCE,
        "description": "a receive identifier resolves by its hash bucket alone",
        "before": "        bucket.iter().find(|candidate| *candidate == wanted)\n",
        "after": "        bucket.first()\n",
        "test_file": SPEC,
        "test": "colliding_buckets_resolve_by_full_bytes",
    },
    "RAC-RS-02": {
        "mode": HARNESS,
        "formal": ("RAC-MUT-02",),
        "source": REFERENCE,
        "description": (
            "a receive identifier is the record's ordinal in its group, so a maintained removal "
            "deletes the catalog entry at ordinal 0 instead of the named target"
        ),
        "before": "            held.remove(target);\n",
        "after": "            let _ = held.pop_first();\n",
        "test_file": SPEC,
        "test": "incremental_nonfinal_removal_equals_rebuild",
    },
    "RAC-RS-03": {
        "mode": HARNESS,
        "formal": ("RAC-MUT-03",),
        "source": REFERENCE,
        "description": "an absent accelerator answers the empty set",
        "before": "            AcceleratorClass::Absent => Answer::Fallback(AcceleratorClass::Absent),\n",
        "after": "            AcceleratorClass::Absent => Answer::Exact(self.lookup(channel)),\n",
        "test_file": SPEC,
        "test": "reference_reads_equal_derivation_in_every_accelerator_state",
    },
    "RAC-RS-04": {
        "mode": HARNESS,
        "formal": ("RAC-MUT-04",),
        "source": REFERENCE,
        "description": "a stale accelerator answers from its old entries",
        "before": "            AcceleratorClass::Stale => Answer::Fallback(AcceleratorClass::Stale),\n",
        "after": "            AcceleratorClass::Stale => Answer::Exact(self.lookup(channel)),\n",
        "test_file": SPEC,
        "test": "stale_accelerators_fall_back_after_every_mutation",
    },
    "RAC-RS-05": {
        "mode": HARNESS,
        "formal": ("RAC-MUT-05",),
        "source": REFERENCE,
        "description": (
            "the classifier ignores the generation, so an accelerator of a foreign generation is "
            "taken for a current or stale one"
        ),
        "before": (
            "        let foreign_generation = built.revision.generation != world.revision.generation;\n"
        ),
        "after": "        let foreign_generation = false;\n",
        "test_file": SPEC,
        "test": "foreign_generation_or_manifest_accelerators_are_never_adopted",
    },
    "RAC-RS-06": {
        "mode": HARNESS,
        "formal": ("RAC-MUT-06",),
        "source": REFERENCE,
        "description": "an incomplete accelerator is classified as coherent and answers",
        "before": "            (false, false, true) => AcceleratorClass::Incomplete,\n",
        "after": "            (false, false, true) => AcceleratorClass::Coherent,\n",
        "test_file": SPEC,
        "test": "unloaded_history_groups_make_the_accelerator_incomplete",
    },
    "RAC-RS-07": {
        "mode": HARNESS,
        "formal": ("RAC-MUT-07",),
        "source": REFERENCE,
        "description": "a repeated channel keeps only its first position",
        "before": "        .filter(|(_, image)| image.as_slice() == channel)\n",
        "after": (
            "        .filter(|(_, image)| image.as_slice() == channel)\n"
            "        .take(1)\n"
        ),
        "test_file": SPEC,
        "test": "repeated_channels_yield_one_occurrence_with_every_position",
    },
    "RAC-RS-08": {
        "mode": HARNESS,
        "formal": ("RAC-MUT-08",),
        "source": REFERENCE,
        "description": (
            "the observation records an installed receive as resting, so it shares one target with "
            "a byte-identical resting record"
        ),
        "before": "                target_of_continuation(ReceiveKind::Installed, key, continuation);\n",
        "after": "                target_of_continuation(ReceiveKind::Resting, key, continuation);\n",
        "test_file": SPEC,
        "test": "install_and_generation_uninstall_equal_rebuild",
    },
    "RAC-RS-09": {
        "mode": HARNESS,
        "formal": ("RAC-MUT-09",),
        "source": REFERENCE,
        "description": (
            "the derivation keeps only the first receive of each group's ordered lookup, so it "
            "depends on construction order"
        ),
        "before": (
            "        let targets = observation.targets(&group);\n"
            "        for channel in distinct_channels(&group) {\n"
        ),
        "after": (
            "        let targets: BTreeSet<ReceiveTarget> =\n"
            "            observation.ordered_targets(&group).into_iter().take(1).collect();\n"
            "        for channel in distinct_channels(&group) {\n"
        ),
        "test_file": SPEC,
        "test": "construction_order_does_not_change_the_relation_or_ids",
    },
    "RAC-RS-10": {
        "mode": HARNESS,
        "formal": ("RAC-MUT-10",),
        "source": REFERENCE,
        "description": "the automaton artifact keeps the previous revision",
        "before": "        next.automaton.revision = after.revision.clone();\n",
        "after": "        next.automaton.revision = before.revision.clone();\n",
        "test_file": SPEC,
        "test": "catalog_occurrence_and_automaton_publish_at_one_revision",
    },
    "RAC-RS-11": {
        "mode": HARNESS,
        "formal": ("RAC-MUT-11",),
        "source": REFERENCE,
        "description": (
            "a failed update publishes its partially updated catalog while the accelerator stays "
            "marked current"
        ),
        "before": "        self.check_fault(FaultStage::Catalog)?;\n",
        "after": (
            "        self.phase = AcceleratorPhase::Built(next.clone());\n"
            "        self.check_fault(FaultStage::Catalog)?;\n"
        ),
        "test_file": SPEC,
        "test": "failed_update_leaves_store_and_accelerator_unchanged",
    },
    "RAC-RS-12": {
        "mode": HARNESS,
        "formal": ("RAC-MUT-12",),
        "source": REFERENCE,
        "description": (
            "the accelerator's occurrences carry a target that differs from the derived one, so "
            "the receive identity depends on whether the accelerator answered"
        ),
        "before": (
            "            group: target.group.clone(),\n"
            "            positions,\n"
            "            target: target.clone(),\n"
        ),
        "after": (
            "            group: target.group.clone(),\n"
            "            positions,\n"
            "            target: ReceiveTarget {\n"
            "                source: Vec::new(),\n"
            "                ..target.clone()\n"
            "            },\n"
        ),
        "test_file": SPEC,
        "test": "enabled_and_disabled_acceleration_select_identically",
    },
    "RAC-RS-13": {
        "mode": HARNESS,
        "formal": ("RAC-MUT-13",),
        "source": REFERENCE,
        "description": "an alias insertion replaces the first source",
        "before": "                Some(_) => (Vec::new(), Vec::new()),\n",
        "after": "                Some(held) => (vec![held], vec![target.clone()]),\n",
        "test_file": SPEC,
        "test": "first_source_representative_survives_alias_insertion",
    },
    "RAC-RS-14": {
        "mode": HARNESS,
        "formal": ("RAC-MUT-14",),
        "source": REFERENCE,
        "description": "a candidate commits without the revision check",
        "before": "    let current = id.revision == world.revision;\n",
        "after": "    let current = true;\n",
        "test_file": SPEC,
        "test": "stale_accelerator_candidates_never_commit",
    },
    "RAC-RS-15": {
        "mode": HARNESS,
        "formal": ("RAC-MUT-15",),
        "source": REFERENCE,
        "description": "the target identity is the continuation body, which erases the persist flag and peeks",
        "before": "        identity: record.identity().to_vec(),\n",
        "after": "        identity: record.body().to_vec(),\n",
        "test_file": SPEC,
        "test": "metadata_distinct_records_keep_distinct_ids",
    },
    "RAC-RS-16": {
        "mode": HARNESS,
        "formal": ("RAC-TLA-04",),
        "source": REFERENCE,
        "description": "a removal leaves its occurrences in a current accelerator",
        "before": "                true => (vec![target.clone()], Vec::new()),\n",
        "after": "                true => (Vec::new(), Vec::new()),\n",
        "test_file": SPEC,
        "test": "reference_answers_never_include_absent_receives",
    },
    "RAC-RS-17": {
        "mode": PRODUCTION,
        "formal": ("RAC-RC-15",),
        "source": HOT_STORE,
        "description": "put_continuation keeps the group's epoch after an insertion",
        "before": (
            "        if inserted {\n"
            "            state.bump_continuation_revision(channels);\n"
        ),
        "after": "        if inserted {\n",
        "test_file": SPEC,
        "test": "incremental_insertion_equals_rebuild",
    },
    "RAC-RS-18": {
        "mode": PRODUCTION,
        "formal": ("RAC-RC-18",),
        "source": HOT_STORE,
        "description": "install_continuation keeps the group's epoch",
        "before": (
            "        state.continuations.install(channels, wc);\n"
            "        state.bump_continuation_revision(channels);\n"
        ),
        "after": "        state.continuations.install(channels, wc);\n",
        "test_file": SPEC,
        "test": "install_and_generation_uninstall_equal_rebuild",
    },
    "RAC-RS-19": {
        "mode": PRODUCTION,
        "formal": ("RAC-RC-16",),
        "source": HOT_STORE,
        "description": "remove_continuation keeps the group's epoch",
        "before": (
            "        if removed.is_some() {\n"
            "            state.bump_continuation_revision(channels);\n"
            "        }\n"
        ),
        "after": "        if removed.is_some() {}\n",
        "test_file": SPEC,
        "test": "incremental_nonfinal_removal_equals_rebuild",
    },
    "RAC-RS-20": {
        "mode": PRODUCTION,
        "formal": ("RAC-RC-21",),
        "source": HOT_STORE,
        "description": "an accepted ordinary removal keeps the group's epoch",
        "before": (
            '                .expect("a validated continuation path must remain present");\n'
            "            state.bump_continuation_revision(&transition.channels);\n"
        ),
        "after": '                .expect("a validated continuation path must remain present");\n',
        "test_file": SPEC,
        "test": "equal_versions_imply_equal_group_relation",
    },
    "RAC-RS-21": {
        "mode": PRODUCTION,
        "formal": ("RAC-RC-20",),
        "source": HOT_STORE,
        "description": "clear keeps the generation",
        "before": (
            "        state.invalidate_journal_base();\n"
            "        state.bump_continuation_generation();\n"
        ),
        "after": "        state.invalidate_journal_base();\n",
        "test_file": SPEC,
        "test": "clear_and_set_state_invalidate_every_held_version_and_rebuild_equal",
    },
    "RAC-RS-22": {
        "mode": PRODUCTION,
        "formal": ("RAC-RC-13",),
        "source": HOT_STORE,
        "description": "set_state copies the old base epoch into its replacement",
        "before": "        replacement.durable_journal = state.durable_journal.take();\n",
        "after": (
            "        replacement.durable_journal = state.durable_journal.take();\n"
            "        replacement.continuation_base_epoch = Arc::clone(&state.continuation_base_epoch);\n"
        ),
        "test_file": SPEC,
        "test": "ids_from_a_prior_revision_or_generation_never_resolve",
    },
    "RAC-RS-23": {
        "mode": PRODUCTION,
        "formal": ("RAC-RC-25",),
        "source": HOT_STORE,
        "description": "the stale-ticket check accepts a changed version",
        "before": (
            "            if expected.address.version.is_none() || expected.address.version != current_version {\n"
        ),
        "after": "            if expected.address.version.is_none() {\n",
        "test_file": SPEC,
        "test": "stale_accelerator_candidates_never_commit",
    },
    "RAC-RS-24": {
        "mode": PRODUCTION,
        "formal": ("RAC-RC-24",),
        "source": RSPACE,
        "description": "the retained-set automaton cache ignores the version",
        "before": (
            "                    cached.version == version &&\n"
            "                        cached.automaton.candidate_count() == candidates.len()\n"
        ),
        "after": "                        cached.automaton.candidate_count() == candidates.len()\n",
        "test_file": SPEC,
        "test": "same_count_group_replacement_never_reuses_a_stale_automaton",
    },
    "RAC-RS-25": {
        "mode": HARNESS,
        "formal": ("RAC-RC-20", "RAC-TLA-08"),
        "source": ADOPTION,
        "description": "adoption ignores the bound semantic manifest",
        "before": "    let bound_matches = bound_manifest == current_manifest;\n",
        "after": (
            "    let bound_matches = {\n"
            "        let _ = (bound_manifest, current_manifest);\n"
            "        true\n"
            "    };\n"
        ),
        "test_file": BINDING,
        "test": "persisted_accelerator_adoption_requires_the_bound_semantic_manifest",
    },
    "RAC-RS-26": {
        "mode": PRODUCTION,
        "formal": ("RAC-RC-10", "RAC-RC-12"),
        "source": RECEIVE_ACCELERATOR,
        "description": "resolve accepts a target of equal identity length (production analogue of RAC-RS-01)",
        "before": "        (found == self.target).then_some(found)\n",
        "after": (
            "        let accepted = found.identity_bytes().len() == self.target.identity_bytes().len();\n"
            "        accepted.then_some(found)\n"
        ),
        "test_file": PRODUCTION_SPEC,
        "test": "production_receive_ids_resolve_only_to_byte_identical_records",
    },
    "RAC-RS-27": {
        "mode": PRODUCTION,
        "formal": ("RAC-RC-16",),
        "source": HOT_STORE,
        "description": (
            "a removal of the first ordered record is treated as final and clears the published group "
            "(production analogue of RAC-RS-02)"
        ),
        "before": "                Some(path) => state.continuations.remove_path(channels, &path).map(|_| ()),\n",
        "after": (
            "                Some(path) => {\n"
            "                    let removed = state.continuations.remove_path(channels, &path).map(|_| ());\n"
            "                    let prefix = RhoPathV1::continuation_prefix(channels);\n"
            "                    let treated_as_final = match (removed_index, state.continuations.receive_index()) {\n"
            "                        (0, Some(published)) => {\n"
            "                            let mut published = published.clone();\n"
            "                            published.remove_group(&prefix).ok().map(|()| published)\n"
            "                        }\n"
            "                        _ => None,\n"
            "                    };\n"
            "                    if let Some(published) = treated_as_final {\n"
            "                        state.continuations.publish_receive_index(published);\n"
            "                    }\n"
            "                    removed\n"
            "                }\n"
        ),
        "test_file": PRODUCTION_SPEC,
        "test": "production_nonfinal_removals_maintain_the_published_accelerator",
    },
    "RAC-RS-28": {
        "mode": PRODUCTION,
        "formal": ("RAC-RC-22", "RAC-TLA-05"),
        "source": HOT_STORE,
        "description": "an absent accelerator answers the exact empty set (production analogue of RAC-RS-03)",
        "before": (
            "            Some(handle) => state.read_receive_occurrences(handle, channel),\n"
            "            None => ReceiveAnswer::Fallback(state.vacant_receive_class()),\n"
        ),
        "after": (
            "            Some(handle) => state.read_receive_occurrences(handle, channel),\n"
            "            None => {\n"
            "                let mut vacant = RSpaceContinuationStore::<C, P, K>::new();\n"
            "                match (state.vacant_receive_class(), vacant.rebuild_receive_index()) {\n"
            "                    (ReceiveAcceleratorClass::Absent, true) => {\n"
            "                        ReceiveAcceleratorHandle::new(state.receive_revision(), vacant)\n"
            '                            .expect("an empty rebuilt continuation store is indexed")\n'
            "                            .materialize(channel)\n"
            "                    }\n"
            "                    (class, _) => ReceiveAnswer::Fallback(class),\n"
            "                }\n"
            "            }\n"
        ),
        "test_file": PRODUCTION_SPEC,
        "test": "production_reads_equal_derivation_in_every_accelerator_state",
    },
    "RAC-RS-29": {
        "mode": PRODUCTION,
        "formal": ("RAC-TLA-06",),
        "source": RECEIVE_ACCELERATOR,
        "description": (
            "handle classification ignores the store revision, so a stale handle answers from its old index "
            "(production analogue of RAC-RS-04)"
        ),
        "before": "            (true, false) => Some(ReceiveAcceleratorClass::Stale),\n",
        "after": "            (true, false) => None,\n",
        "test_file": PRODUCTION_SPEC,
        "test": "production_held_handles_fall_back_after_every_mutation",
    },
    "RAC-RS-30": {
        "mode": PRODUCTION,
        "formal": ("RAC-RC-20",),
        "source": RECEIVE_ACCELERATOR,
        "description": (
            "handle classification ignores the generation, so a handle of a foreign generation is taken for "
            "a current or stale one (production analogue of RAC-RS-05)"
        ),
        "before": "        match (self.revision.shares_generation(current), self.revision == *current) {\n",
        "after": "        match (true, self.revision == *current) {\n",
        "test_file": PRODUCTION_SPEC,
        "test": "production_clear_and_set_state_retire_held_handles_and_rebuilds_equal_the_derivation",
    },
    "RAC-RS-31": {
        "mode": PRODUCTION,
        "formal": ("RAC-TLA-07",),
        "source": RECEIVE_ACCELERATOR,
        "description": (
            "join groups outside the handle's loaded scope are skipped, so an unloaded join group leaves the channel "
            "answering (production analogue of RAC-RS-06)"
        ),
        "before": (
            "        scope\n"
            "            .is_loaded_prefix(&RhoPathV1::continuation_prefix(group))\n"
            "            .then_some(())\n"
            "            .ok_or(())\n"
        ),
        "after": (
            "        let _ = (scope, group);\n"
            "        Ok(())\n"
        ),
        "test_file": PRODUCTION_SPEC,
        "test": "production_scope_gaps_fall_back_as_incomplete_until_the_channel_is_hydrated",
    },
    "RAC-RS-32": {
        "mode": PRODUCTION,
        "formal": ("RAC-TLA-07",),
        "source": RECEIVE_ACCELERATOR,
        "description": (
            "an installed-only group outside the handle's loaded scope is skipped, so the channel answers without it "
            "(production analogue of RAC-CSMUT-02)"
        ),
        "before": "                None => Err(ReceiveAcceleratorClass::Incomplete),\n",
        "after": "                None => Ok(()),\n",
        "test_file": PRODUCTION_SPEC,
        "test": "production_scope_gaps_fall_back_as_incomplete_until_the_channel_is_hydrated",
    },
    "RAC-RS-33": {
        "mode": PRODUCTION,
        "formal": ("RAC-TLA-07",),
        "source": RECEIVE_ACCELERATOR,
        "description": (
            "a channel whose joins are not loaded is treated as having no join groups, so it answers instead of "
            "falling back as incomplete (a production control with no harness analogue)"
        ),
        "before": (
            "        Some(Ok(())) => None,\n"
            "        Some(Err(())) | None => Some(ReceiveAcceleratorClass::Incomplete),\n"
        ),
        "after": (
            "        Some(Ok(())) | None => None,\n"
            "        Some(Err(())) => Some(ReceiveAcceleratorClass::Incomplete),\n"
        ),
        "test_file": PRODUCTION_SPEC,
        "test": "production_scope_gaps_fall_back_as_incomplete_until_the_channel_is_hydrated",
    },
    "RAC-RS-34": {
        "mode": PRODUCTION,
        "formal": ("RAC-RC-04",),
        "source": RECEIVE_ACCELERATOR,
        "description": "a repeated channel keeps only its first position (production analogue of RAC-RS-07)",
        "before": "            .map(|(index, _)| RhoPathReceivePosition::at_index(index)),\n",
        "after": (
            "            .map(|(index, _)| RhoPathReceivePosition::at_index(index))\n"
            "            .take(1),\n"
        ),
        "test_file": PRODUCTION_SPEC,
        "test": "production_repeated_channels_yield_one_occurrence_per_target",
    },
    "RAC-RS-35": {
        "mode": PRODUCTION,
        "formal": ("RAC-RC-11",),
        "source": RECEIVE_ACCELERATOR,
        "description": (
            "an installed target is dropped beside a resting record with the same identity and source bytes "
            "(production analogue of RAC-RS-08)"
        ),
        "before": (
            "        targets.extend(installed.map(|entry| ReceiveTarget {\n"
            "            prefix: Arc::clone(&shared_prefix),\n"
            "            record: ReceiveRecord::Installed(Arc::clone(entry)),\n"
            "        }));\n"
            "        targets.extend(resting.into_iter().map(|record| ReceiveTarget {\n"
            "            prefix: Arc::clone(&shared_prefix),\n"
            "            record: ReceiveRecord::Resting(record),\n"
            "        }));\n"
        ),
        "after": (
            "        let installed_target = installed.map(|entry| ReceiveTarget {\n"
            "            prefix: Arc::clone(&shared_prefix),\n"
            "            record: ReceiveRecord::Installed(Arc::clone(entry)),\n"
            "        });\n"
            "        let resting_targets: Vec<ReceiveTarget<C, P, K>> = resting\n"
            "            .into_iter()\n"
            "            .map(|record| ReceiveTarget {\n"
            "                prefix: Arc::clone(&shared_prefix),\n"
            "                record: ReceiveRecord::Resting(record),\n"
            "            })\n"
            "            .collect();\n"
            "        targets.extend(installed_target.filter(|candidate| {\n"
            "            !resting_targets.iter().any(|twin| {\n"
            "                twin.identity_bytes() == candidate.identity_bytes() &&\n"
            "                    twin.source_bytes() == candidate.source_bytes()\n"
            "            })\n"
            "        }));\n"
            "        targets.extend(resting_targets);\n"
        ),
        "test_file": PRODUCTION_SPEC,
        "test": "production_metadata_distinct_records_keep_distinct_ids",
    },
}


def apply(name, tree):
    if tree.resolve() == ROOT:
        raise SystemExit("refusing to mutate the checkout that contains this generator")
    mutation = MUTATIONS[name]
    target = tree / mutation["source"]
    source = target.read_text(encoding="utf-8")
    if source.count(mutation["before"]) != 1:
        raise SystemExit(f"{name}: source anchor is absent or ambiguous in {mutation['source']}")
    target.write_text(source.replace(mutation["before"], mutation["after"], 1), encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("list")
    applied = commands.add_parser("apply")
    applied.add_argument("name", choices=sorted(MUTATIONS))
    applied.add_argument("tree", type=Path)
    args = parser.parse_args()
    if args.command == "list":
        for name, mutation in MUTATIONS.items():
            print("\t".join((name, mutation["source"], mutation["test_file"], mutation["test"])))
    else:
        apply(args.name, args.tree)


if __name__ == "__main__":
    sys.exit(main())
