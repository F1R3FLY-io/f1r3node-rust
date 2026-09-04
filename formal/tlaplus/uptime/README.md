# Uptime envelope dominance

`UptimeEnvelopeDominance.tla` checks the ordering required to call the Storm
parameter corners worst and best. Two copies of the shard reliability state
run concurrently. Shared events affect both; the adverse copy additionally
receives failure, pressure, lag-growth, and resident-memory-growth events; the
favorable copy additionally receives repair, relief, lag-drain, and
resident-memory-reclamation events.

The checked invariants require the adverse copy to have no more eligible
validators, no less queue or lag, no healthier common-cause state, and no more
available storage, and no less resident-memory pressure. Consequently, service
in the adverse copy implies service in the favorable copy for every explored
interleaving. Exponential clocks in
the declared CTMC interval can be coupled as a shared minimum rate plus the
appropriate residual clock, so this transition order supplies the stochastic
endpoint order.

| Configuration | Required result |
| --- | --- |
| `MC_UptimeEnvelopeDominance.cfg` | `TypeOK`, `Dominance`, and `ServiceOrder` hold |
| `MC_UptimeEnvelopeDominance_unsafe.cfg` | favorable-only validator failure violates `Dominance` |

Run `scripts/check-uptime-tla.sh` directly or use
`scripts/check-uptime-ALL.sh`. Output belongs below
`target/verification/uptime/tla/`.
