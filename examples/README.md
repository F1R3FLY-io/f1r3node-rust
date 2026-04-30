# Examples

Top-level example files for quick manual checks.

## Included Examples

| File | Purpose |
| --- | --- |
| `hello_world.rho` | Minimal Rholang contract that writes to `rho:io:stdout` |
| `where_receive_guard.rho` | `where`-clause guard on a receive: only consumes messages the guard accepts |
| `where_match_fallthrough.rho` | `where`-clause guard on match cases with fall-through |
| `where_match_as_expression.rho` | `match` used as a boolean expression inside `if` |

## Run With `rholang-cli`

From the repository root:

```bash
cargo run --bin rholang-cli -- examples/hello_world.rho
```

## More Examples

The larger collection of contract examples lives under:

```text
rholang/examples/
```
