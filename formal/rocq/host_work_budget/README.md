# Host-work budget Rocq proofs

This directory contains the unbounded proof layer for host-work limits.
The adjacent [TLA+ package](../../tlaplus/host_work_budget/README.md) checks concurrent finite executions.
The permanent [host-work specification](../../../docs/casper/theory/host-work-budget.md) maps these proofs to production play and replay.

[`HostWorkBudget.v`](theories/HostWorkBudget.v) defines checked reservation arithmetic.
It proves successful bounds, overflow rejection, failure non-mutation, and economic separation.

[`HostWorkExecution.v`](theories/HostWorkExecution.v) defines eight phases and sixteen independent dimensions.
It does not combine dimensions through weighted arithmetic.

The execution proof uses a generic dimension type.
It proves dimension commutation, cumulative replay agreement, transactional rollback, rejected-verdict schedule independence, and independent-shard commutation.

Run the complete focused gate from the repository root:

```bash
scripts/check-host-work-budget-formal.sh
```
