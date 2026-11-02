# FX-6 decisions: a seal must be written by its owner

Fix FX-6 (A-7 finding F28), Decision 62, rev-stacks patch plan r8.3, lane C.
Branch `patch/FX-6`, cut from `patch/FX-2` (4aed4d852). It must land before
FX-2 merges anywhere.

## Finding

FX-2 decision 12 checked a sealed region that arrives as data against the
origin of the code that evaluates it. With F19 open, the gate admits a
variable seal over a stack body. M can therefore build `{% a %}[stack]` with
`a` bound to A's deployer id and send it to a contract of A's that evaluates
what it receives (`*p`). The region then ran under A's record and drew on A's
wallet (A-7 F28).

## Decisions

| # | Decision | Alternatives | Reason | Reversible |
|---|----------|--------------|--------|------------|
| 1 | `SealRecord` gains `from_data`, and `SealRecordProto` gains `bool from_data = 4`. Only the reducer sets it. | A separate task-local flag. | The flag must survive storage: data that stores a continuation must not get clean authority when the continuation fires later. | Until a shard holds sealed continuations |
| 2 | The reducer evaluates a process value (`*x`, or a method result evaluated as a process) under the current record with `from_data` set. `enter` and storage keep the flag. With no record, nothing changes. | Mark only values that came from a COMM. | Decision 62: a seal inside data received at run time is refused. Every process value that the reducer evaluates came from a binding, not from the admitted source text. | Yes |
| 3 | Under a `from_data` record, a region seal that names a wallet is refused ("a seal inside data received at run time cannot name a wallet"). This covers a wallet seal and the payer elements of the nested kind form. A `# P` seal is still checked by its own rule. | — | Decision 62: a wallet seal counts only in code that its owner's envelope admitted, or in stored code that carries the owner's record. | Yes |
| 4 | Under a `from_data` record, a stack that would draw on the enclosing record is refused ("a stack inside data received at run time draws on no wallet"). This covers a bare REV stack and a kind transfer with no payer element. | Let a bare stack in data draw on the evaluator. | Otherwise M sends a bare stack instead of a sealed one and gets the same draw. The enclosing seal was not written around the stack by its owner. This applies Decision 62 to the implicit seal. See open item 1. | Yes |
| 5 | A kind mint inside data still mints. Its authority is the unforgeable kind name, which only its holder can write. A kind name received at run time and used in admitted code still moves units (option 1 of the F19 question, for kinds only). | — | Decision 62. | Yes |

## Tests (A-7 F28 plan)

- `rholang/tests/seal_record_spec.rs`:
  - `a_seal_inside_data_received_at_run_time_draws_on_no_wallet`: A stores `for (p <= @"eval") { *p }` and publishes its deployer id. Bob sends `{% slot :: () %}[a]`, a wallet region `{% @"x"!(1) %}[a]`, a bare stack, and a receive whose body is a stack (fired later). Each is refused, and no wallet changes. A's own sealed stack in A's own code still draws on A. With the flag turned off, the test fails at the first case.
  - `a_kind_minted_inside_data_still_mints_and_a_received_kind_name_still_moves_in_code`.
  - The `seal` unit test round-trips `from_data` and checks that `enter` keeps it.
- `casper/tests/v6/region_spec.rs`: `a_seal_sent_as_data_to_an_owners_contract_draws_on_no_wallet`: the F28 attack through the gate (the F19 hole is still open on this branch). Mallory's run fails, no transfer is settled, Alice pays only her own first block, and the block replays to the same settled root.

## Open items

1. Decision 4 extends Decision 62 from explicit seals to the implicit enclosing seal of a stack in data. If the ruling meant only explicit seals, decision 4 is one check in `stack_writer`.
2. Lane B: `rho:rev:wallet` and `rho:rev:bond` should refuse a draw under a `from_data` record by the same rule when they read the record (F5, F6).
3. A `match` on the term's own quoted code that evaluates a branch value also counts as data. No test or contract in the repository does this with a seal.
