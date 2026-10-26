# Campaign control model

The bounded model checks five launch slots and two competing controllers. The positive configuration checks submission limits, approval, scheduling, confirmed cleanup, and retained product failures.

Each negative configuration enables one defect. The verifier requires exit 12, the registered invariant, and a counterexample that starts with state 1.

| Defect | Required invariant |
| --- | --- |
| Approval bypass | `AuthorizedLaunch` |
| Repeated submission | `OneSubmission` |
| Missing schedule | `ScheduledLaunch` |
| Unobserved termination | `ConfirmedCleanup` |
| Incomplete prerequisites | `PriorStagesPassed` |

Run `scripts/casper-soak/check-campaign-control.sh OUTPUT` with `TLA_TOOLS_JAR` set to the pinned TLC 1.7.4 JAR. Set `SOAK_CAMPAIGN_JAVA` when the default Java executable is unsuitable.

The gate retains verifier logs, process exits, exact input digests, fixture results, and source manifests. Timeouts, incomplete searches, wrong invariants, and tool errors fail verification.

On macOS, add GNU coreutils to `PATH`. Docker runs only reservation fixtures in a minimal image built from two hashed static executables.

The reservation check requires the matching Rust musl target. No node binary enters the test image.

The model abstracts approval authentication, conditional storage writes, and provider operations. It does not prove those interfaces or establish deployed timing guarantees.

The five negative controls support `CLAIM-CASPER-CAMPAIGN-003`. The planning and local reservation claims also require their own requirement coverage and implementation evidence.

Passing model checks do not accept a claim, qualify a node adapter, or authorize deployment. Source-bound review and explicit acceptance remain required.
