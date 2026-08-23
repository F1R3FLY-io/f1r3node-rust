# RSpace structural-key preparation measurement metadata

Status: attribution evidence only; not independently reproducible acceptance evidence

Data: [`rspace-structural-key-preparation-2026-08-23.tsv`](rspace-structural-key-preparation-2026-08-23.tsv)

SHA-256: `ee500fd7df6e0a6b95c2675e618db9dcb992b70dd55e65d9042f5431b1a2be80`

Stable repository base: commit `a6b3ad09032c22bb10fafd6f7a890d7871b36609`, tree
`3388f0e80310d9794857d14650138bb45231a2e4`

Recorded envelope:

- release-only ignored diagnostic probe;
- one CPU, recorded as CPU 0 in the TSV;
- systemd `MemoryMax=8589934592` bytes;
- zero swap;
- unary-list depths 1,024, 2,048, 4,096, and 8,192.

Reproducibility limitation: the probe included uncommitted working-tree changes after the stable
base, and neither that exact tree nor the exact shell command was preserved. This metadata does not
reconstruct either missing fact. The samples support phase attribution at the recorded checkpoint;
they must not be used as acceptance or reproducibility evidence. A future repeated experiment must
record the committed source coordinate, executable digest, full command, toolchain, host state, and
preregistered sampling protocol.
