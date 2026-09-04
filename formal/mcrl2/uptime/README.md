# Uptime concurrency interface

The mCRL2 model checks the finite service-action interface around the existing
Casper and cost-accounting authorities. It does not reproduce quorum
arithmetic, block validity, replay roots, or purse settlement.

`concurrent_service.mcrl2` permits independent receive, capture, replay,
validation, publication, commit, release, and retry cycles for two shards.
The modal formulas establish deadlock freedom and reachable overlap in both
orders for replay and validation. `global_mutex_unsafe.mcrl2` is the required
negative control: its validation-overlap property is false because it
serializes both workers through one global lifecycle.

Run `scripts/check-uptime-mcrl2.sh`. Generated linear-process, transition, and
Boolean-equation artifacts remain under `target/verification/uptime/mcrl2/`.
