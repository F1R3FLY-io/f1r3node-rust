// Finalized-floor Phase-4 verification proptests (LOCAL-ONLY; verification, not
// consensus code). Wired into the `mod` integration-test binary so
// `cargo test -p casper` runs them, and picked up by the finalized-floor gate
// (scripts/check-finalized-floor-ALL.sh) via the `finalized_floor::` filter.
//
//   prop_ft_ppm_provenance — G2: θ_ppm provenance determinism + f32↔ppm round-trip.
//   prop_bonds_from_floor   — P1: committee derivation PLAY ≡ REPLAY.

mod prop_bonds_from_floor;
mod prop_ft_ppm_provenance;
