# Resource Limiting for Rocq Proof Compilation

These proofs should be compiled with resource limits to prevent system unresponsiveness,
particularly for modules with complex induction principles or coinductive proofs.

## Recommended Build Command

```bash
cd formal/rocq/cost_accounted_rho
coq_makefile -f _CoqProject -o CoqMakefile
systemd-run --user --scope -p MemoryMax=126G -p CPUQuota=1800% -p IOWeight=30 -p TasksMax=200 make -j1 -f CoqMakefile
```

- **MemoryMax=126G**: 50% of 252GB RAM limit
- **CPUQuota=1800%**: 18 cores maximum
- **IOWeight=30**: Low I/O priority to keep system responsive
- **TasksMax=200**: Process count limit
- **-j1**: Serial compilation (recommended for memory-intensive modular proofs to avoid OOM kills)

## macOS

`systemd-run` is not available on macOS. Use `ulimit` or compile without resource limits:

```bash
cd formal/rocq/cost_accounted_rho
coq_makefile -f _CoqProject -o CoqMakefile
make -j1 -f CoqMakefile
```

## Notes

- `RhoSyntax.v` and `Bisimulation.v` are the most memory-intensive modules
- `RhoSyntax.v` uses `rocq-equations` for mutual recursion, which generates large terms
- `Bisimulation.v` uses `rocq-coinduction` companion approach with multiple up-to techniques
- If compilation is slow, try `-j1` first and increase parallelism only if memory allows
