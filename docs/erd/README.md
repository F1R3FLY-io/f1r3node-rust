# Entity-Relationship Diagrams

This directory describes node data in memory and on disk.
The diagrams use PlantUML, as requested for this documentation.
Each diagram has a `.puml` source and a rendered `.svg` file.
GitHub can display the SVG files without a PlantUML extension.

## Start Here

| View | Contents |
| --- | --- |
| [Memory](memory/README.md) | Runtime relationships and the complete declared-type catalog within the scope below |
| [Storage](storage/README.md) | Named databases, record encodings, snapshots, files, and logical contract state |
| [Type inventory](memory/inventory.md) | Searchable source-to-diagram index |
| [Machine-readable inventory](memory/inventory.json) | Exact declared fields, source locations, and referenced type names |
| [Memory cardinality and size](memory/sizing.md) | Per-field assessments, ownership rules, and production owner limits |
| [Storage cardinality and size](storage/sizing.md) | Record-count growth, byte formulas, and physical accounting |
| [Runtime observations](storage/observed.md) | Timestamped counts, page sizes, and bounded record-size samples |

## Scope

The catalog scans repository-owned `src` trees for the node, deploy queue, interpreter, and their workspace libraries.
It also includes `rho-pure-eval` and `scripts/file-read-service`.
The crate list is explicit in [generate.rs](generate.rs).

The catalog includes Rust structs, enums, type aliases, statics, and protobuf messages and enums.
It also reads `thread_local!` and `lazy_static!` declarations.
It retains private and function-local declarations.
It includes production feature alternatives, even when the current build does not enable them.
This is a source inventory, not a heap capture or a reachability analysis of one binary.

The scan excludes test-gated declarations, conventional test paths and filenames, benchmarks, build scripts, third-party implementations, and development harnesses.
Helpers that compile without a test gate can remain in the catalog.
Generic parameters and trait objects remain symbolic.
The catalog does not expand arbitrary Rust macros or infer compiler-generated layouts.
Generated protobuf values come from descriptors, not an old build directory.

All runtime values also have a physical memory representation.
Their presence in the memory catalog does not mean that they remain resident for the node's full lifetime.
Their presence also does not mean that the node writes them to disk.

The storage view covers the registered node databases and the deploy queue's separate databases.
It distinguishes active schemas from registered but unused stores.
The filesystem view covers the implemented WAL, snapshot, payload, configuration, and diagnostic forms.
Arbitrary user-contract state uses the tuple-space value grammar, not a fixed node-level schema.

## Notation

- An entity box describes a type, logical record, or named database.
- `* key` identifies a record key. It does not declare an SQL primary key.
- Cardinalities describe logical references where the focused diagram can establish them.
- An arrow does not imply ownership, an atomic write, or an enforced foreign key.
- Enum rows are alternatives, not fields that coexist in every value.
- Focused diagrams show selected fields. Catalog diagrams show every field found by the parser.

Catalog arrows connect field types that resolve within the same page.
The resolver accepts an exact local declaration or a unique catalog name.
It omits ambiguous names rather than guessing an import target.
Cross-page references appear beside each catalog diagram in its Markdown page.
The JSON inventory retains type references even when no arrow can resolve them.

Storage keys can refer to a block whose body was pruned.
The DAG, deploy lifecycle, and carrier indices have different retention rules.
Do not infer a required body row from a hash reference.

## Regenerate

Requirements: Rust, Cargo, Java, and `protoc` with proto3 optional-field support.
Use `PROTOC` to select a compiler when the system compiler is too old.
The generator is a separate Cargo workspace and does not build the node.

1. Download the pinned renderer into the ignored tools directory:

   ```bash
   mkdir -p target/erd-tools
   curl -fL -o target/erd-tools/plantuml-1.2026.10.jar \
     https://github.com/plantuml/plantuml/releases/download/v1.2026.10/plantuml.jar
   ```

2. Generate the catalog and render all diagrams:

   ```bash
   bash docs/erd/render.sh
   ```

3. Check generated sources, diagram syntax, render hashes, database coverage, and local documentation links:

   ```bash
   bash docs/erd/render.sh --check
   ```

The script verifies the renderer's SHA-256 digest before execution.
It records source and SVG hashes in [rendered.sha256](rendered.sha256).
Check mode rejects changes to rendered files or their PlantUML inputs after generation.
It uses the embedded Smetana layout engine and does not need Graphviz.
It uses two rendering threads and a 1 GiB Java heap limit.
No source or diagram content is sent to a public rendering service.

Run generator tests with the repository test lock:

```bash
CARGO_TARGET_DIR="$PWD/target/erd-tools/build" \
  scripts/ci/run-tests.sh cargo test --locked --manifest-path docs/erd/Cargo.toml
```

The storage schemas and focused memory diagrams require source review after changes.
The type catalog regenerates automatically.
These checks do not establish runtime correctness or validate an existing shard database.

See the official [PlantUML command-line documentation](https://plantuml.com/command-line) and [layout engine documentation](https://plantuml.com/layout-engines).
