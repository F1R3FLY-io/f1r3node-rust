# 05 · Write a new Sage model

## 1 · Prerequisites

- Sage installed: `sage --version` returns a version ≥ 9.0.
- Familiarity with the existing 30 Sage scripts
  ([`formal/sage/slashing/`](../../../../../formal/sage/slashing/)).
- The scenario schema
  ([`formal/sage/slashing/scenario_schema.sage`](../../../../../formal/sage/slashing/scenario_schema.sage)).
- The methodology's family taxonomy in
  [`../sage-models/`](../sage-models/).

## 2 · Skeleton

Create `formal/sage/slashing/<model_name>.sage`:

```python
"""
<one-paragraph purpose>

This model searches <strategy space> using <Sage facility>.
Emits witnesses for <property> classified as <expected class>.
"""

import argparse
import json
import sys

from sage.all import DiGraph, Integer, Subsets, binomial, ZZ, vector
# (import only what you need; Sage's `from sage.all import *` is heavy)


def <core_search>(n, ...):
    """<one-sentence purpose>."""
    # <exact-arithmetic enumeration or graph computation>
    yield {
        "kind": "<model_name>_witness",
        "n": n,
        ...
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--seed", type=int, default=0,
                        help="deterministic seed for reproducibility")
    parser.add_argument("--n-max", type=int, default=5)
    parser.add_argument("--output", type=str, default="-")
    args = parser.parse_args()

    out = sys.stdout if args.output == "-" else open(args.output, "w")
    for witness in <core_search>(args.n_max, ...):
        out.write(json.dumps(witness) + "\n")


if __name__ == "__main__":
    main()
```

## 3 · Example from this repo

See [`formal/sage/slashing/closure_model.sage`](../../../../../formal/sage/slashing/closure_model.sage)
— 384 lines, implements the two-level closure with iterative +
transitive-closure cross-check.

## 4 · Verification step

```sh
sage formal/sage/slashing/<model_name>.sage --seed 42 --output - | head
```

Each line is a JSON witness; pipe through `jq .` to pretty-print.

Replay any witness through the Rust path:

```sh
SLASHING_REPLAY_JSON=$(mktemp) sage formal/sage/slashing/<model_name>.sage --output $SLASHING_REPLAY_JSON
SLASHING_REPLAY_JSON=$SLASHING_REPLAY_JSON cargo test -p casper --test mod -- slashing::generated_
```

## 5 · Common pitfalls

- **Non-deterministic output** — always accept `--seed`; use
  `set_random_seed(seed)` from `sage.all`.
- **Wrong family** — assign the model to one of the 14 families in
  [`../sage-models/`](../sage-models/); if none fits, define a new
  family before writing the model.
- **State-space explosion** — every search must be objective-
  guided (see
  [`../sage-models/07-horizon-and-objective-frontier.md`](../sage-models/07-horizon-and-objective-frontier.md))
  for `n > 5`.
- **Missing classification** — every witness must carry the
  `classification` field per the scenario schema; the
  corpus-generator rejects fixtures without it.

See [`../formal-methods/04-finite-modeling-sage.md §5`](../formal-methods/04-finite-modeling-sage.md)
for the full pitfall catalog.
