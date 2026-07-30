#!/usr/bin/env bash
# Structural, fail-closed checks on the CI credential model.
#
# These are the invariants that comments cannot protect. `_integration-pipeline.yml`
# says "SECURITY: do not remove" above the `needs: await_approval` that gates
# untrusted fork code, but a comment does not stop a delete — this does.
#
# Wired into ci.yml's `lint` job rather than `build_base` on purpose: `Lint` is a
# required status check on both rulesets, so a violation blocks the merge. A job
# that only fails upstream can surface as `skipped`, which required-check
# evaluation may treat as satisfied.
#
# Run from the repository root. Every check prints its verdict; the script exits
# non-zero if any failed, listing all failures rather than stopping at the first.
set -euo pipefail

PIPELINE=.github/workflows/_integration-pipeline.yml
fail=0

err() {
    printf '::error::%s\n' "$1"
    fail=1
}

ok() {
    printf 'ok: %s\n' "$1"
}

# Emit one line of facts per job in a workflow file:
#
#   <job> <has_environment> <reads_oci> <reads_app_key> <gated> <calls_pipeline> <inherits>
#
# Job headers are the two-space-indented keys under `jobs:`. Comment lines are
# skipped first and deliberately: several jobs discuss `await_approval` in prose,
# and counting those would let the guard pass on a workflow whose real `needs:`
# had been deleted.
scan_jobs() {
    awk '
        /^[[:space:]]*#/ { next }
        /^  [A-Za-z_][A-Za-z0-9_-]*:[[:space:]]*$/ {
            job = $1
            sub(/:$/, "", job)
            order[++count] = job
            next
        }
        job == "" { next }
        /^    environment:/                                                  { has_env[job] = 1 }
        /secrets\.OCI_[A-Z_]+/                                               { oci[job] = 1 }
        # Any App private key, not one specific name. There are two Apps — a
        # release identity and a runner-admin identity — and more may follow, so
        # matching a literal name would let a renamed or newly added App key slip
        # past every check below while they all still reported ok.
        /secrets\.[A-Z_]*APP_PRIVATE_KEY/                                    { app[job] = 1 }
        /await_approval/                                                     { gated[job] = 1 }
        /^    uses:[[:space:]]*\.\/\.github\/workflows\/_integration-pipeline\.yml/ { calls[job] = 1 }
        /^    secrets:[[:space:]]*inherit/                                   { inherits[job] = 1 }
        END {
            for (i = 1; i <= count; i++) {
                j = order[i]
                printf "%s %d %d %d %d %d %d\n", j, \
                    has_env[j] + 0, oci[j] + 0, app[j] + 0, \
                    gated[j] + 0, calls[j] + 0, inherits[j] + 0
            }
        }
    ' "$1"
}

# 1. Approval gate. Both jobs that spend real resources on fork-authored code
#    must depend on await_approval: the launch job creates OCI VMs, and the
#    Docker build compiles untrusted code on the permanent self-hosted runners.
#    Removing either `needs:` is the abuse path the ephemeral-launch approval
#    exists to close.
for job in launch_ephemeral_runners build_docker_image; do
    if scan_jobs "$PIPELINE" | awk -v j="$job" '$1 == j && $5 == 1 { found = 1 } END { exit !found }'; then
        ok "$job gates on await_approval"
    else
        err "$job in $PIPELINE no longer depends on await_approval; unapproved fork PRs could spend CI resources"
    fi
done

# 2. Credential concentration. Exactly one job in the reusable pipeline may hold
#    credentials, and it must be the launch job — the only one that checks out
#    solely the pinned system-integration SHA and never the code under test.
cred_jobs="$(scan_jobs "$PIPELINE" | awk '$3 == 1 || $4 == 1 { print $1 }')"
cred_count="$(printf '%s' "$cred_jobs" | grep -c . || true)"
if [ "$cred_count" = "1" ] && [ "$cred_jobs" = "launch_ephemeral_runners" ]; then
    ok "credentials confined to launch_ephemeral_runners"
else
    # Flattened to one line: a GitHub error annotation captures only its first
    # line, so a multi-line job list would hide every name after the first.
    cred_list="$(printf '%s' "$cred_jobs" | tr '\n' ' ')"
    err "expected exactly one credential-bearing job (launch_ephemeral_runners) in $PIPELINE, found: ${cred_list:-none}"
fi

# 3. Environment scoping. Any job anywhere that reads OCI or GitHub App
#    credentials must name an environment, so access is a property of the
#    environment rather than of the repository secret store.
for workflow in .github/workflows/*.yml; do
    while read -r job has_env reads_oci reads_app _gated _calls _inherits; do
        [ -n "${job:-}" ] || continue
        if [ "$reads_oci$reads_app" != "00" ] && [ "$has_env" = "0" ]; then
            err "$workflow job '$job' reads privileged credentials without naming an environment"
        fi
    done <<EOF
$(scan_jobs "$workflow")
EOF
done
ok "every credential-reading job names an environment"

# 4. Secret delivery. A called workflow's jobs do not receive secrets.* merely by
#    naming an environment — the caller must pass them down. Dropping this made
#    the launch job's OCI_* expressions resolve empty and the OCI CLI reject the
#    config as malformed (runs 30495007422 and 30498890544). Guarded because the
#    line reads like redundant over-sharing and invites deletion.
for caller in .github/workflows/ci.yml .github/workflows/ci-fork-pr.yml; do
    while read -r job _has_env _reads_oci _reads_app _gated calls inherits; do
        [ -n "${job:-}" ] || continue
        if [ "$calls" = "1" ] && [ "$inherits" = "0" ]; then
            err "$caller job '$job' calls the integration pipeline without 'secrets: inherit'; the launch job would see empty credentials"
        fi
    done <<EOF
$(scan_jobs "$caller")
EOF
done
ok "both pipeline callers pass secrets down"

if [ "$fail" -ne 0 ]; then
    printf '::error::%s\n' "workflow security invariants violated; see errors above"
    exit 1
fi

echo "All workflow security invariants hold."
