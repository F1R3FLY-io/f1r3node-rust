# Stack-safe PDA and EPathMap proof kernel

This Rocq development proves the algorithmic obligations shared by the generated Rust machines.

- `compile_run_equivalence` proves that an explicit post-order instruction/value-stack machine equals the mutually recursive fold for every finite typed tree and every algebra. Clone, equality, hashing, ordering, debug formatting, bincode/protobuf encoding, encoded length, clear, and teardown are instances of that theorem.
- `decoder_machine_equivalent_to_recursive_rebuild` instantiates the same machine with the tree-building algebra and proves exact reconstruction.
- `compiled_program_has_one_instruction_per_node` proves linear work and one completion instruction per node.
- `EPathMap.v` proves neutral-empty mode laws, set lattice laws, exact map-value conflict behavior, the EPM1 header round trip, and the prefix-restriction subset law.

The theorem kernel is schema-parametric. The generated Rust equivalence manifest binds each concrete schema surface to a theorem and an independent recursive-oracle differential. The manifest test is the coverage proof: a new generated traversal cannot remain converted without a theorem/differential row. Concrete byte parsing and PathMap ACTree03 behavior remain executable-library obligations and are checked against the retained implementations by the Rust differential and malformed-input suites.

The development contains no axioms, admitted goals, or opaque assumptions.
