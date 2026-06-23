# Examples

Top-level example files for quick manual checks.

## Included Examples

| File | Purpose |
| --- | --- |
| `hello_world.rho` | Minimal Rholang contract that writes to `rho:io:stdout` |
| `where_receive_guard.rho` | `where`-clause guard on a receive: only consumes messages the guard accepts |
| `where_match_fallthrough.rho` | `where`-clause guard on match cases with fall-through |
| `cost_accounting_demo.rho` | Cost-accounting showcase: a token-gated buyer–seller ecosystem exercising every W1 surface form (ground/compound/lollipop/`#P` signatures, budget stacks, ring-fencing, per-clause signed binds, N-ary joins); money & inventory conserved |
| `multi_sig_treasury.rho` | Multi-signature treasury: `(*)` joint authority (2-of-2, 3-of-3, combined-cell token), a 2-of-3 quorum via racing pairs over a one-shot grant, four `-o` lollipop delegation forms (plain, co-authorized, delegate-to-a-pair, and a multi-hop nested pipeline), a ring-fenced reserve, a receive-side endorsement join, and a `#` charter signature; money conserved at 640 |

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
