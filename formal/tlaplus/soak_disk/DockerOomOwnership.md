# Docker Memory Preference Ownership

B36 checks ownership of container out-of-memory (OOM) preferences.
The fixture starts an unrelated writer before the driver starts its workload writer.
Both writers use the same nonprivileged container restrictions.
The fixture records actual healthy memory samples without causing memory exhaustion.

The baseline periodically selects Docker processes by container name.
It changes the unrelated process preference from zero to 1000.
The correction removes that periodic Docker mutation.
It supplies preference 1000 when the workload wrapper creates containers with host-memory protection enabled.
Docker `run` receives `--oom-score-adj 1000`.
Compose `up` receives a service override with `oom_score_adj: 1000`.

The matched `run` and Compose cases preserve the unrelated preference at zero after correction.
Both cases still set the workload preference to 1000.
Driver exit stops the workload writer and preserves the unrelated writer and its file growth.
The separate default-configuration cases leave Docker creation preference at zero when host-memory protection is disabled.

## Model correspondence

`Launch` represents successful workload creation with a stable cooperative owner.
`Observe` represents a subsequent guardian sample.
The baseline control adds both writers to the preference set during `Observe`.
The corrected model selects only the workload during `Launch` and preserves that selection during `Observe`.

The exact control violates `UnownedContainerPreferencesPreserved` with exit 12.
The positive configuration also checks `OwnedContainerPreferenceApplied`, `TypeOK`, and `Completes`.
The positive model has three distinct states.
The original production and formal counterexamples preceded the correction.
The final batch repeats both production counterexamples against the retained baseline bytes.

## Contract limits

The wrapper also handles Docker `create` and Compose `create` in the same branches.
These creation variants did not receive separate real-Docker tests in B36.
The tested callers do not supply conflicting preference options.
For `run`, a later caller option can override the injected preference.
The result therefore does not establish preference enforcement against conflicting caller configuration.

Absolute Docker paths, direct API calls, unsupported global options, existing-container adoption, and concurrent creation remain outside the tested contract.
Owner labels and inherited values express cooperative creation, not authorization against forged metadata or Docker administration.
The model assumes successful creation and atomic transitions.
It does not model every daemon failure, process race, restart, or scheduler delay.

A preference value does not guarantee that the kernel will spare the runner.
Legacy guardian comments that suggest this guarantee are not verified claims.
This result does not establish a complete shutdown deadline, durable evidence publication, or sufficient disk reserve.
D2 and acceptance remain pending.

See the [B35–B36 evidence](../../../docs/cbc-evidence/soak-d2-container-preference-2026-09-11/README.md).
