# Cost Model

Every metered Rholang reduction consumes source-token phlogiston (gas). Deploys specify a `phlo_limit` and `phlo_price`. The runtime charges token events as it executes. If the limit is exhausted, execution halts with `OutOfPhlogistonsError`.

## Charging Mechanism

```
Total Phlo Charge = phlo_limit * phlo_price
Refund = (phlo_limit - cost_used) * phlo_price
```

The deployer pays for the full limit upfront. Unused phlogiston is refunded after execution. Parser failures occur before a metered source state exists, so they consume zero tokens.

`phlo_limit` and `phlo_price` must be non-negative and their product must
fit in `i64`; deploy admission and block validation reject values that
violate those bounds. The precharge/refund contracts are fee settlement
only: they move balances before and after evaluation, but they do not
mutate the in-flight runtime budget or add fuel back to a running deploy.
Refunds are bounded by the deploy's recorded precharge.

Default limits (from `construct_deploy.rs`):
- Standard deploy: 90,000
- Full deploy: 1,000,000

## Cost Table: Arithmetic

| Operation | Cost | Notes |
|-----------|------|-------|
| Addition (`+`) | 3 | Int, String |
| Subtraction (`-`) | 3 | Int |
| Multiplication (`*`) | 9 | Int |
| Division (`/`) | 9 | Int |
| Modulo (`%`) | 9 | Int |
| Boolean AND | 2 | |
| Boolean OR | 2 | |
| Comparison (`<`, `>`, etc.) | 3 | |

## Cost Table: BigInt (Size-Proportional)

Costs scale with operand byte length. `a_len` and `b_len` are the byte lengths of the two operands.

| Operation | Formula | Minimum |
|-----------|---------|---------|
| Addition | `max(a_len, b_len) + 1` | 3 |
| Subtraction | `max(a_len, b_len) + 1` | 3 |
| Multiplication | `a_len * b_len` | 9 |
| Division | `a_len * b_len` | 9 |
| Modulo | `a_len * b_len` | 9 |
| Negation | `len` | 1 |
| Comparison | `max(a_len, b_len)` | 3 |

This means multiplying two 100-byte BigInts costs 10,000 phlogiston. Gas is the rate limiter for large operands.

## Cost Table: BigRat (Rational)

Costs account for cross-multiplication and GCD normalization. `na`, `da`, `nb`, `db` are byte lengths of numerator/denominator pairs.

| Operation | Formula | Minimum |
|-----------|---------|---------|
| Addition | `4 * max_len^2 + max_len` | 3 |
| Subtraction | `4 * max_len^2 + max_len` | 3 |
| Multiplication | `na*nb + da*db + max_len` | 9 |
| Division | `na*db + da*nb + max_len` | 9 |
| Negation | `num_len` | 1 |
| Comparison | `max(na*db, nb*da)` | 3 |

Where `max_len = max(na, da, nb, db)`.

## Cost Table: Collections

| Operation | Cost |
|-----------|------|
| Map/Set lookup (`get`, `contains`) | 3 |
| Map/Set add | 3 |
| Map/Set remove | 3 |
| Set/Map diff | 3 * num_elements |
| Set/Map union | 3 * num_elements |

## Cost Table: String and Bytes

| Operation | Cost |
|-----------|------|
| String concatenation (`++`) | `len(a) + len(b)` |
| ByteArray append | `log10(len(left))` |
| List append (`++`) | `len(right)` |
| String interpolation (`%%`) | `str_len * map_size` |
| Slice | `to` index value |
| Take | `to` count value |
| hexToBytes | `len(hex_string)` |
| bytesToHex | `len(byte_array)` |
| toList | collection size |
| toByteArray | protobuf encoded size |

## Cost Table: Evaluation

| Operation | Cost |
|-----------|------|
| Method call | 10 |
| Operator call | 10 |
| Variable evaluation | 10 |
| `nth` | 10 |
| `keys` | 10 |
| `length` | 10 |
| `size` | collection size |
| Send evaluation | 11 |
| Receive evaluation | 11 |
| Channel evaluation | 11 |
| `new` binding | 2 per binding + 10 base |
| Match evaluation | 12 |

## Storage and COMM Accounting

RSpace operations are no longer charged by a storage wrapper. The reducer records the
source-level work that leads to sends, receives, substitutions, primitive calls, and
COMM continuation execution. RSpace remains responsible for deterministic matching
and replay logs, while `RuntimeBudget` owns token reservation and exhaustion.

## Source-Token Events

The runtime represents billable work as typed events:

| Event kind | Runtime source |
|------------|----------------|
| `SourceStep` | Structural Rholang reductions such as send, receive, `new`, and `match` |
| `Substitution` | De Bruijn substitution over normalized `Par` terms |
| `Primitive(name)` | Expression operators, method calls, collection operations, and variable lookup |

`MeteredMachine` is the reducer-facing entry point. It builds billable frames,
drains them in canonical `(source_path, redex_id, local_index)` order, then asks
`RuntimeBudget` to commit the ready batch. `RuntimeBudget` returns execution
permits for the funded canonical prefix and records the first canonical OOP
boundary if the batch exceeds the remaining phlo. Permit grant is the cost
commit: the work is charged even if later deploy state rolls back. This keeps
parallel branches out of the per-event budget hot path while preventing unpaid
physical work.

Normal evaluation records only `SourceStep`, `Substitution`, or `Primitive`
events. Legacy `cost.charge(...)` calls are not part of the user-deploy path.

## Equality Cost

```
equality_check_cost(x, y) = min(encoded_len(x), encoded_len(y))
```

Equality is bounded by the smaller operand since the comparison can short-circuit.

## Cost Optimization Tips

1. **Avoid large BigInt multiplication** -- cost is quadratic: `a_len * b_len`
2. **Use `fastUnsafeGet` for TreeHashMap** when you know the key exists -- avoids the existence check
3. **Minimize string interpolation on large maps** -- cost is `str_len * map_size`
4. **Prefer peek (`<<-`) over consume+resend** for read-only access -- avoids extra continuation work
5. **Keep deploys focused** -- unused phlogiston is refunded, but overly broad deploys waste resources
6. **Use smaller phlo limits** when testing -- start with 90,000 and increase as needed

## Phlogiston Errors

When phlogiston is exhausted:
- Execution halts immediately
- All state changes from the current deploy are rolled back
- The `OutOfPhlogistonsError` is recorded in the deploy result
- The deployer is charged for the full `phlo_limit * phlo_price`

To diagnose: check the deploy result's `cost` field to see how much was consumed before the error.
