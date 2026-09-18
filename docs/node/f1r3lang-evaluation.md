# F1R3Lang evaluation composition

The `node` feature `mettail-frontend` connects the MeTTaIL-generated Rholang
parser to the existing node interpreter. It changes the public eval/REPL
frontend, not the interpreter or the consensus deployment protocol. Activation
requires the checked preparation, installed-language matcher, and end-to-end
application tests to pass together; this document is not a passing-test receipt.

## Request contract

Each request is serialized against its runtime. The generated parser runs once;
the checked preparation session admits imports and constructs one owned
`PreparedProgram`. Display borrows that artifact. The existing signed, metered
interpreter consumes the same artifact without parsing source again.

The frontend calls the generated `Proc::parse_via_wpda` entrypoint, also used
by inline-module installation tests. This entrypoint applies the parser's
existing elected-source semantics and complete-input checks. The adapter adds
no candidate ranking, source reparse, or enumeration-limit increase; parser
errors remain preparation failures. A successful elected parse is not a claim
that the source has a unique derivation or that every ambiguity was preserved.
Exhaustive ambiguity preservation remains a separate parser obligation.

One `RholangLanguageRuntime` owns installed-language capabilities for all
install, parse, construct, pattern, semantic, and theorem services. The matcher
uses that same instance. Registration checks compare the actual system-service
URNs, channels, and body references before constructing the node runtime.

Preparation failure publishes no program. Execution errors and undecidable
guard refusals restore the existing RSpace soft checkpoint before the request
returns. A false guard is not an error. This checkpoint covers RSpace state;
it does not claim to undo external effects or successful language installations
already performed by system services. Request cancellation/unwind recovery is
not an additional guarantee of this composition.

## Host policy

The server requires all of these environment settings at startup. There is no
implicit unlimited evaluation budget. The source program cannot change them.

| Setting | Meaning |
| --- | --- |
| `F1R3LANG_EVAL_PHLO` | Positive host-funded token grant per evaluation, below the maximum signed 64-bit value |
| `F1R3LANG_MAX_SOURCE_BYTES` | Source byte limit, checked before the blocking-worker source copy |
| `F1R3LANG_MAX_IMPORT_ENTRIES` | Maximum caller-import entries |
| `F1R3LANG_MAX_IMPORT_NODES` | Maximum admitted import structural occurrences |
| `F1R3LANG_MAX_IMPORT_BYTES` | Maximum import payload bytes |
| `F1R3LANG_PREPARATION_WORK` | Checked lowering work limit |
| `F1R3LANG_PREPARATION_UNITS` | Checked lowering logical storage limit, not physical resident memory |

For example, an operator may explicitly select the following local-test policy.
These are example grants, not measured minimum requirements or production
recommendations:

```sh
export F1R3LANG_EVAL_PHLO=10000000
export F1R3LANG_MAX_SOURCE_BYTES=1048576
export F1R3LANG_MAX_IMPORT_ENTRIES=1024
export F1R3LANG_MAX_IMPORT_NODES=1000000
export F1R3LANG_MAX_IMPORT_BYTES=16777216
export F1R3LANG_PREPARATION_WORK=100000000
export F1R3LANG_PREPARATION_UNITS=100000000
```

This feature requires the native-operation profile audited by MeTTaIL's
`runtime/build.rs`: the standard x86-64, 64-bit Rust compiler at commit
`2e2b193f8ada105f27608b7be81c293e0d7292cb`, distributed as
`nightly-2026-09-03`. The older default node toolchain does not enable that
profile. It can compile the feature, but checked preparation then refuses
profile-dependent operations with `UnsupportedProfile`. This refusal protects
the correspondence between native-operation bounds and the audited standard
library; do not override its configuration flags to force acceptance.

Select the audited toolchain explicitly when building the existing node:

```sh
cargo +nightly-2026-09-03 build --locked -p node --features mettail-frontend
```

Use that binary's existing `run --standalone`, `eval`, and REPL commands.
For the complete Regex application, allow a longer client wait explicitly:

```sh
target/debug/node --grpc-host=127.0.0.1 --grpc-port=40402 eval --timeout 5m \
  ../mettail-module-dev/mettail-rust/rholang-runtime/tests/fixtures/regex_gslt_application.rho
```

`eval --timeout` accepts a positive duration such as `30s`, `1500ms`, or `5m`.
Its default remains `30s`; durations outside the platform clock range refuse
before connection. The limit applies separately to each evaluation RPC,
including server preparation and execution, not to the entire batch of files.
File reading is outside this RPC limit, and the connection timeout remains
five seconds. Increasing the wait does not change host funding, preparation,
or semantic-work limits. Expiration stops the client's wait; it does not
guarantee cancellation or rollback of work already running on the node.

Non-standalone startup explicitly refuses. The generated source parser keeps
its existing canonical generalized-LL and ambiguity-realization limits.
Source-byte and preparation limits do not substitute for those parser limits.
Installed-language parsers use the existing finite `RuntimePolicy` defaults;
the host grants the existing native FLT rights, not publication or bridging
authority. Compile-time guard discharge is disabled on this route so runtime
guard evaluation remains visible.

## Deliberate boundaries

The startup composition supplies an empty immutable registry snapshot: inline
`Module`/`Theory` definitions work within the admitted source profile; registry
and filesystem loading are unavailable. The constructor accepts an injected
registry snapshot for a subsequent registry-backed composition. No filesystem
I/O implementation is introduced.

Signed deploys, exploratory deploys and queries implemented by exploratory
source execution, bond-status queries, and legacy LSP validation explicitly refuse under this
feature before invoking their legacy source paths. Non-source state queries
remain separate. This is not a production consensus/replay cutover. A service
constructed without the shared F1R3Lang composition also refuses evaluation;
it cannot silently fall back to Tree-Sitter. Incoming Casper peer packets
explicitly refuse without decoding or executing their source.

The frozen source-route inventory distinguishes the admitted application path
from trusted baseline startup and separately built tools:

| Inventory family | This composition |
| --- | --- |
| Node eval/gRPC | Single checked MeTTaIL preparation and existing metered execution |
| LSP validation | Explicit refusal before legacy validation |
| Casper deploy admission | Public HTTP/gRPC deployment and peer packet ingress refused |
| Casper interpreter utilities | Not called by admitted eval; new public/peer source ingress refused |
| Casper runtime queries | Public exploratory and bond-status routes refused |
| Standalone Rholang CLI binary | Separate legacy tool, not used by the node eval client or service |
| Source artifact builder | Existing trusted genesis/bootstrap source remains baseline startup, not admitted application source |

This distinction does not claim that legacy parser code is removed from the
binary or that arbitrary historical-chain replay is supported by the new
frontend. Production consensus and replay activation remain separate work.

## Verification boundary

The node's `PreparedProgramAdmission.v` model proves exact prepared-artifact
handoff, single preparation, metered funding, and checkpoint-wrapper laws.
The bridge's preparation models supply its resource/session contracts. These
are model proofs with source correspondence, not a proof of all Rust code.
`ProviderRegistration.v` additionally proves the finite registration check:
accepted candidates avoid reserved keys and every earlier registration, the
successful roster is unchanged, and refusal publishes nothing. Actual channel
equality and enumeration of the built-in Rust definitions remain source
correspondence obligations.
Focused tests cover explicit policy refusal, real registration collisions,
single preparation without source reparse, and unconfigured-route refusal.
The actual Regex application through the public service remains the decisive
integration test.

## Run the focused application check

From this node worktree, with the audited compiler selected, run:

```sh
cargo +nightly-2026-09-03 test --locked -p node \
  --no-default-features --features mettail-frontend --lib \
  rust::api::repl_grpc_service::prepared_route_tests \
  -- --nocapture --test-threads=1
```

The application source is the sibling workspace file
`../mettail-module-dev/mettail-rust/rholang-runtime/tests/fixtures/regex_gslt_application.rho`.
It contains the complete `Module RegexGSLT` and `Theory Regex`, declared rewrite
rules, installation, qualified FLT construction and matching, semantic
observations, and a regex `where` guard. The test calls the actual `Repl::eval`
service with that source and inspects the existing interpreter's RSpace.
It does not substitute a test evaluator or native regex engine.

Expected observations are nullable `true`, derivative `a*`, full match `true`,
search byte span `[2,6)` containing `λλ`, replace-first `xba`, replace-all `xbx`,
and guarded consumption of `abcb`. The nonmatching `ax` remains on the input
channel. Search and replacement captures are structural guest `Text` terms,
not automatically unwrapped host strings; their private-tag representation in
stdout is expected. The test independently derives the declaration's language
commitment and compares the exact reflected values and their metadata.

The other tests in this group check one-time preparation, unavailable-route
refusal, finite funding, rollback, and clean subsequent requests. This focused
check is separate from standalone CLI startup, full public-preparation resource
closure, production deployment/replay, and the campaign's final acceptance gates.
