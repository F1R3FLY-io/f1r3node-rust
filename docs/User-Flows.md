---
doc_type: user_flows
version: "1.0"
last_updated: 2026-09-30
---

# User Flows

This file describes user interactions. Each flow records ordered steps, test assertions, and success metrics.
Each flow links to its user stories and implementation epic.

**Document Structure**

- User flows: `docs/User-Flows.md`
- User stories: `docs/UserStories.md`
- Implementation tracking: `docs/ToDos.md`

**Linkage**

- Each flow lists its related stories and implementation epic.
- Each related story includes a `User Flow:` back-reference.
- The related epic includes a `user_flow:` field.
- Add `Integration Tests:` after implementation and unit tests are complete.
- Use `test-spec FLOW-XXX` to create the test artifact.
- Use `link FLOW-XXX <ITEST-NNN|path>` to add more tests.

---

## Document Relationships

- User flows: `docs/User-Flows.md`
- User stories: `docs/UserStories.md`
- Implementation epics and tasks: `docs/ToDos.md`

A flow lists its related stories and implementation epic. Each related story contains a `User Flow:` back-reference.
The related epic contains a `user_flow:` field. Add integration-test references after implementation and unit tests are complete.

## Personas

Add reusable persona definitions here. Give each persona a name and a short description.
Use the persona name in each flow's `Personas:` field.

---

## Core Workflows

### FLOW-002: Operate a node through resource faults

**Status:** In Progress
**Implemented in:** EPIC-020
**Related Stories:** US-009
**Related Flows:**
**Personas:** Node operator
**Integration Tests:**

**Journey:** Configure limits -> Observe a resource fault -> Inspect bounded errors -> Confirm recovery -> Verify storage limits

**Steps:**
1. **Configure limits** - Configure the node log budget, one deployment sink, and container log caps before deployment.
2. **Observe a resource fault** - Detect descriptor exhaustion or accept failures during node operation.
3. **Inspect bounded errors** - Inspect error delivery, retry spacing, limited error logs, and suppressed-count summaries.
4. **Confirm recovery** - Release exhausted resources and confirm that the listener accepts a new connection without persistent error backoff.
5. **Verify storage limits** - Check file retention and container log budgets during sustained load.

**Key Interactions:**
- The listener preserves accept errors for its consumer while limiting retry frequency.
- A successful accept resets the error backoff without delaying the next ordinary accept.
- Consumer closure stops the listener and cancels unfinished handshakes.
- File retention and container log caps prevent unbounded deployment log storage.
- The soak guardian stops work when a node or container log exceeds its budget.

**Success Metrics:**
- Error retry delay doubles from 10 ms to a maximum of one second, subject to the documented timer rounding.
- The listener emits at most one accept-error ERROR line per second and a suppressed-count summary after each minute with continuing errors.
- The controlled Linux fault test observes one to six accept errors and less than 1,024 log bytes during 500 ms.
- After descriptor release, the controlled test reaches the new connection's handshake timeout within two seconds without another accept error.
- TASK-020-2 must establish file and directory byte limits.
- TASK-020-3 must verify one sink and container log caps across the required deployments.
- TASK-020-4 must verify the soak guardian's log-growth budgets.

**Verification Boundary:**

This flow describes required operator behavior, not a completed live deployment exercise.
TASK-020-1 has [source-bound hosted transport evidence](work-logs/evidence/task-020-1-hosted-20260930-01/report.json).

The regression file is `comm/src/rust/transport/f1r3fly_server_resource_tests.rs`.
The controlled recovery test reaches handshake timeout. It does not establish successful authenticated peer communication.

TASK-020-2 has [local file-budget verification](work-logs/task-020-2-byte-bounded-logging-20260930.md).
The file sink defaults to 100 MiB per file and 2 GiB across its log directory.
Container caps and guardian enforcement remain pending under TASK-020-3 and TASK-020-4.
Integration-test references remain empty until actual deployment tests exist. No generated specification is treated as executed evidence.

---

## Planned Flows

No planned flows are defined.

---

## Related Documentation

- [User Stories](UserStories.md)
- [ToDos](ToDos.md)
