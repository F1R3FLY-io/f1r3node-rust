# Host Memory Preference Ownership

B34 checks ownership of native-process out-of-memory (OOM) preferences.
The fixture starts two unrelated writers before it starts the driver.
The workload then starts its detached writer.

The baseline changes the workload node and unrelated node from preference zero to 1000.
The unrelated client remains at zero.
The corrected driver changes only the workload preference.
Both versions stop the workload writer and preserve the unrelated writers after exit.

The helper selects same-user processes with the exact inherited owner value.
It opens each process directory before it reads the environment or writes the preference.
Both operations use that directory descriptor instead of reopening a numeric process path.
The helper does not use a privileged fallback for native-process writes.

The model has one atomic preference update and two distinct states.
The exact control violates `UnownedPreferencesPreserved` with exit 12.
The positive configuration also requires `OwnedPreferenceApplied`.

The model assumes stable cooperative ownership and a successful selected write.
It does not prove every process turnover race, permission outcome, or scheduling bound.
The fixture tests real preference writes without actual memory exhaustion.
No fixture can access developer-host processes.

The Docker preference loop remains outside this correction.
Its name-based selection remains an open B36 requirement.
These preference values do not guarantee that the kernel will spare the runner.
D2, D3 bounds, durable evidence, hosted checks, and maintainer review remain pending.

See the [B34 evidence](../../../docs/cbc-evidence/soak-d2-oom-ownership-2026-09-11/README.md).
