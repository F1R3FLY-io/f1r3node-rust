---
doc_type: user_flows
version: "1.0"
last_updated: 2026-08-13
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

<!-- Created flows are inserted above the "Planned Flows" section below. -->

---


### FLOW-001: Verify Casper harness evidence

**Status:** In Progress
**Implemented in:** EPIC-017
**Related Stories:** US-006
**Related Flows:** None
**Personas:** release engineer
**Integration Tests:** None

**Journey:** Pin inputs -> Run controls -> Run isolated fixtures -> Audit claims

**Steps:**
1. **Pin inputs** - Record exact source, executable, fixture, and configuration identities.
2. **Run controls** - Check the clean model and each named negative control.
3. **Run isolated fixtures** - Exercise the real driver with controlled processes and retain all outcomes.
4. **Audit claims** - Check claim identities, source digests, phase evidence, and remaining gaps.

**Key Interactions:**
- A changed manifest cannot resume an existing run.
- A terminal transition cannot admit another workload.
- Incomplete evidence cannot produce a passing soak verdict.

**Success Metrics:**
- Every registered invocation has a matching exit record.

---

## Planned Flows

No planned flows are defined.

---

## Related Documentation

- [User Stories](UserStories.md)
- [ToDos](ToDos.md)
