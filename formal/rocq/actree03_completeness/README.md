# ACTree03 canonical writer and validator completeness

Status: kernel-checked formal authority; executable correspondence refinement in progress

This Rocq project proves that the stack-safe ACTree03 validator accepts every
canonical byte image emitted by the modeled PathMap writer. It closes the
previous gap between structural EPM1 envelope proofs and the concrete arena
writer/parser contract.

The development proves, without `Admitted`, `Axiom`, or caller-supplied fuel
assumptions:

- exact ACTree03 header, root, arena, and trailer boundaries;
- discovery of every reachable semantic node with unique physical identities;
- strict child order, exact child-root layout, and source-node indexing;
- canonical line allocation, equal-byte reuse, hash-collision overwrite, and
  A/B/A re-emission behavior;
- equivalence between recursive semantic replay and the explicit enter/finish
  pushdown automaton; and
- acceptance of every canonical ordinary-writer output by the direct
  validator.

The numbered filenames preserve the audited proof lineage. The final authority
is `Epm1ActConcreteV5.v`; earlier files provide definitions and lemmas imported
by that capstone rather than competing production specifications. The two
`Mutants.v` files are negative controls and must continue to compile their
counterexamples against deliberately altered decisions.

Run the project under a hard memory ceiling:

```bash
systemd-run --user --scope \
  -p MemoryMax=1536M -p MemorySwapMax=0 \
  coqc -Q theories '' theories/Epm1CursorAuthorityV2.v
# Continue in _CoqProject order through Epm1ActConcreteV5Mutants.v.
```

The repository gate `scripts/check-stack-safe-pda-proofs.sh` performs that
ordered compilation. Executable properties and their theorem bindings are
specified by the companion ACTree03 invariant catalog under
`docs/design/pathmap/`.
