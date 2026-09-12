# Cleanup Outcome Correspondence

## Failure and correction

B26 injects an error into each Docker cleanup command boundary: container listing, container removal, network pruning, image pruning, and builder pruning.

Each baseline execution observes the command error and a later sufficient disk sample. It still admits work and reports no protection failure.

The correction preserves command failures across the hygiene group. It enables pipeline failure detection so a failed container listing cannot become a successful pipeline result.

The driver reports the failed cleanup stage and returns a failed hygiene result. The existing failure path refuses admission and records one protection failure.

The correction does not change Docker selectors or remove commands. Later cleanup commands can still run after an earlier command fails.

## Model

`CleanupOutcome` represents five command outcomes followed by an admission decision. Each execution has either one failing command or no failing command.

The model assumes a sufficient final disk sample. It summarizes command outcomes in sequence and does not claim that the production pipeline executes sequentially.

The negative control ignores the accumulated failure. It requires exit 12 on `CleanupFailurePreventsAdmission`.

The corrected model has 42 distinct states. Its fairness condition does not establish a deadline or durable publication.

## Additional coverage

Two successful-cleanup cases pass on both baseline and corrected source. Partial reclamation leaves 8000 MiB below the 8192 MiB threshold and prevents admission.

Sufficient reclamation leaves 16384 MiB and permits one iteration. These cases provide characterization coverage, not additional repair cycles.

## Current command mapping

B27 replaces destructive hygiene with read-only inspection. The historical B26 evidence remains unchanged, and the model still represents five command outcomes.

The fixture retains its historical scenario labels:

| Label | Current command |
| --- | --- |
| `list` | `docker ps -aq --filter status=exited --filter name=rnode.` |
| `remove` | `docker inspect cleanup-fixture` |
| `network` | `docker network ls -q` |
| `image` | `docker image ls -q` |
| `builder` | `docker system df` |

The two recovery cases now simulate later disk samples after read-only inspection. They do not establish actual space reclamation.

## Limits

The fixtures replace external Docker commands and disk samples. They do not start real nodes or prove Docker daemon completion.

The model covers one failing command per execution. It does not enumerate every combination of concurrent failures.

Docker ownership, image preservation, complete shutdown, durable evidence, deadline and reserve bounds, hosted checks, and maintainer review remain pending.
