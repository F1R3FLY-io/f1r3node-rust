// Finalized-floor Phase-4 verification proptests (LOCAL-ONLY; verification, not
// consensus code). Wired into the `mod` integration-test binary so
// `cargo test -p casper` runs them, and picked up by the finalized-floor gate
// (scripts/check-finalized-floor-ALL.sh) via the `finalized_floor::` filter.
//
//   prop_ft_ppm_provenance — G2: θ_ppm provenance determinism + f32↔ppm round-trip.
//   prop_bonds_from_floor   — P1: committee derivation PLAY ≡ REPLAY.
//   recovery_no_double_apply — T-NDA: the production `canonical_won_sigs` recovery record
//     applies a recovered effect at most once (Recovery.apply_idem / no_double_apply).

//   oracle_stall_replay_spec — exact oracle replays of CI stall instances i1
//     and i5 from committed sub-DAG fixtures: logged-verdict fidelity pins,
//     plus the below-target ancestor-prefix red the walk refinement answers.

mod horizon_read_abstention_spec;
mod oracle_stall_replay_spec;
mod prop_bonds_from_floor;
mod prop_ft_ppm_provenance;
mod recovery_no_double_apply;
