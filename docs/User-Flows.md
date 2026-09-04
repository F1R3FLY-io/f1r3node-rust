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

## Planned Flows

No planned flows are defined.

---

## Related Documentation

- [User Stories](UserStories.md)
- [ToDos](ToDos.md)
