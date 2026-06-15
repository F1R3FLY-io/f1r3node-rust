# 06 · Write a new Hypothesis state machine

## 1 · Prerequisites

- Python ≥ 3.10, Hypothesis ≥ 6.0 installed.
- Familiarity with the existing 12 Hypothesis machines
  (in [`formal/sage/slashing/hypothesis_search/`](../../../../../formal/sage/slashing/hypothesis_search/)
  and the corresponding `casper/tests/slashing/hypothesis_*.rs`
  Rust replays).
- The scenario schema in `scenario_schema.sage`.

## 2 · Skeleton

Create `formal/sage/slashing/hypothesis_search/<machine_name>.py`
(Python; the `.sage` file in this directory hosts the framework
glue):

```python
"""
<one-paragraph purpose>
"""

from hypothesis import given, settings
from hypothesis.stateful import (
    RuleBasedStateMachine, rule, precondition, invariant
)
from hypothesis import strategies as st


class <MachineName>(RuleBasedStateMachine):

    def __init__(self):
        super().__init__()
        self.harness = create_harness(n=4, initial_bond=100)
        # <other state>

    @rule(v=st.text(min_size=1), seq=st.integers(min_value=0, max_value=10))
    @precondition(lambda self: <precondition>)
    def <action_name>(self, v, seq):
        # <execute action>
        self.harness.sign_block(v, seq)

    # ... more @rule actions ...

    @invariant()
    def <invariant_name>(self):
        # <check invariant>
        assert <condition>, f"violated: {<details>}"


TestCase = <MachineName>.TestCase
```

## 3 · Example from this repo

See [`formal/sage/slashing/hypothesis_search/hypothesis_scenario_search.sage`](../../../../../formal/sage/slashing/hypothesis_search/hypothesis_scenario_search.sage)
— 5359 lines, the foundational Hypothesis scenario search.

Smaller per-machine examples are the
[`casper/tests/slashing/hypothesis_*.rs`](../../../../../casper/tests/slashing/)
files (Rust translations of shrunken corpora).

## 4 · Verification step

```sh
pytest formal/sage/slashing/hypothesis_search/<machine_name>.py
```

A failure prints the minimal shrunken counterexample (an
attribute sequence on the state machine). Replay in Rust:

```sh
SLASHING_REPLAY_JSON=/tmp/<witness>.json cargo test -p casper --test mod -- slashing::hypothesis_<name>
```

## 5 · Common pitfalls

- **Missing `@precondition`** — actions with implicit preconditions
  generate infeasible sequences; the framework flags them as
  failures even though they don't represent real defects.
- **Non-determinism in the harness** — use `BTreeMap`/`BTreeSet`
  and accept an explicit RNG seed.
- **Internal-state invariants** — invariants must use the harness's
  *observation API*, not its internal fields.

See [`../randomized-search/02-stateful-hypothesis.md §5`](../randomized-search/02-stateful-hypothesis.md)
for the full pitfall catalog.
