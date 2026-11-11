# PeTTa (SWI-Prolog + MeTTa) Examples

This directory contains Rholang examples demonstrating the `rho:petta:execute`
system contract, which provides access to the MeTTA language.

## What is PeTTa?

**PeTTa** is an interpreter for MeTTa written in SWI-Prolog.

This integration allows Rholang contracts to perform advanced symbolic reasoning,
pattern matching, and AI-style computations.

## Prerequisites

PeTTa (the MeTTa interpreter) and `swipl` must be available on your system for
`rho:petta:execute` to work when running the node locally. See the "MeTTa /
PeTTa" prerequisites section in the top-level
[README.md](../../../../README.md#source-development) for installation instructions.
When running the node in Docker, PeTTa is already included in the image.

## MeTTa Execution Model

MeTTa programs provided to the `rho:petta:execute` contract are executed as a
sequence of definitions and queries in the order they appear. There is no
required entry point - definitions are loaded and queries are evaluated as
encountered.

## Examples Overview

### 01-swap.rho - Pattern Matching Basics

Demonstrates basic MeTTa pattern matching by defining a `swap` function that reverses a pair.

### 02-fib-long.rho - Recursive Computation

Computes a large Fibonacci number (fib(1000000)) using tail recursion. The
result is larger than the 64 KiB integer limit, so the call fails with
`max_integer_size_exceeded`. The failure is the same on every node.


**MeTTa code:**
```metta
(= (fib-tr $n $a $b) (if (== $n 0) $a (fib-tr (- $n 1) $b (+ $a $b))))
(= (fib $n) (fib-tr $n 0 1))
!(fib 1000000)
```

### Return Value Structure

The acknowledgment channel receives a Rholang list with one element for each
`!` query of the program:

```rholang
[result1, result2, ...]
```

## Common Patterns

### Pattern 1: Simple Computation

```rholang
new executePetta(`rho:petta:execute`), retCh in {
  executePetta!("!(+ 1 2)", *retCh) |
  for(@result <- retCh) {
    // result = [3]
    match result {
      [answer] => {
        stdout!(answer)  // Prints: 3
      }
    }
  }
}
```

### Pattern 2: Multiple Calls

```rholang
new executePetta(`rho:petta:execute`), ret1, ret2 in {
  executePetta!("!(+ 1 2)", *ret1) |
  executePetta!("!(* 3 4)", *ret2) |
  
  for(@r1 <- ret1; @r2 <- ret2) {
    stdout!([r1, r2])
  }
}
```

## Troubleshooting

### Error: "Can't find PeTTa"

**Cause:** `$PETTA_PATH` points to invalid location

**Solution:**
```bash
# Check PeTTa location
ls ./PeTTa/src/metta.pl

# Or set explicitly
export PETTA_PATH=/full/path/to/PeTTa
```

### Error: "swipl: command not found"

**Cause:** SWI-Prolog not installed or not in PATH

**Solution:**
```bash
# Install SWI-Prolog
brew install swi-prolog  # macOS
apt-get install swi-prolog  # Ubuntu/Debian

# Verify
which swipl
```

### Resource Limit Errors

**Cause:** The program passed a deterministic resource limit. The error reason
names the limit, for example `inference_limit_exceeded`.

**Solutions:**
1. Break computation into smaller steps
2. Use more efficient algorithms
3. Pre-compute complex results off-chain

### No Output

**Cause:** Error occurred (doesn't send on ack channel)

**Solution:** Check node logs for error details:
```bash
tail -f ~/.rnode/rnode.log
```

## Consensus Execution

Every validator runs the program again during replay. The event log records only
a 32-byte hash of the result and the `println!` frames. On a different hash, the
validator logs an error on the `f1r3fly.petta` target with the deploy, both
hashes, and the inference count. It then uses its fresh result, and replay
validation rejects the block.

PeTTa runs without network under `consensus_lockdown/1`, which keeps only the
functions in `PETTA_CONSENSUS_ALLOWED_FUNS`. Clock, sleep, entropy, host access,
assertion, and raw Prolog forms fail with `consensus_forbidden(<name>)` or stay
inert. Seeded `rand-*` stays available. Programs are self-contained: under
consensus, `import!` accepts only the standard library of the pinned fork, and
`PLN_DIR` has no effect on the node path.

| Limit | Value | Error reason |
|-------|-------|--------------|
| Inferences | 10,000,000 | `inference_limit_exceeded` |
| Stack | 1 GiB | `stack_limit_exceeded` |
| Integer size | 64 KiB | `max_integer_size_exceeded` |
| Output (frames and result) | 2^20 characters | `output_limit_exceeded` |
| Address space (liveness only) | 4 GiB | `memory_limit_exceeded` |
| Wall clock (liveness only, node fault) | 600 s | backstop error |

Other Prolog errors give the formal error term without its context. Every
failure sends no output on the acknowledgment channel.

**Phlo.** A call costs 100,000 phlo before the sandbox starts and 1 phlo for each
inference after the run. A run without an inference count pays the full budget.

**Pins.** The consensus fingerprint holds the limits, the phlo rate, the
allowlist, `SWIPL_VERSION`, and the tree hash of the PeTTa sources
(SHA-256 over `path\0content` of each file in sorted path order). When
`PETTA_DIR` is set, the node runs `check_backend()` at startup and exits on a
mismatch. `test_petta_backend_matches_pins` checks a local backend.

**Known limit.** One builtin call can run for a long time with few inferences,
for example `term_to_atom` of a large shared term.

## Security Notes

⚠️ **This feature is EXPERIMENTAL.** PeTTa runs in a bubblewrap sandbox with a
seccomp filter and no network, under the consensus lockdown and the limits above.
