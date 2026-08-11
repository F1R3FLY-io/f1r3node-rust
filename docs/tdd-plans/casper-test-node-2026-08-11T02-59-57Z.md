---
kind: tdd-plan
scope: casper-test-node
produced_by: /tdd
produced_at: 2026-08-11T03:18:01Z
source_discovery: docs/discoveries/architecture-review-2026-08-11T02-59-57Z.md
source_candidate: C1
parent_task: TASK-012-1
accepted_design: common-caller
glossary: docs/Glossary.md
test_runner: cargo-test
system_boundaries:
  - time
  - randomness
  - filesystem
conformance_audit:
  status: pass
  notes:
    - "Behavior statements exercise the test node interface and avoid internal call counts or collaborator mocks."
    - "Local storage, runtime, rejected-deploy-buffer, consensus, and in-memory-transport adapters remain inside the system under test."
    - "C1 acceptance is recorded in docs/ToDos.md because the gitignored discovery predates the acceptance decision."
behaviors:
  - id: B1
    statement: "A standalone test node can perform block proposal and block validation for a valid deploy."
    priority: must
    deep_module: true
    done: false
    cycle_log: []
  - id: B2
    statement: "A default test network creates the requested number of participating nodes and lets peers validate a published block."
    priority: must
    deep_module: true
    done: false
    cycle_log: []
  - id: B3
    statement: "Empty-block configuration determines whether a test node can advance without user deploys."
    priority: must
    deep_module: false
    done: false
    cycle_log: []
  - id: B8
    statement: "Targeted propagation makes a block known to the selected peer without implicitly updating unrelated peers."
    priority: must
    deep_module: true
    done: false
    cycle_log: []
  - id: B9
    statement: "Synchronization brings a lagging test node to the block knowledge shared by its peers."
    priority: must
    deep_module: true
    done: false
    cycle_log: []
  - id: B10
    statement: "Focused inspection through the test node interface exposes observable consensus, runtime, and storage outcomes without consensus-implementation field construction."
    priority: must
    deep_module: true
    done: false
    cycle_log: []
  - id: B4
    statement: "A selected bootstrap node anchors network startup regardless of its position in the node collection."
    priority: should
    deep_module: false
    done: false
    cycle_log: []
  - id: B5
    statement: "Read-only nodes can observe and synchronize but cannot perform block proposal."
    priority: should
    deep_module: false
    done: false
    cycle_log: []
  - id: B6
    statement: "Parent-limit configuration constrains the parents selected during block proposal."
    priority: should
    deep_module: false
    done: false
    cycle_log: []
  - id: B7
    statement: "Synchrony configuration changes proposal eligibility without changing network construction."
    priority: consider
    deep_module: false
    done: false
    cycle_log: []
---

# TDD Plan -- `casper-test-node`

This plan deepens the [`Test node`](../Glossary.md#test-node) interface accepted as candidate C1 in the [architecture review](../discoveries/architecture-review-2026-08-11T02-59-57Z.md). It exercises standalone and configured-network entry points, network scenario operations, and focused inspection while preserving observable [`Block proposal`](../Glossary.md#block-proposal) and [`Block validation`](../Glossary.md#block-validation) behavior. Each invocation completes one vertical RED-GREEN cycle through the public interface.

## Public Interface

- **Name:** [`Test node`](../Glossary.md#test-node), including its common-caller test-network interface
- **Signature surface:** `TestNode::standalone(genesis)`, `TestNetwork::new(genesis, size)`, `TestNetwork::with_config(genesis, config)`, network proposal/publication/propagation/synchronization operations, and focused test-node inspection accessors
- **Invariants:** one canonical fixture behavior; deterministic local network construction; explicit configuration for behavioral variation; no caller construction of consensus implementation fields; preserved block-proposal and block-validation outcomes
- **Ordering constraints:** construction precedes scenario operations; publication or propagation precedes peer observation; synchronization applies peer knowledge before convergence assertions
- **Error modes:** invalid configuration, read-only proposal, failed proposal, failed validation, unavailable peer, and local storage/runtime errors remain typed `Result` failures or existing typed proposal outcomes
- **Required configuration:** genesis context and node count; optional parent limits, synchrony settings, read-only count, bootstrap index, and empty-block behavior
- **Performance characteristics:** tests use local adapters with deterministic bounded scenarios; no remote network is required

Changing this interface requires re-running TDD planning because every behavior is pinned to the accepted common-caller design.

## System Boundaries (Mocking Allowed)

Mocks may be placed at and only at these system seams:

- **Time** -- deterministic control for expiration or heartbeat behavior
- **Randomness** -- deterministic key or identifier generation
- **Filesystem** -- explicit filesystem-failure behavior only; normal cycles use real temporary storage

Casper consensus modules, the Rholang runtime, block/DAG/deploy storage, the [`Rejected deploy buffer`](../Glossary.md#rejected-deploy-buffer), and in-memory transport are controlled local dependencies. They use real local adapters behind internal seams and are not mocked.

## Behavior Checklist

Each behavior is a capability statement. Must behaviors run before should and consider behaviors.

- [ ] **B1** -- A standalone [`Test node`](../Glossary.md#test-node) can perform [`Block proposal`](../Glossary.md#block-proposal) and [`Block validation`](../Glossary.md#block-validation) for a valid deploy. -- priority: `must` -- deep_module: `true`
- [ ] **B2** -- A default test network creates the requested number of participating nodes and lets peers validate a published block. -- priority: `must` -- deep_module: `true`
- [ ] **B3** -- Empty-block configuration determines whether a [`Test node`](../Glossary.md#test-node) can advance without user deploys. -- priority: `must` -- deep_module: `false`
- [ ] **B8** -- Targeted propagation makes a block known to the selected peer without implicitly updating unrelated peers. -- priority: `must` -- deep_module: `true`
- [ ] **B9** -- Synchronization brings a lagging [`Test node`](../Glossary.md#test-node) to the block knowledge shared by its peers. -- priority: `must` -- deep_module: `true`
- [ ] **B10** -- Focused inspection through the [`Test node`](../Glossary.md#test-node) interface exposes observable consensus, runtime, and storage outcomes without consensus-implementation field construction. -- priority: `must` -- deep_module: `true`
- [ ] **B4** -- A selected bootstrap node anchors network startup regardless of its position in the node collection. -- priority: `should` -- deep_module: `false`
- [ ] **B5** -- Read-only nodes can observe and synchronize but cannot perform [`Block proposal`](../Glossary.md#block-proposal). -- priority: `should` -- deep_module: `false`
- [ ] **B6** -- Parent-limit configuration constrains the parents selected during [`Block proposal`](../Glossary.md#block-proposal). -- priority: `should` -- deep_module: `false`
- [ ] **B7** -- Synchrony configuration changes proposal eligibility without changing network construction. -- priority: `consider` -- deep_module: `false`

## Cycle Log

No cycles have run. The first `/tdd` invocation against this plan selects B1 as the tracer bullet and writes exactly one new behavior test before the minimal GREEN implementation.

## Glossary Anchors Used

- [`Test node`](../Glossary.md#test-node)
- [`Block proposal`](../Glossary.md#block-proposal)
- [`Block validation`](../Glossary.md#block-validation)
- [`Rejected deploy buffer`](../Glossary.md#rejected-deploy-buffer)

## Completion

The plan is complete when all ten behaviors are checked and every cycle has a non-empty GREEN record. At completion:

- `/tdd --status` reports the plan complete.
- `/loop /tdd` stops naturally.
- `/task-complete TASK-012-1 --unit-tests B1,B2,B3,B4,B5,B6,B7,B8,B9,B10` closes the parent task after verification.

## Reopening

The plan may be reopened when a completed cycle surfaces a new user-observable behavior and the user ratifies it. Internal construction or collaborator changes do not reopen the behavior checklist unless they alter the accepted test-node interface.
