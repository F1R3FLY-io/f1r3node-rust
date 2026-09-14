# D2 ownership review: memory protection and launch paths

## Purpose and scope

This review supports the D2 completion boundary.
It records who owns the memory-protection metadata and every workload launch path.
It confirms that each launch path has an owner marker and a stop path.
It names the gaps that the remaining D2 and D3 work must close.

This review reads the committed driver at the current branch head.
It does not change the driver, the launcher, or any gate.
It marks a claim as confirmed only when the driver code shows it.
It marks a claim as open when the behavior needs a run or a launcher review.

## Memory-protection metadata

The run identity is a per-run owner value.
The driver reads a fresh universally unique identifier into `SOAK_WRITER_OWNER` at startup.
It applies that value to the owned writers and uses it to select them for a stop.
The guardian stamps disk and memory health tags as durable last words.

| Metadata | Owner | How it is applied |
| --- | --- | --- |
| `SOAK_WRITER_OWNER`, the run identity | The driver, one value per run | The Docker label `io.f1r3fly.soak.owner` and the process marker |
| `SOAK_PROCESS_OWNER`, the process marker | The driver | Written into the workload process environment for stop selection |
| Out-of-memory preference | The driver's Docker shim | `--oom-score-adj 1000` on Docker run and create, and the same value injected into Compose service configuration |
| Health tags | The guardian | `guardian_stamp_health_tag` writes disk and memory summaries |

The out-of-memory preference makes the workload the preferred kernel victim, so the runner process survives.
The driver applies this preference through a Docker shim on the workload path.
The shim covers the Docker provider and the Compose provider.
It does not cover a process that the shim does not launch.

## Workload launch paths

The workload starts through one of four paths.
Each path needs an owner marker, a stop path, and an out-of-memory preference.
The table records the current state of each.

| Launch path | Launcher | Owner marker | Stop path | Out-of-memory preference |
| --- | --- | --- | --- | --- |
| Subprocess provider | `poetry run pytest` with the subprocess provider | `SOAK_PROCESS_OWNER` marker | `stop_node_writers` by owner marker | Open: the shim does not set the node subprocess score |
| Docker provider | The harness through the Docker shim | The `io.f1r3fly.soak.owner` label | `stop_node_writers` by label | Confirmed: `--oom-score-adj 1000` |
| Compose provider | The harness through the Docker shim | The label injected into every service | `stop_node_writers` by label | Confirmed: the injected `oom_score_adj` |
| Native service manager | The native containment launcher | The run-domain record and control directory | The native controller stop | Open: the native launcher review is pending |

## Findings

The Docker and Compose providers are fully owned and preferred.
The shim labels every container and sets the preferred out-of-memory score, so the runner survives a kill.
The stop path selects these containers by the owner label.
These two paths need no further ownership work.

The subprocess provider has an owner marker and a stop path, but its out-of-memory preference is open.
The shim applies the preference only to the processes it launches, and the subprocess node processes do not pass through the shim.
The incident record notes that the kernel killed the pytest process at score 500, not the workload at the preferred score.
A remaining cycle must confirm that every subprocess node process carries the preferred score, or must add it.

The native service manager has an owner marker and a stop path through the run-domain record.
Its out-of-memory preference for native processes is the launcher's responsibility and is not verified here.
The native launcher review belongs to the native-admission session.

## What remains

The two open items are the subprocess out-of-memory preference and the native launcher preference.
Both need a driver or launcher change, so they wait for the admission-check cleanup to settle.
The subprocess item connects to the incident, so it has priority once the driver is free.
D2, D3, and claim discharge remain pending.
