# Docker Cleanup Ownership Correspondence

B27 runs disk hygiene against a real Docker daemon on a disposable virtual machine (VM). The fixture creates an unrelated stopped container, unused network, and untagged image.

The baseline deletes all three resources. The corrected driver performs read-only inspection and refuses admission at 7000 MiB with an 8192 MiB threshold.

`DockerCleanupOwnership` visits each resource once. `DeleteUnowned` selects deletion or preservation. The corrected model has eight distinct states.

The negative control requires exit 12 on `UnownedDockerResourcesPreserved`. `Completes` uses weak fairness and does not establish a response deadline.

The fixture substitutes disk samples and workload commands. Docker commands reach the real daemon. The fixture records resource identities before and after driver execution.

The imported image is a small fixture image, not the node image. This test does not establish the full node-image lifecycle or safe reclamation.

B27 removes unscoped deletion from disk hygiene only. Stop commands still use name and process selectors, which do not establish ownership.

The driver can refuse work earlier because it no longer requests Docker reclamation. D2, reserve bounds, durable publication, hosted checks, and maintainer review remain pending.

See the [B27–B29 evidence](../../../docs/cbc-evidence/soak-d2-real-system-2026-09-10/README.md).
