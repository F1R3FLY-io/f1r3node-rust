# PeTTa Tests Summary

This document summarizes the test suite implemented for the PeTTa (MeTTa +
SWI-Prolog) execution functionality in RNode.

## Requirements

For the PeTTa tests to run, the following requirements must be met:

1. **The PeTTa installation must be available**.

   See the "MeTTa / PeTTa" prerequisites section in the top-level
   [README.md](../../README.md#source-development) for details.

   The installation location must be specified in the `PETTA_PATH` environment
   variable. If this variable is not set, the relative path `./PeTTa` will be used,
   which may fail or not depending on how the tests are run. Therefore, setting this
   variable is encouraged.

2. **`swipl` must be available in the `PATH`**.

   See the "MeTTa / PeTTa" prerequisites section in the top-level
   [README.md](../../README.md#source-development) for how to install this program.

If these are not available, the tests will be skipped by default. If this behaviour
is not desirable (e.g: all tests must be run as part of CI), then the environment
variable `REQUIRE_PETTA_TESTS` must be set.

## Test Coverage Overview

The test suite consists of:
1. Unit tests for `value_to_par` function
2. Unit tests for `petta_execute` function  
3. Integration tests with Rholang runtime
4. Replay tests to verify deterministic re-execution during consensus replay

## Test Files

### 1. Unit Tests: `value_to_par` Function and `petta_execute`
**Location:** `rholang/src/rust/interpreter/swi_prolog_service.rs` (lines 127-360)

**Coverage:** 18 unit tests

Tests JSON→Par conversion for all data types:
- Basic types: `null`, `boolean`, `number`, `string`
- Collections: `array`, `object`
- Nested structures: nested arrays, nested objects.
- **Timeout test:** Large fibonacci computation that exceeds the 10-second timeout of the NORMAL-mode `petta_execute` path

**Run:** `PETTA_PATH=/path/to/PeTTa cargo test --package rholang --lib swi_prolog_service::tests`

### 2. Direct Execution Tests
**Location:** `rholang/tests/swipl_petta_execution_spec.rs`

**Coverage:** 9 tests

Tests the `petta_execute` function directly:
- `test_petta_execute_simple_swap` - Basic pattern matching
- `test_petta_execute_fibonacci` - Recursive function execution
- `test_petta_execute_simple_arithmetic` - Simple operations
- `test_petta_execute_invalid_syntax` - Error handling
- `test_petta_execute_empty_code` - Edge case handling
- `test_petta_execute_timeout_large_fibonacci` - Timeout enforcement for long-running computations
- `test_petta_cache_is_keyed_on_lib_import_content` - A changed PeTTa tree never gets the cached patch of another tree

**Run:** `PETTA_PATH=/path/to/PeTTa cargo test --package rholang --test swipl_petta_execution_spec`

### 3. Integration Tests with Rholang Runtime
**Location:** `rholang/tests/swipl_petta_integration_spec.rs`

**Coverage:** 16 tests

The direct-execution cases through the Rholang runtime, plus consensus mode:
lockdown escape cases, library imports under lockdown, the inference budget, and phlo
(`test_petta_cost_grows_with_inferences`, `test_petta_underfunded_deploy_does_not_spawn`,
`test_petta_run_without_inference_count_pays_full_budget`).

**Run:** `PETTA_PATH=/path/to/PeTTa cargo test --package rholang --test swipl_petta_integration_spec`

### 4. Replay Tests (Deterministic Re-Execution Verification)
**Location:** `rholang/tests/swipl_petta_replay_spec.rs`

**Coverage:** 13 tests

Every node runs PeTTa again on replay. The event log records only a hash of the
result and the frames. The tests check that each limit gives the same reason and
cost in play and replay, that play records the hash, and that a changed hash
logs a divergence and still produces the fresh result.

**Run:** `PETTA_PATH=/path/to/PeTTa cargo test --package rholang --test swipl_petta_replay_spec`

The NODE-mode tests need mlabs-haskell/PeTTa b9ca1884 in `PETTA_DIR`.

## Running All Tests

### With PeTTa Installed
```bash
# Set PeTTa path
export PETTA_PATH=/path/to/PeTTa

# Run all PeTTa tests
cargo test --package rholang --test swipl_petta_execution_spec
cargo test --package rholang --test swipl_petta_integration_spec  
cargo test --package rholang --test swipl_petta_replay_spec

# Run unit tests
cargo test --package rholang --lib swi_prolog_service::tests
```

### With Docker (no local SWI-Prolog / PeTTa needed)

The `petta-test` stage in [node/Dockerfile](../../node/Dockerfile) runs the whole
suite inside a container that already provides the janus-capable SWI-Prolog and
the pinned PeTTa. From the repository root:

```bash
# Build the test image once (reuses the node build cache)
docker build --target petta-test -t f1r3fly-petta-test -f node/Dockerfile .

# Run the full PeTTa test suite. The named volumes cache compiled crates and
# artifacts, so repeated runs only re-run the tests, not a full rebuild.
docker run --rm \
  -v petta-cargo:/usr/local/cargo/registry \
  -v petta-target:/build/target \
  f1r3fly-petta-test
```

The container sets `PETTA_PATH` and `RUSTFLAGS` and invokes
`rholang/tests/RUN_PETTA_TESTS.sh` for you; the run exits non-zero if any test
fails.
