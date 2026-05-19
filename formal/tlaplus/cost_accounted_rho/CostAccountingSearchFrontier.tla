--------------------- MODULE CostAccountingSearchFrontier ---------------------
(****************************************************************************)
(* Bounded witness-classification model for the cost-accounting search        *)
(* frontier. It checks that generated witnesses cannot silently motivate      *)
(* implementation changes before production traceability classifies them.             *)
(****************************************************************************)

EXTENDS Naturals, TLC

VARIABLES
    witness,
    threatFamily,
    reproduced,
    productionInvariantViolated,
    guardedByProduction,
    theoremGap,
    hasRustReproducer,
    hasExpectedInvariant,
    hasCampaignSteps,
    hasProductionPath,
    hasOracle,
    hasSourceFacet,
    hasSourceAnchorDigest,
    hasCrossSurfaceRole,
    classified,
    action,
    promotionTarget

vars ==
    <<witness, threatFamily, reproduced, productionInvariantViolated,
      guardedByProduction, theoremGap, hasRustReproducer,
      hasExpectedInvariant, hasCampaignSteps, hasProductionPath, hasOracle,
      hasSourceFacet, hasSourceAnchorDigest, hasCrossSurfaceRole,
      classified, action, promotionTarget>>

WitnessSet ==
    {"budget", "replay", "settlement", "concurrency", "slashing",
     "stateful_campaign", "source_corpus", "production_path_diff",
     "exploit_cross_product", "source_semantic_oracle"}
ThreatFamilySet ==
    {"producer_routing", "concurrency_schedule", "replay_authentication",
     "settlement", "slashing_composition", "resource_exhaustion",
     "search_governance", "stateful_campaign", "production_path_diff",
     "source_corpus", "exploit_cross_product", "exploit_campaign",
     "differential_replay", "source_semantic_runtime_replay",
     "source_semantic_runtime_settlement", "source_semantic_metering_parallel",
     "source_semantic_replay_slashing", "source_semantic_legacy_runtime",
     "source_semantic_runtime_to_replay_trace_commitment",
     "source_semantic_runtime_to_settlement_fuel_isolation",
     "source_semantic_metering_to_parallel_digest_stability",
     "source_semantic_replay_to_slashing_authentication",
     "source_semantic_legacy_to_runtime_quarantine",
     "source_semantic_coverage"}
ClassSet ==
    {"unclassified", "confirmed_safe", "bisimilar", "projection_risk",
     "assumption_counterexample", "proof_or_model_strengthening",
     "needs_source_audit", "confirmed_current_bug"}
ActionSet ==
    {"none", "record", "guard", "strengthen_formal", "audit", "fix_source"}
PromotionTargetSet ==
    {"none", "record", "rust_guard", "rust_regression", "rocq", "tla",
     "sage", "audit", "source_fix"}

Init ==
    /\ witness \in WitnessSet
    /\ threatFamily \in ThreatFamilySet
    /\ reproduced = FALSE
    /\ productionInvariantViolated = FALSE
    /\ guardedByProduction = FALSE
    /\ theoremGap = FALSE
    /\ hasRustReproducer = FALSE
    /\ hasExpectedInvariant = FALSE
    /\ hasCampaignSteps = FALSE
    /\ hasProductionPath = FALSE
    /\ hasOracle = FALSE
    /\ hasSourceFacet = FALSE
    /\ hasSourceAnchorDigest = FALSE
    /\ hasCrossSurfaceRole = FALSE
    /\ classified = "unclassified"
    /\ action = "none"
    /\ promotionTarget = "none"

TypeOK ==
    /\ witness \in WitnessSet
    /\ threatFamily \in ThreatFamilySet
    /\ reproduced \in BOOLEAN
    /\ productionInvariantViolated \in BOOLEAN
    /\ guardedByProduction \in BOOLEAN
    /\ theoremGap \in BOOLEAN
    /\ hasRustReproducer \in BOOLEAN
    /\ hasExpectedInvariant \in BOOLEAN
    /\ hasCampaignSteps \in BOOLEAN
    /\ hasProductionPath \in BOOLEAN
    /\ hasOracle \in BOOLEAN
    /\ hasSourceFacet \in BOOLEAN
    /\ hasSourceAnchorDigest \in BOOLEAN
    /\ hasCrossSurfaceRole \in BOOLEAN
    /\ classified \in ClassSet
    /\ action \in ActionSet
    /\ promotionTarget \in PromotionTargetSet

MetadataReady ==
    /\ witness # "stateful_campaign" \/ hasCampaignSteps
    /\ witness # "source_corpus" \/ (hasProductionPath /\ hasOracle)
    /\ witness # "production_path_diff" \/
        (hasProductionPath /\ hasOracle /\ hasRustReproducer)
    /\ witness # "exploit_cross_product" \/
        (hasCampaignSteps /\ hasExpectedInvariant /\
         threatFamily # "search_governance")
    /\ witness # "source_semantic_oracle" \/
        (hasProductionPath /\ hasOracle /\ hasRustReproducer /\
         hasSourceFacet /\ hasSourceAnchorDigest /\ hasCrossSurfaceRole)

DiscoverCampaignSteps ==
    /\ classified = "unclassified"
    /\ witness \in {"stateful_campaign", "exploit_cross_product"}
    /\ hasCampaignSteps' = TRUE
    /\ UNCHANGED <<witness, threatFamily, reproduced,
        productionInvariantViolated, guardedByProduction, theoremGap,
        hasRustReproducer, hasExpectedInvariant, hasProductionPath, hasOracle,
        hasSourceFacet, hasSourceAnchorDigest, hasCrossSurfaceRole,
        classified, action, promotionTarget>>

DiscoverProductionPath ==
    /\ classified = "unclassified"
    /\ witness \in {"source_corpus", "production_path_diff", "exploit_cross_product"}
    /\ hasProductionPath' = TRUE
    /\ hasOracle' = TRUE
    /\ hasRustReproducer' = TRUE
    /\ UNCHANGED <<witness, threatFamily, reproduced,
        productionInvariantViolated, guardedByProduction, theoremGap,
        hasExpectedInvariant, hasCampaignSteps, classified, action,
        promotionTarget, hasSourceFacet, hasSourceAnchorDigest,
        hasCrossSurfaceRole>>

DiscoverSourceSemanticMetadata ==
    /\ classified = "unclassified"
    /\ witness = "source_semantic_oracle"
    /\ hasProductionPath' = TRUE
    /\ hasOracle' = TRUE
    /\ hasRustReproducer' = TRUE
    /\ hasSourceFacet' = TRUE
    /\ hasSourceAnchorDigest' = TRUE
    /\ hasCrossSurfaceRole' = TRUE
    /\ UNCHANGED <<witness, threatFamily, reproduced,
        productionInvariantViolated, guardedByProduction, theoremGap,
        hasExpectedInvariant, hasCampaignSteps, classified, action,
        promotionTarget>>

DiscoverRustReproduction ==
    /\ classified = "unclassified"
    /\ reproduced' = TRUE
    /\ hasRustReproducer' = TRUE
    /\ promotionTarget' = "rust_regression"
    /\ UNCHANGED <<witness, threatFamily, productionInvariantViolated,
        guardedByProduction, theoremGap, hasExpectedInvariant,
        hasCampaignSteps, hasProductionPath, hasOracle, hasSourceFacet,
        hasSourceAnchorDigest, hasCrossSurfaceRole, classified, action>>

DiscoverInvariantViolation ==
    /\ classified = "unclassified"
    /\ productionInvariantViolated' = TRUE
    /\ hasExpectedInvariant' = TRUE
    /\ promotionTarget' = "source_fix"
    /\ UNCHANGED <<witness, threatFamily, reproduced, guardedByProduction,
        theoremGap, hasRustReproducer, hasCampaignSteps, hasProductionPath,
        hasOracle, hasSourceFacet, hasSourceAnchorDigest, hasCrossSurfaceRole,
        classified, action>>

DiscoverProductionGuard ==
    /\ classified = "unclassified"
    /\ guardedByProduction' = TRUE
    /\ hasRustReproducer' = TRUE
    /\ promotionTarget' = "rust_guard"
    /\ UNCHANGED <<witness, threatFamily, reproduced,
        productionInvariantViolated, theoremGap, hasExpectedInvariant,
        hasCampaignSteps, hasProductionPath, hasOracle, hasSourceFacet,
        hasSourceAnchorDigest, hasCrossSurfaceRole, classified, action>>

DiscoverTheoremGap ==
    /\ classified = "unclassified"
    /\ theoremGap' = TRUE
    /\ hasExpectedInvariant' = TRUE
    /\ promotionTarget' \in {"rocq", "tla", "sage"}
    /\ UNCHANGED <<witness, threatFamily, reproduced,
        productionInvariantViolated, guardedByProduction, hasRustReproducer,
        hasCampaignSteps, hasProductionPath, hasOracle, hasSourceFacet,
        hasSourceAnchorDigest, hasCrossSurfaceRole, classified, action>>

ClassifyBug ==
    /\ classified = "unclassified"
    /\ MetadataReady
    /\ reproduced \/ productionInvariantViolated
    /\ classified' = "confirmed_current_bug"
    /\ action' = "fix_source"
    /\ promotionTarget' = "source_fix"
    /\ UNCHANGED <<witness, threatFamily, reproduced,
        productionInvariantViolated, guardedByProduction, theoremGap,
        hasRustReproducer, hasExpectedInvariant, hasCampaignSteps,
        hasProductionPath, hasOracle, hasSourceFacet, hasSourceAnchorDigest,
        hasCrossSurfaceRole>>

ClassifyGuardedProjection ==
    /\ classified = "unclassified"
    /\ MetadataReady
    /\ ~reproduced
    /\ ~productionInvariantViolated
    /\ guardedByProduction
    /\ classified' = "projection_risk"
    /\ action' = "guard"
    /\ promotionTarget' = "rust_guard"
    /\ UNCHANGED <<witness, threatFamily, reproduced,
        productionInvariantViolated, guardedByProduction, theoremGap,
        hasRustReproducer, hasExpectedInvariant, hasCampaignSteps,
        hasProductionPath, hasOracle, hasSourceFacet, hasSourceAnchorDigest,
        hasCrossSurfaceRole>>

ClassifyFormalStrengthening ==
    /\ classified = "unclassified"
    /\ MetadataReady
    /\ ~reproduced
    /\ ~productionInvariantViolated
    /\ theoremGap
    /\ hasExpectedInvariant
    /\ promotionTarget \in {"rocq", "tla", "sage"}
    /\ classified' = "proof_or_model_strengthening"
    /\ action' = "strengthen_formal"
    /\ UNCHANGED <<witness, threatFamily, reproduced,
        productionInvariantViolated, guardedByProduction, theoremGap,
        hasRustReproducer, hasExpectedInvariant, hasCampaignSteps,
        hasProductionPath, hasOracle, hasSourceFacet, hasSourceAnchorDigest,
        hasCrossSurfaceRole, promotionTarget>>

ClassifySafe ==
    /\ classified = "unclassified"
    /\ MetadataReady
    /\ ~reproduced
    /\ ~productionInvariantViolated
    /\ ~guardedByProduction
    /\ ~theoremGap
    /\ classified' \in {"confirmed_safe", "bisimilar"}
    /\ action' = "record"
    /\ promotionTarget' = "record"
    /\ UNCHANGED <<witness, threatFamily, reproduced,
        productionInvariantViolated, guardedByProduction, theoremGap,
        hasRustReproducer, hasExpectedInvariant, hasCampaignSteps,
        hasProductionPath, hasOracle, hasSourceFacet, hasSourceAnchorDigest,
        hasCrossSurfaceRole>>

EscalateAudit ==
    /\ classified = "unclassified"
    /\ MetadataReady
    /\ action' = "audit"
    /\ classified' = "needs_source_audit"
    /\ promotionTarget' = "audit"
    /\ UNCHANGED <<witness, threatFamily, reproduced,
        productionInvariantViolated, guardedByProduction, theoremGap,
        hasRustReproducer, hasExpectedInvariant, hasCampaignSteps,
        hasProductionPath, hasOracle, hasSourceFacet, hasSourceAnchorDigest,
        hasCrossSurfaceRole>>

TerminalStutter ==
    /\ classified # "unclassified"
    /\ UNCHANGED vars

Next ==
    DiscoverCampaignSteps \/ DiscoverProductionPath \/
    DiscoverSourceSemanticMetadata \/
    DiscoverRustReproduction \/ DiscoverInvariantViolation \/
    DiscoverProductionGuard \/ DiscoverTheoremGap \/ ClassifyBug \/
    ClassifyGuardedProjection \/ ClassifyFormalStrengthening \/
    ClassifySafe \/ EscalateAudit \/ TerminalStutter

Spec == Init /\ [][Next]_vars

NoSourceFixWithoutRustOrInvariantEvidence ==
    action = "fix_source" => reproduced \/ productionInvariantViolated

ClassifiedWitnessHasAction ==
    classified # "unclassified" => action # "none"

NoUnexpectedTerminalClass ==
    classified \in ClassSet

GuardedProjectionDoesNotFixSource ==
    classified = "projection_risk" => action = "guard"

FormalGapDoesNotDirectlyFixSource ==
    classified = "proof_or_model_strengthening" => action = "strengthen_formal"

ProjectionRiskHasRustGuard ==
    classified = "projection_risk" =>
        /\ guardedByProduction
        /\ hasRustReproducer
        /\ promotionTarget = "rust_guard"

FormalStrengtheningHasInvariantTarget ==
    classified = "proof_or_model_strengthening" =>
        /\ theoremGap
        /\ hasExpectedInvariant
        /\ promotionTarget \in {"rocq", "tla", "sage"}

ConfirmedBugHasSourceTarget ==
    classified = "confirmed_current_bug" =>
        /\ (reproduced \/ productionInvariantViolated)
        /\ promotionTarget = "source_fix"

ClassifiedWitnessHasPromotionTarget ==
    classified # "unclassified" => promotionTarget # "none"

StatefulCampaignNamesSteps ==
    (classified # "unclassified" /\ witness = "stateful_campaign") =>
        hasCampaignSteps

ProductionPathWitnessNamesOracle ==
    (classified # "unclassified" /\
     witness \in {"source_corpus", "production_path_diff"}) =>
        hasProductionPath /\ hasOracle

ExploitCrossProductHasThreatAndSteps ==
    (classified # "unclassified" /\ witness = "exploit_cross_product") =>
        /\ hasCampaignSteps
        /\ hasExpectedInvariant
        /\ threatFamily # "search_governance"

SourceSemanticWitnessHasFacets ==
    (classified # "unclassified" /\ witness = "source_semantic_oracle") =>
        /\ hasSourceFacet
        /\ hasSourceAnchorDigest
        /\ hasCrossSurfaceRole
        /\ hasProductionPath
        /\ hasOracle
        /\ hasRustReproducer

=============================================================================
