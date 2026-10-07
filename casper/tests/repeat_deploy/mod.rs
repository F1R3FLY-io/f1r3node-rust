// Repeat-deploy verification proptests (LOCAL-ONLY; verification, not consensus code).
// Wired into the `mod` integration-test binary so `cargo test -p casper` runs them.
//
//   prop_repeat_deploy_agreement — P7: proposer ↔ validator AGREEMENT on the repeat-
//   deploy expiration window (`earliest = block_number - deploy_lifespan`). An honest
//   recovery block is never wrongly flagged InvalidRepeatDeploy, because both sides gate
//   on the SAME `canonical_won_sigs` record.

mod prop_repeat_deploy_agreement;
