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

Computes a large Fibonacci number (fib(1000000)) using tail recursion,
which on consumer hardware should exceed the timeout of 10 seconds currently
defined for all MeTTa computations.


**MeTTa code:**
```metta
(= (fib-tr $n $a $b) (if (== $n 0) $a (fib-tr (- $n 1) $b (+ $a $b))))
(= (fib $n) (fib-tr $n 0 1))
!(fib 1000000)
```

### Return Value Structure

PeTTa always returns results wrapped in a `{"results": [...]}` JSON object, which is converted to a Rholang map:

```rholang
{
  "results": [result1, result2, ...]  // Rholang list
}
```

## Common Patterns

### Pattern 1: Simple Computation

```rholang
new executePetta(`rho:petta:execute`), retCh in {
  executePetta!("!(+ 1 2)", *retCh) |
  for(@result <- retCh) {
    // result = {"results": [3]}
    match result {
      {"results": [answer]} => {
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

### Timeout Errors

**Cause:** Computation exceeded 10-second limit

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

## Error Handling and Replay Behavior

### Non-Deterministic Operation

`rho:petta:execute` is a **non-deterministic operation**. This means:

1. **During play execution:** PeTTa is invoked and the result (or error) is cached
2. **During replay:** The cached result is used without re-invoking PeTTa
3. **Consensus safety:** All validators must agree on both successes and failures

### Error Recording

When PeTTa execution fails (timeout, syntax error, etc.):
- The error is wrapped in `NonDeterministicProcessFailure`
- The failure is recorded in the event log
- No output is produced to the acknowledgment channel
- During replay, the same error is reproduced from the event log

This ensures that:
- Validators reach consensus on failures as well as successes
- Failed operations don't cause replay divergence
- Contract behavior is deterministic across all validators

### Failure Modes

| Error Type | Description | Replay Behavior |
|------------|-------------|-----------------|
| Timeout | Execution exceeds 10 seconds | Cached failure replayed |
| Syntax Error | Invalid MeTTa code | Cached failure replayed |
| PeTTa Not Found | Missing SWI-Prolog or PeTTa | Cached failure replayed |
| Number Overflow | Result number exceeds i64 | Cached failure replayed |
| Floating Point | Non-integer JSON number | Cached failure replayed |

All failures prevent output from being sent on the acknowledgment channel,
which may cause the calling contract to deadlock or timeout.

## Security Notes

⚠️ **Important Security Considerations: this feature is EXPERIMENTAL.**

1. **Untrusted Code:** Never execute untrusted MeTTa code - there is currently
   no sandboxing at language level
2. **Timeouts:** All execution limited to 10 seconds to prevent DoS
3. **Non-Deterministic:** Results are cached for replay (consensus safety)
4. **Resource Limits:** System memory limits apply
