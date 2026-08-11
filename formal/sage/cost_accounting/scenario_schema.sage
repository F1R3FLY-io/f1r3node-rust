from sage.all import Integer, Set, ZZ, vector


CLASSIFICATIONS = [
    "confirmed_safe",
    "bisimilar",
    "projection_risk",
    "assumption_counterexample",
    "proof_or_model_strengthening",
    "needs_source_audit",
    "confirmed_current_bug",
]

THREAT_FAMILIES = [
    "producer_routing",
    "concurrency_schedule",
    "replay_authentication",
    "settlement",
    "slashing_composition",
    "resource_exhaustion",
    "search_governance",
    "exploit_campaign",
    "differential_replay",
    "stateful_campaign",
    "production_path_diff",
    "source_corpus",
    "exploit_cross_product",
    "adversarial_budget",
    "adversarial_replay",
    "adversarial_settlement",
    "adversarial_slashing",
    "adversarial_lifecycle",
    "adversarial_source_corpus",
    "property_invariant",
    "negative_authentication",
    "source_shape",
    "cross_deploy",
    "scheduler_interleaving",
    "cache_resource",
    "production_differential",
    "production_rholang_eval",
    "production_replay",
    "production_settlement",
    "production_scheduler",
    "production_resource",
    "production_eval_replay",
    "production_source_corpus",
    "production_auth_composition",
    "production_state_root",
    "production_external_boundary",
    "production_error_boundary",
    "generative_semantic",
    "semantic_metamorphic",
    "external_service_replay",
    "coverage_adequacy",
    "semantic_cross_product",
    "production_mutation",
    "corpus_semantic",
    "grammar_mutation",
    "differential_oracle",
    "external_service_matrix",
    "casper_security_matrix",
    "runtime_trace_interleaving",
    "hybrid_fuzz_security",
    "hybrid_fuzz_runtime",
    "hybrid_fuzz_replay",
    "hybrid_fuzz_lifecycle",
    "hybrid_fuzz_casper",
    "hybrid_fuzz_external",
    "hybrid_fuzz_corpus",
    "hybrid_kani_bound",
    "hybrid_parallel_stress",
    "hybrid_settlement_matrix",
    "hybrid_slashing_matrix",
    "hybrid_legacy_quarantine",
    "source_anchored_runtime_budget",
    "source_anchored_metering",
    "source_anchored_parallel_eval",
    "source_anchored_casper_replay",
    "source_anchored_settlement",
    "source_anchored_slashing",
    "source_anchored_legacy_quarantine",
    "production_oracle_runtime_budget",
    "production_oracle_metering",
    "production_oracle_parallel_eval",
    "production_oracle_casper_replay",
    "production_oracle_settlement",
    "production_oracle_slashing",
    "production_oracle_legacy_quarantine",
    "source_semantic_runtime_to_replay_trace_commitment",
    "source_semantic_runtime_to_settlement_fuel_isolation",
    "source_semantic_metering_to_parallel_digest_stability",
    "source_semantic_replay_to_slashing_authentication",
    "source_semantic_legacy_to_runtime_quarantine",
    "source_graph_runtime_replay_security",
    "source_graph_replay_cache_security",
    "source_graph_slashing_authorization_security",
    "source_graph_transport_tls_security",
    "source_graph_crypto_key_material_security",
    "source_graph_api_ingress_security",
    "source_graph_dependency_advisory_security",
]

TRACE_DIGEST_IDENTITY_FIELDS = [
    "deploy_id",
    "source_path",
    "redex_id",
    "local_index",
    "billable_kind",
    "primitive_descriptor",
    "weight",
]


def schema_json_default(value):
    try:
        return int(value)
    except Exception:
        return str(value)


def canonical_event(
    kind,
    weight,
    descriptor="source",
    deploy=0,
    path=None,
    redex_id=None,
    local_index=None,
    primitive_descriptor=None,
):
    event_path = [int(v) for v in (path or [])]
    stable_index = event_path[-1] if event_path else 0
    primitive = (
        primitive_descriptor
        if primitive_descriptor is not None
        else (descriptor if str(kind) == "primitive" else "")
    )
    return {
        "kind": str(kind),
        "weight": int(weight),
        "descriptor": str(descriptor),
        "primitive_descriptor": str(primitive),
        "deploy": int(deploy),
        "path": event_path,
        "redex_id": int(redex_id) if redex_id is not None else int(stable_index),
        "local_index": int(local_index) if local_index is not None else int(stable_index),
    }


def canonical_scenario(
    name,
    events=None,
    lifecycle=None,
    deploy_count=1,
    initial_budget=0,
    phlo_limit=0,
    phlo_price=0,
    token_cost=0,
    replay_fields=None,
    replay_mutations=None,
    settlement=None,
    concurrency=None,
    projection=None,
    resource_bounds=None,
    rust_replay=None,
    source_seed=None,
    attack_campaign=None,
    oracle_kind=None,
    production_path=None,
    campaign_steps=None,
    minimized_input_digest=None,
    reproducer_command=None,
    candidate_property=None,
    oracle_strength=None,
    negative_mutations=None,
    rho_source=None,
    production_oracle=None,
    expected_outcome=None,
    differential_axes=None,
    eval_phlo=None,
    expected_error_kind=None,
    eval_result_axes=None,
    rho_source_digest=None,
    state_root_axis=None,
    replay_mode=None,
    term_family=None,
    term_parameters=None,
    metamorphic_relation=None,
    external_service_mode=None,
    adequacy_requirements=None,
    expected_play_replay_relation=None,
    source_corpus_case=None,
    mutation_operator=None,
    differential_oracle=None,
    service_case=None,
    security_axis=None,
    adequacy_budget=None,
    fuzz_target=None,
    fuzz_seed_kind=None,
    kani_harness=None,
    bounded_depth=None,
    mutator_family=None,
    production_replay_target=None,
    promotion_gate=None,
    threat_family="search_governance",
    expected_invariants=None,
    rust_reproducer=None,
    promotion_target="record",
    expected_classification="bisimilar",
    trace_digest_fields=None,
    source_file=None,
    source_line=0,
    source_symbol=None,
    cost_surface=None,
    source_risk=None,
    reachable_from_user_deploy=False,
    source_surface_status=None,
    oracle_surface=None,
    mutation_axis=None,
    expected_disposition=None,
    source_facets=None,
    source_anchor_digest=None,
    cross_surface_role=None,
    semantic_oracle=None,
    security_surface=None,
    external_input_kind=None,
    auth_boundary=None,
    replay_boundary=None,
    slashing_authorization=None,
    secret_material_touched=False,
    source_anchor_status=None,
    dependency_advisory_id=None,
):
    return {
        "name": str(name),
        "threat_family": str(threat_family),
        "events": events or [],
        "lifecycle": lifecycle or [],
        "deploy_count": int(deploy_count),
        "initial_budget": int(initial_budget),
        "phlo_limit": int(phlo_limit),
        "phlo_price": int(phlo_price),
        "token_cost": int(token_cost),
        "replay_fields": replay_fields or {},
        "replay_mutations": replay_mutations or [],
        "settlement": settlement or {},
        "concurrency": concurrency or {},
        "projection": projection or {},
        "resource_bounds": resource_bounds or {},
        "rust_replay": rust_replay or {},
        "source_seed": source_seed or {},
        "attack_campaign": str(attack_campaign or ""),
        "oracle_kind": str(oracle_kind or ""),
        "production_path": str(production_path or ""),
        "campaign_steps": campaign_steps or [],
        "minimized_input_digest": str(minimized_input_digest or ""),
        "reproducer_command": str(reproducer_command or ""),
        "candidate_property": str(candidate_property or ""),
        "oracle_strength": str(oracle_strength or ""),
        "negative_mutations": negative_mutations or [],
        "rho_source": str(rho_source or ""),
        "production_oracle": str(production_oracle or ""),
        "expected_outcome": str(expected_outcome or ""),
        "differential_axes": differential_axes or [],
        "eval_phlo": int(eval_phlo) if eval_phlo is not None else 0,
        "expected_error_kind": str(expected_error_kind or ""),
        "eval_result_axes": eval_result_axes or [],
        "rho_source_digest": str(rho_source_digest or ""),
        "state_root_axis": str(state_root_axis or ""),
        "replay_mode": str(replay_mode or ""),
        "term_family": str(term_family or ""),
        "term_parameters": term_parameters or {},
        "metamorphic_relation": str(metamorphic_relation or ""),
        "external_service_mode": str(external_service_mode or ""),
        "adequacy_requirements": adequacy_requirements or [],
        "expected_play_replay_relation": str(expected_play_replay_relation or ""),
        "source_corpus_case": source_corpus_case or {},
        "mutation_operator": str(mutation_operator or ""),
        "differential_oracle": str(differential_oracle or ""),
        "service_case": str(service_case or ""),
        "security_axis": str(security_axis or ""),
        "adequacy_budget": adequacy_budget or {},
        "fuzz_target": str(fuzz_target or ""),
        "fuzz_seed_kind": str(fuzz_seed_kind or ""),
        "kani_harness": str(kani_harness or ""),
        "bounded_depth": int(bounded_depth) if bounded_depth is not None else 0,
        "mutator_family": str(mutator_family or ""),
        "production_replay_target": str(production_replay_target or ""),
        "promotion_gate": str(promotion_gate or ""),
        "expected_invariants": expected_invariants or [],
        "rust_reproducer": rust_reproducer or {},
        "promotion_target": str(promotion_target),
        "expected_classification": str(expected_classification),
        "trace_digest_fields": trace_digest_fields or (TRACE_DIGEST_IDENTITY_FIELDS if events else []),
        "source_file": str(source_file or ""),
        "source_line": int(source_line) if source_line is not None else 0,
        "source_symbol": str(source_symbol or ""),
        "cost_surface": str(cost_surface or ""),
        "source_risk": str(source_risk or ""),
        "reachable_from_user_deploy": bool(reachable_from_user_deploy),
        "source_surface_status": str(source_surface_status or ""),
        "oracle_surface": str(oracle_surface or ""),
        "mutation_axis": str(mutation_axis or ""),
        "expected_disposition": str(expected_disposition or ""),
        "source_facets": [str(facet) for facet in (source_facets or [])],
        "source_anchor_digest": str(source_anchor_digest or ""),
        "cross_surface_role": str(cross_surface_role or ""),
        "semantic_oracle": str(semantic_oracle or ""),
        "security_surface": str(security_surface or ""),
        "external_input_kind": str(external_input_kind or ""),
        "auth_boundary": str(auth_boundary or ""),
        "replay_boundary": str(replay_boundary or ""),
        "slashing_authorization": slashing_authorization or {},
        "secret_material_touched": bool(secret_material_touched),
        "source_anchor_status": str(source_anchor_status or ""),
        "dependency_advisory_id": str(dependency_advisory_id or ""),
    }


def coverage_features(scenario, classification, witness=None):
    features = Set(["class:{}".format(classification)])
    if scenario.get("events"):
        features = features.union(Set(["events"]))
    for field in scenario.get("trace_digest_fields", []):
        features = features.union(Set(["trace_digest_field:{}".format(field)]))
    if scenario.get("lifecycle"):
        features = features.union(Set(["lifecycle"]))
    if int(scenario.get("deploy_count", 1)) > 1:
        features = features.union(Set(["multi_deploy"]))
    if any(int(event.get("weight", 0)) <= 0 for event in scenario.get("events", [])):
        features = features.union(Set(["invalid_admission"]))
    if any(int(event.get("weight", 0)) > 2**63 - 1 for event in scenario.get("events", [])):
        features = features.union(Set(["oversized_weight"]))
    if scenario.get("replay_fields", {}):
        features = features.union(Set(["replay"]))
    if scenario.get("replay_mutations", []):
        features = features.union(Set(["replay_mutation"]))
    if scenario.get("negative_mutations", []):
        features = features.union(Set(["negative_auth", "negative_mutation"]))
    if scenario.get("settlement", {}):
        features = features.union(Set(["settlement"]))
    if scenario.get("concurrency", {}):
        features = features.union(Set(["concurrency"]))
    if scenario.get("projection", {}):
        features = features.union(Set(["projection"]))
    if scenario.get("resource_bounds", {}):
        features = features.union(Set(["resource_bounds"]))
    if scenario.get("rust_replay", {}):
        features = features.union(Set(["rust_replay"]))
    if scenario.get("source_seed", {}):
        features = features.union(Set(["source_seed"]))
    if scenario.get("attack_campaign"):
        features = features.union(Set(["attack_campaign"]))
    if scenario.get("oracle_kind"):
        features = features.union(Set(["oracle"]))
    if scenario.get("production_path"):
        features = features.union(Set(["production_path"]))
    if scenario.get("campaign_steps", []):
        features = features.union(Set(["campaign_steps"]))
    if scenario.get("minimized_input_digest"):
        features = features.union(Set(["minimized_input"]))
    if scenario.get("reproducer_command"):
        features = features.union(Set(["reproducer_command"]))
    if scenario.get("candidate_property"):
        features = features.union(Set(["candidate_property"]))
        features = features.union(Set(["property:{}".format(scenario.get("candidate_property"))]))
    if scenario.get("oracle_strength"):
        features = features.union(Set(["oracle_strength"]))
        features = features.union(Set(["oracle_strength:{}".format(scenario.get("oracle_strength"))]))
    if scenario.get("rho_source"):
        features = features.union(Set(["rho_source", "production_rholang_eval"]))
    if scenario.get("production_oracle"):
        features = features.union(Set(["production_oracle"]))
        features = features.union(Set(["oracle:{}".format(scenario.get("production_oracle"))]))
    if scenario.get("expected_outcome"):
        features = features.union(Set(["expected_outcome"]))
        features = features.union(Set(["outcome:{}".format(scenario.get("expected_outcome"))]))
    if scenario.get("differential_axes", []):
        features = features.union(Set(["production_differential", "differential_axes"]))
    if int(scenario.get("eval_phlo", 0)) > 0:
        features = features.union(Set(["eval_phlo"]))
    if scenario.get("expected_error_kind"):
        features = features.union(Set(["expected_error_kind"]))
        features = features.union(Set(["error:{}".format(scenario.get("expected_error_kind"))]))
    if scenario.get("eval_result_axes", []):
        features = features.union(Set(["production_eval_result", "eval_result_axes"]))
    if scenario.get("rho_source_digest"):
        features = features.union(Set(["rho_source_digest"]))
    if scenario.get("state_root_axis"):
        features = features.union(Set(["state_root_axis"]))
        features = features.union(Set(["state_root:{}".format(scenario.get("state_root_axis"))]))
    if scenario.get("replay_mode"):
        features = features.union(Set(["replay_mode"]))
        features = features.union(Set(["replay_mode:{}".format(scenario.get("replay_mode"))]))
    if scenario.get("term_family"):
        features = features.union(Set(["term_family"]))
        features = features.union(Set(["term_family:{}".format(scenario.get("term_family"))]))
    if scenario.get("term_parameters", {}):
        features = features.union(Set(["term_parameters"]))
    if scenario.get("metamorphic_relation"):
        features = features.union(Set(["semantic_metamorphic", "metamorphic"]))
        features = features.union(Set(["metamorphic_relation:{}".format(scenario.get("metamorphic_relation"))]))
    if scenario.get("external_service_mode"):
        features = features.union(Set(["mock_external_service", "external_service_replay"]))
        features = features.union(Set(["external_service_mode:{}".format(scenario.get("external_service_mode"))]))
    if scenario.get("adequacy_requirements", []):
        features = features.union(Set(["coverage_adequacy"]))
    if scenario.get("expected_play_replay_relation"):
        features = features.union(Set(["expected_play_replay_relation"]))
        features = features.union(Set(["play_replay_relation:{}".format(scenario.get("expected_play_replay_relation"))]))
    if scenario.get("source_corpus_case", {}):
        features = features.union(Set(["corpus_semantic", "source_corpus_case"]))
    if scenario.get("mutation_operator"):
        features = features.union(Set(["grammar_mutation", "mutation_operator"]))
        features = features.union(Set(["mutation_operator:{}".format(scenario.get("mutation_operator"))]))
    if scenario.get("differential_oracle"):
        features = features.union(Set(["differential_oracle"]))
        features = features.union(Set(["differential_oracle:{}".format(scenario.get("differential_oracle"))]))
    if scenario.get("service_case"):
        features = features.union(Set(["external_service_matrix", "service_case"]))
        features = features.union(Set(["service_case:{}".format(scenario.get("service_case"))]))
    if scenario.get("security_axis"):
        features = features.union(Set(["casper_security_matrix", "security_axis"]))
        features = features.union(Set(["security_axis:{}".format(scenario.get("security_axis"))]))
    if scenario.get("adequacy_budget", {}):
        features = features.union(Set(["adequacy_budget"]))
    if scenario.get("fuzz_target"):
        features = features.union(Set(["fuzz_target"]))
        features = features.union(Set(["fuzz_target:{}".format(scenario.get("fuzz_target"))]))
    if scenario.get("fuzz_seed_kind"):
        features = features.union(Set(["fuzz_seed_kind"]))
        features = features.union(Set(["fuzz_seed_kind:{}".format(scenario.get("fuzz_seed_kind"))]))
    if scenario.get("kani_harness"):
        features = features.union(Set(["kani_harness"]))
        features = features.union(Set(["kani_harness:{}".format(scenario.get("kani_harness"))]))
    if int(scenario.get("bounded_depth", 0)) > 0:
        features = features.union(Set(["bounded_depth"]))
    if scenario.get("mutator_family"):
        features = features.union(Set(["mutator_family"]))
        features = features.union(Set(["mutator_family:{}".format(scenario.get("mutator_family"))]))
    if scenario.get("production_replay_target"):
        features = features.union(Set(["production_replay_target"]))
        features = features.union(Set(["production_replay_target:{}".format(scenario.get("production_replay_target"))]))
    if scenario.get("promotion_gate"):
        features = features.union(Set(["promotion_gate"]))
        features = features.union(Set(["promotion_gate:{}".format(scenario.get("promotion_gate"))]))
    if scenario.get("source_file") or scenario.get("source_symbol") or scenario.get("cost_surface") or scenario.get("source_risk"):
        features = features.union(Set(["source_anchored"]))
    if scenario.get("source_file"):
        features = features.union(Set(["source_file"]))
        features = features.union(Set(["source_file:{}".format(scenario.get("source_file"))]))
    if int(scenario.get("source_line", 0)) > 0:
        features = features.union(Set(["source_line"]))
    if scenario.get("source_symbol"):
        features = features.union(Set(["source_symbol"]))
        features = features.union(Set(["source_symbol:{}".format(scenario.get("source_symbol"))]))
    if scenario.get("cost_surface"):
        features = features.union(Set(["cost_surface"]))
        features = features.union(Set(["cost_surface:{}".format(scenario.get("cost_surface"))]))
    if scenario.get("source_risk"):
        features = features.union(Set(["source_risk"]))
        features = features.union(Set(["source_risk:{}".format(scenario.get("source_risk"))]))
    if scenario.get("reachable_from_user_deploy"):
        features = features.union(Set(["reachable_from_user_deploy"]))
    if scenario.get("source_surface_status"):
        features = features.union(Set(["source_surface_status"]))
        features = features.union(Set(["source_surface_status:{}".format(scenario.get("source_surface_status"))]))
    if scenario.get("oracle_surface"):
        features = features.union(Set(["production_oracle_surface"]))
        features = features.union(Set(["oracle_surface:{}".format(scenario.get("oracle_surface"))]))
    if scenario.get("mutation_axis"):
        features = features.union(Set(["mutation_axis"]))
        features = features.union(Set(["mutation_axis:{}".format(scenario.get("mutation_axis"))]))
    if scenario.get("expected_disposition"):
        features = features.union(Set(["expected_disposition"]))
        features = features.union(Set(["disposition:{}".format(scenario.get("expected_disposition"))]))
    if scenario.get("semantic_oracle"):
        features = features.union(Set(["source_semantic_oracle"]))
        features = features.union(Set(["semantic_oracle:{}".format(scenario.get("semantic_oracle"))]))
    if scenario.get("source_facets", []):
        features = features.union(Set(["source_facet"]))
        for facet in scenario.get("source_facets", []):
            features = features.union(Set(["source_facet:{}".format(facet)]))
    if scenario.get("source_anchor_digest"):
        features = features.union(Set(["source_anchor_digest"]))
    if scenario.get("cross_surface_role"):
        features = features.union(Set(["cross_surface_role"]))
        features = features.union(Set(["cross_surface_role:{}".format(scenario.get("cross_surface_role"))]))
    if scenario.get("security_surface"):
        features = features.union(Set(["source_graph_security", "security_surface"]))
        features = features.union(Set(["security_surface:{}".format(scenario.get("security_surface"))]))
    if scenario.get("external_input_kind"):
        features = features.union(Set(["external_input_kind"]))
        features = features.union(Set(["external_input_kind:{}".format(scenario.get("external_input_kind"))]))
    if scenario.get("auth_boundary"):
        features = features.union(Set(["auth_boundary"]))
        features = features.union(Set(["auth_boundary:{}".format(scenario.get("auth_boundary"))]))
    if scenario.get("replay_boundary"):
        features = features.union(Set(["replay_boundary"]))
        features = features.union(Set(["replay_boundary:{}".format(scenario.get("replay_boundary"))]))
    if scenario.get("slashing_authorization", {}):
        features = features.union(Set(["slashing_authorization"]))
        auth = scenario.get("slashing_authorization", {})
        if auth.get("evidence_epoch") == auth.get("current_epoch"):
            features = features.union(Set(["current_evidence_epoch"]))
        if auth.get("target_activation_epoch") == auth.get("current_epoch"):
            features = features.union(Set(["current_target_activation_epoch"]))
        if int(auth.get("parent_pre_state_bond", 0)) > 0:
            features = features.union(Set(["parent_pre_state_bond"]))
        if int(auth.get("ambient_bond", 0)) > 0:
            features = features.union(Set(["ambient_bond"]))
        if int(auth.get("execution_bond", 0)) == 0:
            features = features.union(Set(["zero_execution_bond"]))
    if scenario.get("secret_material_touched"):
        features = features.union(Set(["secret_material_touched"]))
    if scenario.get("source_anchor_status"):
        features = features.union(Set(["source_anchor_status"]))
        features = features.union(Set(["source_anchor_status:{}".format(scenario.get("source_anchor_status"))]))
    if scenario.get("dependency_advisory_id"):
        features = features.union(Set(["dependency_advisory"]))
        features = features.union(Set(["dependency_advisory_id:{}".format(scenario.get("dependency_advisory_id"))]))
    threat_family = scenario.get("threat_family")
    if threat_family:
        features = features.union(Set(["family:{}".format(threat_family)]))
    promotion_target = scenario.get("promotion_target")
    if promotion_target:
        features = features.union(Set(["target:{}".format(promotion_target)]))
    if witness is not None:
        text = str(witness)
        for token in [
            "trace_cap",
            "refund",
            "overflow",
            "missing_digest",
            "digest",
            "event_count",
            "signature",
            "activation",
            "multi_deploy",
            "oop",
            "slot",
            "unmetered",
            "slashing",
            "external",
            "producer",
            "routing",
            "finalization",
            "cache",
            "descriptor",
            "source_path",
            "lifecycle",
            "precharge",
            "admission",
            "rollback",
            "replay_mutation",
            "authority",
            "metamorphic",
            "permutation",
            "corpus",
            "cross_product",
            "source_seed",
            "differential",
            "exploit",
            "campaign",
            "signed_payload",
            "replay_cache",
            "tamper",
            "multi_axis",
            "stateful",
            "production_path",
            "oracle",
            "source_corpus",
            "exploit_cross_product",
            "reset",
            "clear_diagnostic",
            "block_hash",
            "resource_campaign",
            "adversarial_budget",
            "adversarial_replay",
            "adversarial_settlement",
            "adversarial_slashing",
            "adversarial_lifecycle",
            "refund_as_fuel",
            "trace_collision",
            "stale_evidence",
            "finalize_before_join",
            "property_invariant",
            "negative_auth",
            "negative_mutation",
            "source_shape",
            "cross_deploy",
            "scheduler",
            "scheduler_interleaving",
            "cache_resource",
            "candidate_property",
            "oracle_strength",
            "monotonicity",
            "digest_sensitivity",
            "reset_isolation",
            "deploy_separation",
            "bounded_resource",
            "production_differential",
            "production_oracle",
            "production_rholang_eval",
            "rho_source",
            "expected_outcome",
            "differential_axes",
            "replay_downgrade",
            "settlement_isolation",
            "scheduler_finalization",
            "casper_boundary",
            "production_eval_replay",
            "production_source_corpus",
            "production_auth_composition",
            "production_state_root",
            "production_external_boundary",
            "production_error_boundary",
            "eval_phlo",
            "expected_error_kind",
            "eval_result_axes",
            "rho_source_digest",
            "state_root_axis",
            "replay_mode",
            "user_abort",
            "parser_error",
            "state_root_replay",
            "auth_composition",
            "generative_semantic",
            "semantic_metamorphic",
            "external_service_replay",
            "coverage_adequacy",
            "semantic_cross_product",
            "production_mutation",
            "term_family",
            "term_parameters",
            "metamorphic_relation",
            "mock_external_service",
            "external_service_mode",
            "expected_play_replay_relation",
            "corpus_semantic",
            "source_corpus_case",
            "grammar_mutation",
            "mutation_operator",
            "differential_oracle",
            "external_service_matrix",
            "service_case",
            "casper_security_matrix",
            "security_axis",
            "runtime_trace_interleaving",
            "adequacy_budget",
            "hybrid_fuzz_security",
            "hybrid_fuzz_runtime",
            "hybrid_fuzz_replay",
            "hybrid_fuzz_lifecycle",
            "hybrid_fuzz_casper",
            "hybrid_fuzz_external",
            "hybrid_fuzz_corpus",
            "hybrid_kani_bound",
            "hybrid_parallel_stress",
            "hybrid_settlement_matrix",
            "hybrid_slashing_matrix",
            "hybrid_legacy_quarantine",
            "fuzz_target",
            "fuzz_seed_kind",
            "kani_harness",
            "bounded_depth",
            "mutator_family",
            "production_replay_target",
            "promotion_gate",
            "source_anchored",
            "source_file",
            "source_symbol",
            "source_line",
            "cost_surface",
            "source_risk",
            "source_surface_status",
            "reachable_from_user_deploy",
            "runtime_budget",
            "metering",
            "parallel_eval",
            "casper_replay",
            "settlement",
            "legacy_quarantine",
            "production_oracle_surface",
            "oracle_surface",
            "mutation_axis",
            "expected_disposition",
            "source_semantic_oracle",
            "semantic_oracle",
            "source_facet",
            "source_anchor_digest",
            "cross_surface_role",
            "runtime_to_replay",
            "runtime_to_settlement",
            "metering_to_parallel",
            "replay_to_slashing",
            "legacy_to_runtime",
            "accepted",
            "rejected_before_mutation",
            "oop_boundary",
            "replay_invalid",
            "settlement_bounded",
            "source_absent",
            "source_graph_security",
            "security_surface",
            "external_input_kind",
            "auth_boundary",
            "replay_boundary",
            "secret_material_touched",
            "source_anchor_status",
            "dependency_advisory",
            "dependency_advisory_id",
            "transport_tls",
            "crypto_key_material",
            "api_ingress",
            "replay_cache",
            "peer_certificate",
            "private_key",
            "rustsec",
        ]:
            if token in text:
                features = features.union(Set([token]))
    return sorted([str(feature) for feature in features])


def threat_score(classification, features, witness=None):
    base = {
        "confirmed_current_bug": 100,
        "needs_source_audit": 85,
        "projection_risk": 70,
        "assumption_counterexample": 55,
        "proof_or_model_strengthening": 45,
        "bisimilar": 0,
        "confirmed_safe": 0,
    }.get(classification, 10)
    bonus = Integer(0)
    for feature, value in [
        ("concurrency", 10),
        ("settlement", 10),
        ("replay", 10),
        ("replay_mutation", 10),
        ("invalid_admission", 8),
        ("oversized_weight", 8),
        ("resource_bounds", 8),
        ("overflow", 8),
        ("trace_cap", 7),
        ("multi_deploy", 7),
        ("oop", 6),
        ("slot", 6),
        ("activation", 6),
        ("signature", 6),
        ("slashing", 6),
        ("external", 5),
        ("rust_replay", 4),
        ("producer", 5),
        ("routing", 5),
        ("finalization", 7),
        ("cache", 6),
        ("descriptor", 5),
        ("source_path", 5),
        ("lifecycle", 7),
        ("precharge", 5),
        ("admission", 5),
        ("rollback", 5),
        ("authority", 7),
        ("metamorphic", 6),
        ("permutation", 5),
        ("corpus", 4),
        ("cross_product", 8),
        ("source_seed", 6),
        ("differential", 7),
        ("exploit", 10),
        ("campaign", 8),
        ("signed_payload", 8),
        ("replay_cache", 8),
        ("tamper", 8),
        ("multi_axis", 8),
        ("stateful", 9),
        ("production_path", 9),
        ("oracle", 7),
        ("source_corpus", 7),
        ("exploit_cross_product", 10),
        ("reset", 5),
        ("clear_diagnostic", 5),
        ("block_hash", 8),
        ("resource_campaign", 8),
        ("adversarial_budget", 10),
        ("adversarial_replay", 10),
        ("adversarial_settlement", 10),
        ("adversarial_slashing", 10),
        ("adversarial_lifecycle", 10),
        ("refund_as_fuel", 10),
        ("trace_collision", 10),
        ("stale_evidence", 9),
        ("finalize_before_join", 9),
        ("candidate_property", 5),
        ("negative_auth", 10),
        ("negative_mutation", 10),
        ("source_shape", 8),
        ("cross_deploy", 8),
        ("scheduler_interleaving", 8),
        ("cache_resource", 8),
        ("deploy_separation", 8),
        ("digest_sensitivity", 8),
        ("production_differential", 10),
        ("production_oracle", 9),
        ("production_rholang_eval", 9),
        ("replay_downgrade", 10),
        ("settlement_isolation", 9),
        ("scheduler_finalization", 9),
        ("casper_boundary", 10),
        ("production_eval_replay", 10),
        ("production_source_corpus", 9),
        ("production_auth_composition", 10),
        ("production_state_root", 9),
        ("production_external_boundary", 9),
        ("production_error_boundary", 9),
        ("production_eval_result", 9),
        ("state_root_axis", 8),
        ("replay_mode", 8),
        ("expected_error_kind", 7),
        ("generative_semantic", 10),
        ("semantic_metamorphic", 10),
        ("external_service_replay", 10),
        ("coverage_adequacy", 10),
        ("semantic_cross_product", 9),
        ("production_mutation", 9),
        ("mock_external_service", 9),
        ("term_family", 7),
        ("metamorphic_relation", 8),
        ("expected_play_replay_relation", 8),
        ("corpus_semantic", 10),
        ("source_corpus_case", 8),
        ("grammar_mutation", 10),
        ("mutation_operator", 8),
        ("differential_oracle", 10),
        ("external_service_matrix", 10),
        ("service_case", 8),
        ("casper_security_matrix", 10),
        ("security_axis", 8),
        ("runtime_trace_interleaving", 10),
        ("adequacy_budget", 8),
        ("hybrid_fuzz_security", 12),
        ("hybrid_fuzz_runtime", 10),
        ("hybrid_fuzz_replay", 10),
        ("hybrid_fuzz_lifecycle", 10),
        ("hybrid_fuzz_casper", 12),
        ("hybrid_fuzz_external", 9),
        ("hybrid_fuzz_corpus", 9),
        ("hybrid_kani_bound", 10),
        ("hybrid_parallel_stress", 10),
        ("hybrid_settlement_matrix", 10),
        ("hybrid_slashing_matrix", 12),
        ("hybrid_legacy_quarantine", 10),
        ("fuzz_target", 8),
        ("fuzz_seed_kind", 6),
        ("kani_harness", 8),
        ("bounded_depth", 5),
        ("mutator_family", 7),
        ("production_replay_target", 8),
        ("promotion_gate", 8),
        ("source_anchored", 12),
        ("source_risk", 9),
        ("source_file", 5),
        ("source_symbol", 5),
        ("reachable_from_user_deploy", 8),
        ("production_oracle_surface", 12),
        ("mutation_axis", 8),
        ("expected_disposition", 8),
        ("runtime_budget", 10),
        ("metering", 10),
        ("parallel_eval", 10),
        ("casper_replay", 12),
        ("legacy_quarantine", 10),
        ("source_semantic_oracle", 12),
        ("semantic_oracle", 8),
        ("source_facet", 9),
        ("source_anchor_digest", 8),
        ("cross_surface_role", 8),
        ("source_graph_security", 14),
        ("security_surface", 10),
        ("external_input_kind", 8),
        ("auth_boundary", 12),
        ("replay_boundary", 12),
        ("secret_material_touched", 14),
        ("source_anchor_status", 8),
        ("dependency_advisory", 10),
        ("transport_tls", 10),
        ("crypto_key_material", 12),
        ("api_ingress", 9),
        ("replay_cache", 10),
    ]:
        if feature in features:
            bonus += Integer(value)
    return int(base + bonus)


def record(axis, classification, name, statement, scenario, witness, followups):
    features = coverage_features(scenario, classification, witness)
    return {
        "axis": str(axis),
        "classification": str(classification),
        "name": str(name),
        "statement": str(statement),
        "scenario": scenario,
        "deterministic_witness": witness,
        "coverage_features": features,
        "threat_score": threat_score(classification, features, witness),
        "production_disposition": production_disposition(classification),
        "production_guards": production_guards(name, classification, scenario),
        "formal_followups": followups,
    }


def production_disposition(classification):
    if str(classification) == "projection_risk":
        return "guarded_safe"
    return ""


def production_guards(identifier, classification, scenario):
    # These names are model-level guard labels emitted into findings and docs.
    # They are intentionally not Rust test names unless a scenario separately
    # names an executable rust_reproducer.
    if str(classification) != "projection_risk":
        return []

    name = str(identifier)
    family = str(scenario.get("threat_family", ""))
    guards = []

    if family == "producer_routing" or "producer" in name:
        guards.extend([
            "projection_risk_zero_weight_strict_route_rejects_before_trace_mutation",
            "rb_zero_weight_admission_rejection_preserves_trace",
        ])
    if family in ["concurrency_schedule", "adversarial_lifecycle"] or "finalization" in name or "worker_join" in name:
        guards.extend([
            "projection_risk_parallel_evaluation_result_waits_for_complete_cost_trace",
            "RuntimeBudgetReplay.tla",
        ])
    if family == "exploit_campaign" or "lifecycle_attack" in name:
        guards.extend([
            "projection_risk_lifecycle_campaign_does_not_leak_budget_or_trace",
            "RuntimeBudgetReplay.tla",
        ])
    if family in ["adversarial_slashing", "slashing_composition"] or "stale" in name or "slashing" in name:
        guards.extend([
            "stale_slashing_evidence_fields_are_replay_and_block_hash_authenticated",
            "stale_cost_invalid_evidence_cannot_reslash_or_mutate_user_cost_trace",
            "stale_cost_evidence_sound",
        ])

    if not guards:
        guards.append("cost_accounting_frontier_generated_fixtures_are_classified")

    unique = []
    for guard in guards:
        if guard not in unique:
            unique.append(guard)
    return unique


def scenario_fixture(identifier, classification, scenario, oracle, harness, projection=None, assertions=None):
    features = coverage_features(scenario, classification, oracle)
    return {
        "id": str(identifier),
        "classification": str(classification),
        "scenario": scenario,
        "expected_oracle": oracle,
        "expected_harness": harness,
        "expected_projection": projection if projection is not None else harness,
        "coverage_features": features,
        "threat_score": threat_score(classification, features, oracle),
        "production_disposition": production_disposition(classification),
        "production_guards": production_guards(identifier, classification, scenario),
        "assertions": assertions or ["classification != unexpected"],
    }


def objective_vector(item):
    features = item.get("coverage_features", [])
    witness_text = str(item.get("deterministic_witness", {}))
    return vector(
        ZZ,
        [
            Integer(item.get("threat_score", 0)),
            Integer(len(features)),
            Integer(1 if "concurrency" in features else 0),
            Integer(1 if "settlement" in features else 0),
            Integer(1 if "replay" in features else 0),
            Integer(len(witness_text)),
        ],
    )


def dominates(left, right):
    left_v = objective_vector(left)
    right_v = objective_vector(right)
    return all(left_v[i] >= right_v[i] for i in range(len(left_v))) and any(
        left_v[i] > right_v[i] for i in range(len(left_v))
    )


def pareto_frontier(records):
    frontier = []
    for candidate in records:
        if not any(
            dominates(other, candidate)
            for other in records
            if other["name"] != candidate["name"]
        ):
            candidate = dict(candidate)
            candidate["objective_vector"] = [
                int(value) for value in objective_vector(candidate)
            ]
            frontier.append(candidate)
    return sorted(frontier, key=lambda item: (-item["threat_score"], item["name"]))


def coverage_summary(records):
    class_counts = {}
    features = Set([])
    for item in records:
        class_counts[item["classification"]] = class_counts.get(item["classification"], 0) + 1
        features = features.union(Set(item.get("coverage_features", [])))
    return {
        "record_count": len(records),
        "class_counts": class_counts,
        "features": sorted([str(feature) for feature in features]),
    }
