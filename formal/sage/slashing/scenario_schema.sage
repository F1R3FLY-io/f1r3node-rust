from sage.all import Integer, Set, vector, ZZ


CLASSIFICATIONS = [
    "confirmed_safe",
    "bisimilar",
    "permitted_bug_fix",
    "candidate_boundary",
    "projection_risk",
    "assumption_counterexample",
    "unexpected",
]


def schema_json_default(value):
    try:
        return int(value)
    except Exception:
        return str(value)


def validator_label(index):
    return "v{}".format(int(index))


def canonical_pair_list(items):
    pairs = []
    for item in items or []:
        if isinstance(item, (list, tuple)) and len(item) == 2:
            pairs.append([int(item[0]), int(item[1])])
        else:
            pairs.append([int(item), int(item)])
    return pairs


def canonical_scenario(
    validators,
    stakes=None,
    epochs=None,
    blocks=None,
    justifications=None,
    direct_equivocators=None,
    neglect_edges=None,
    reports=None,
    slash_targets=None,
    events=None,
    views=None,
    retention_policy=None,
    projection=None,
    rust_replay=None,
    expected_classification="bisimilar",
):
    validators = [int(v) for v in validators]
    stakes = [int(v) for v in (stakes if stakes is not None else [1 for _ in validators])]
    epochs = [int(v) for v in (epochs if epochs is not None else [0 for _ in validators])]
    return {
        "validators": validators,
        "stakes": stakes,
        "epochs": epochs,
        "blocks": blocks or [],
        "justifications": justifications or [],
        "direct_equivocators": [int(v) for v in (direct_equivocators or [])],
        "neglect_edges": [[int(a), int(b)] for a, b in (neglect_edges or [])],
        "reports": canonical_pair_list(reports),
        "slash_targets": canonical_pair_list(slash_targets),
        "events": events or [],
        "views": views or [],
        "retention_policy": retention_policy or {},
        "projection": projection or {},
        "rust_replay": rust_replay or {},
        "expected_classification": expected_classification,
    }


def schema_example():
    return canonical_scenario(
        [0, 1, 2],
        stakes=[1, 1, 1],
        blocks=[
            {"hash": 1, "sender": 0, "seq": 1, "justifications": [], "slash_targets": []},
            {"hash": 2, "sender": 0, "seq": 1, "justifications": [], "slash_targets": []},
            {"hash": 3, "sender": 1, "seq": 2, "justifications": [{"validator": 0, "hash": 2}], "slash_targets": []},
        ],
        direct_equivocators=[0],
        neglect_edges=[(1, 0)],
        events=[{"kind": "observe", "slot": 0, "validator": 1}],
        views=[{"node": "local", "active_edges": [[1, 0]]}],
        retention_policy={"window": 2},
        expected_classification="candidate_boundary",
    )


def coverage_features(scenario, classification, witness=None):
    features = Set(["class:{}".format(classification)])
    if len(scenario.get("blocks", [])) > 0:
        features = features.union(Set(["dag"]))
    if len(scenario.get("direct_equivocators", [])) > 0:
        features = features.union(Set(["direct_equivocation"]))
    if len(scenario.get("neglect_edges", [])) > 0:
        features = features.union(Set(["neglect_edges"]))
    if len(scenario.get("reports", [])) > 0:
        features = features.union(Set(["reports"]))
    if len(scenario.get("slash_targets", [])) > 0:
        features = features.union(Set(["slash_targets"]))
    if len(scenario.get("events", [])) > 0:
        features = features.union(Set(["events"]))
    if len(scenario.get("views", [])) > 0:
        features = features.union(Set(["views"]))
    if scenario.get("retention_policy", {}):
        features = features.union(Set(["retention_policy"]))
    if scenario.get("projection", {}):
        features = features.union(Set(["projection"]))
    if scenario.get("rust_replay", {}):
        features = features.union(Set(["rust_replay"]))
    if len(Set(scenario.get("epochs", []))) > 1:
        features = features.union(Set(["epoch_churn"]))
    stakes = vector(ZZ, [Integer(s) for s in scenario.get("stakes", [])])
    if len(stakes) > 0 and len(Set([int(s) for s in stakes])) > 1:
        features = features.union(Set(["weighted"]))
    if witness is not None:
        text = str(witness)
        for token in [
            "closure",
            "overflow",
            "retention",
            "pruning",
            "withholding",
            "projection",
            "view_gap",
            "partition",
            "delay",
            "cache",
            "replay",
            "finality",
            "schedule",
            "detector",
            "lifecycle",
            "availability",
            "objective",
            "canonical",
            "coverage_gap",
            "uncovered_rust",
            "validator_churn_depth",
            "withholding_duration",
            "detector_traversal_depth",
            "retention_window_boundary",
            "stake_damage_pareto",
            "replay_divergence",
        ]:
            if token in text:
                features = features.union(Set([token]))
    return sorted([str(feature) for feature in features])


def threat_score(classification, features, witness=None):
    base = {
        "unexpected": 100,
        "projection_risk": 70,
        "assumption_counterexample": 55,
        "candidate_boundary": 35,
        "permitted_bug_fix": 20,
        "bisimilar": 0,
        "confirmed_safe": 0,
    }.get(classification, 10)
    bonus = Integer(0)
    for feature, value in [
        ("weighted", 10),
        ("overflow", 10),
        ("retention", 8),
        ("pruning", 8),
        ("epoch_churn", 6),
        ("withholding", 6),
        ("view_gap", 6),
        ("neglect_edges", 5),
        ("projection", 5),
        ("events", 4),
        ("views", 4),
        ("rust_replay", 4),
        ("partition", 4),
        ("delay", 4),
        ("detector", 4),
        ("availability", 4),
        ("lifecycle", 4),
        ("objective", 4),
        ("coverage_gap", 6),
        ("uncovered_rust", 6),
        ("detector_traversal_depth", 6),
        ("retention_window_boundary", 6),
        ("stake_damage_pareto", 6),
        ("replay_divergence", 6),
        ("withholding_duration", 5),
        ("validator_churn_depth", 5),
    ]:
        if feature in features:
            bonus += Integer(value)
    if witness is not None and "extra_stake" in str(witness):
        bonus += Integer(10)
    return int(base + bonus)


def scenario_fixture(identifier, classification, scenario, oracle, harness, projection=None, assertions=None):
    features = coverage_features(scenario, classification, oracle)
    return {
        "id": identifier,
        "classification": classification,
        "scenario": scenario,
        "expected_oracle": oracle,
        "expected_harness": harness,
        "expected_projection": projection if projection is not None else harness,
        "coverage_features": features,
        "threat_score": threat_score(classification, features, oracle),
        "assertions": assertions or ["classification != unexpected"],
    }


def coverage_summary(records):
    features = Set([])
    class_counts = {}
    for item in records:
        class_counts[item["classification"]] = class_counts.get(item["classification"], 0) + 1
        for feature in item.get("coverage_features", []):
            features = features.union(Set([feature]))
    return {
        "record_count": len(records),
        "class_counts": class_counts,
        "feature_count": len(features),
        "features": sorted([str(feature) for feature in features]),
    }
