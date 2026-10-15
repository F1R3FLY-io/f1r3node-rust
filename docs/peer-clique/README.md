# RGB Peer Clique Documentation

This directory holds the documentation of the RGB Peer Clique replication medium. The
tree travels with the medium if the medium moves to its own repository. The
f1r3node-rust platform itself stays neutral about the replication medium of a
shard.

## Status

Peer-clique consensus is specified in the Rholang-RGB repository (SoW2 workstream WS5). It is not implemented in this repository. Bitcoin and Lightning seal media are at the `alpha` tier in the RGB tree.

## Documentation Map

| Area | Documents |
|---|---|
| Vocabulary | [RGB Peer Clique Glossary](./GLOSSARY.md) |
| Boundary | [Replication boundary design](../designs/replication-boundary.md) (platform-owned) |
| Architecture | [Parallel state machines and consensus-neutral execution](../artifacts/f1r3fly-consensus-neutral-sm.md) |
| Sources | Rholang-RGB `docs/sow-2.md`, `docs/design/architecture.md`, and `docs/design/peer-clique-consensus.md` |
