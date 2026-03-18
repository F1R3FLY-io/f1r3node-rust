# graphz

Small helper crate for generating Graphviz DOT output.

## Responsibilities

- Build graphs and digraphs programmatically
- Serialize output to strings, lists, or files
- Support the DAG visualization features used elsewhere in the workspace

## Build

```bash
cargo build -p graphz
cargo build --release -p graphz
```

## Test

```bash
cargo test -p graphz
```

## Usage

Add the crate as a path dependency:

```toml
[dependencies]
graphz = { path = "../graphz" }
```

Key implementation file:

```text
graphz/src/rust/graphz.rs
```
