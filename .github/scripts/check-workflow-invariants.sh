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

# 5. Fast checks use ref-and-SHA push groups. Heavy branch work keeps only the
#    current head, while each version tag has an independent release group.
#    A branch publisher must also reject a stale SHA after it gets its lock.
ci_concurrency_errors=""
if ! ci_concurrency_errors="$(ruby -ryaml - .github/workflows/ci.yml 2>&1 <<'RUBY'
def environment_name(job)
  value = job["environment"]
  value.is_a?(Hash) ? value["name"] : value
end

def normalized(value)
  value.to_s.gsub(/\s+/, " ").strip
end

doc = YAML.load_file(ARGV[0])
jobs = doc.fetch("jobs")
expected_group = "${{ github.workflow }}-${{ github.event_name == 'push' && format('{0}-{1}', github.ref, github.sha) || github.ref }}"
expected_cancel = "${{ github.event_name == 'pull_request' }}"
concurrency = doc.fetch("concurrency", {})
puts "workflow concurrency must separate branch and tag pushes at the same SHA" unless normalized(concurrency["group"]) == normalized(expected_group)
puts "workflow concurrency must cancel only superseded PR runs" unless normalized(concurrency["cancel-in-progress"]) == normalized(expected_cancel)

pipeline = jobs.fetch("pipeline", {})
pipeline_concurrency = pipeline.fetch("concurrency", {})
expected_pipeline_group = "ci-heavy-${{ github.event_name == 'pull_request' && format('pr-{0}', github.event.pull_request.number) || github.ref }}"
expected_pipeline_cancel = "${{ !startsWith(github.ref, 'refs/tags/') }}"
puts "heavy pipeline must use a PR-or-ref queue" unless normalized(pipeline_concurrency["group"]) == normalized(expected_pipeline_group)
puts "heavy branch and PR work must replace obsolete heads without cancelling version tags" unless normalized(pipeline_concurrency["cancel-in-progress"]) == normalized(expected_pipeline_cancel)

publisher = jobs.fetch("release_docker_image", {})
publisher_concurrency = publisher.fetch("concurrency", {})
puts "image publication must use a ref-specific queue" unless normalized(publisher_concurrency["group"]) == normalized("ci-image-publish-${{ github.ref }}")
puts "image publication queue must not cancel in-progress work" unless publisher_concurrency["cancel-in-progress"] == false
expected_environment = "${{ (github.ref == 'refs/heads/dev' || github.ref == 'refs/heads/master') && 'protected-branch-image-publish' || 'ephemeral-launch' }}"
puts "only dev and master can bypass reviewer approval for image publication" unless normalized(environment_name(publisher)) == normalized(expected_environment)
puts "image publication must remain gated on the unit-test matrix" unless Array(publisher["needs"]).include?("test")

steps = Array(publisher["steps"])
gate_index = steps.index { |step| step.is_a?(Hash) && step["id"] == "publish_gate" }
if gate_index.nil?
  puts "image publication must check the current branch tip"
else
  puts "branch-tip gate must run before checkout" unless gate_index == 0
  gate_body = steps[gate_index]["run"].to_s
  gate_patterns = [
    /^\s*branch_tip_status\(\)/,
    /^\s*publish_mutable\(\)/,
    /^\s*for attempt in 1 2 3;/,
    /^\s*publish=false\s*$/,
    /repos\/\$\{GITHUB_REPOSITORY\}\/git\/ref\/heads/,
    /GITHUB_OUTPUT/
  ]
  puts "branch-tip publication gate is incomplete" unless gate_patterns.all? { |pattern| gate_body.match?(pattern) }

  publish_condition = "steps.publish_gate.outputs.publish == 'true'"
  guarded_steps = steps[(gate_index + 1)..-1].to_a.reject { |step| step["name"] == "Report image publication result" }
  puts "all image publication steps must use the branch-tip gate" unless guarded_steps.all? { |step| normalized(step["if"]) == normalized(publish_condition) }

  report = steps.find { |step| step["name"] == "Report image publication result" }
  puts "image publication must report published or stale status" unless report.is_a?(Hash) && normalized(report["if"]) == "always()"

  %w[Publish\ Docker\ Image Publish\ to\ OCIR].each do |name|
    step = steps.find { |candidate| candidate["name"] == name }
    body = step && step["run"].to_s
    puts "#{name} must source the branch-tip guard" unless body&.match?(/^\s*source "\$RUNNER_TEMP\/branch-tip-guard\.sh"/)
    puts "#{name} must recheck every mutable remote update" unless body&.scan(/^\s*publish_mutable /)&.length.to_i >= 3
    puts "#{name} must authenticate branch-tip checks" unless step&.dig("env", "GH_TOKEN")
  end
end

packages = jobs.fetch("release_packages", {})
packages_concurrency = packages.fetch("concurrency", {})
puts "package publication must use a ref-specific queue" unless normalized(packages_concurrency["group"]) == normalized("ci-package-publish-${{ github.ref }}")
puts "package publication queue must not cancel in-progress work" unless packages_concurrency["cancel-in-progress"] == false
puts "package publication must remain gated on the unit-test matrix" unless Array(packages["needs"]).include?("test")

reviewer_gated_jobs = jobs.each_with_object([]) do |(job_id, job), found|
  found << job_id if job.is_a?(Hash) && environment_name(job) == "ephemeral-launch"
end
puts "CI jobs use an unconditional reviewer-gated environment: #{reviewer_gated_jobs.join(', ')}" unless reviewer_gated_jobs.empty?
RUBY
)"; then
	err "CI concurrency invariant checker failed: $(printf '%s' "$ci_concurrency_errors" | tr '\n' ';')"
	ci_concurrency_errors=""
elif [ -n "$ci_concurrency_errors" ]; then
	err "CI concurrency invariants failed: $(printf '%s' "$ci_concurrency_errors" | tr '\n' ';')"
else
	ok "each push SHA runs tests independently and release side effects use ref-scoped controls"
fi

# 6. The CI runner compartment OCID is pinned identically wherever it appears.
#    It is hardcoded rather than held in an Actions variable on purpose: the
#    reaper's own comment claims it "can never touch other compartments", and a
#    variable is mutable by anyone with repo admin, so moving it there would
#    trade a compile-time guarantee for a permissions one. The cost of pinning
#    is drift — ci-runner-reaper.yml decides what may be terminated, while
#    merge-recovery-soak.yml tags the instance that must not be, and if those
#    two ever name different compartments the tag is written where the reaper
#    never looks. The soak then dies at the 2h mark with its exemption intact
#    but invisible, which is indistinguishable from the bug we just fixed.
#    Checking equality keeps the value immutable in-repo and makes divergence
#    fail CI instead of failing a 60h weekend soak.
#    `|| true` on both greps is load-bearing: under `set -e` a no-match grep
#    inside a command substitution kills the script outright, so the "nobody
#    pins it any more" branch below would be unreachable and the failure would
#    surface as a red job with no annotation saying why. A guard that cannot
#    explain itself is the failure mode this file exists to prevent.
#    Both files are checked BY NAME and required to pin exactly one literal
#    each. An earlier version only asserted "at least one file pins it, and
#    all literals found agree", which passed when one of the two declarations
#    was deleted or rewritten as `${{ vars.X }}` — the single most likely way
#    this drifts, since that is precisely the migration the note above argues
#    against. My own mutation tests missed it: they covered divergent values
#    and both-removed, never one-removed.
#    Only the reaper is REQUIRED to pin it. merge-recovery-soak.yml used to
#    carry a copy because the launch job looked its instance up by compartment
#    + display-name; that step is gone, because it tagged whichever VM the
#    launch created rather than the one the job actually ran on (run
#    30590630059). The soak now tags itself by the OCID the metadata service
#    reports, so it needs no compartment at all. Any file that still pins one
#    must agree with the reaper — checked below — but absence is no longer a
#    violation for the soak.
ocid_required=".github/workflows/ci-runner-reaper.yml"
ocid_optional=".github/workflows/merge-recovery-soak.yml"
ocid_values=""
for ocid_file in $ocid_optional; do
	ocid_found="$(grep -hoE 'CI_RUNNER_COMPARTMENT_OCID:[[:space:]]*"ocid1\.compartment\.[A-Za-z0-9._-]+"' \
		"$ocid_file" 2>/dev/null |
		sed -E 's/.*"(ocid1\.compartment\.[A-Za-z0-9._-]+)"/\1/' || true)"
	[ -n "$ocid_found" ] && ocid_values="${ocid_values}${ocid_found}
"
done
for ocid_file in $ocid_required; do
	ocid_found="$(grep -hoE 'CI_RUNNER_COMPARTMENT_OCID:[[:space:]]*"ocid1\.compartment\.[A-Za-z0-9._-]+"' \
		"$ocid_file" 2>/dev/null |
		sed -E 's/.*"(ocid1\.compartment\.[A-Za-z0-9._-]+)"/\1/' || true)"
	ocid_count="$(printf '%s' "$ocid_found" | grep -c . || true)"
	if [ "$ocid_count" -ne 1 ]; then
		err "$ocid_file must pin exactly one literal CI_RUNNER_COMPARTMENT_OCID, found $ocid_count (an Actions variable trades a compile-time guarantee for an admin-mutable one — see the note in ci-runner-reaper.yml)"
	else
		ocid_values="${ocid_values}${ocid_found}
"
	fi
done
if [ "$fail" -eq 0 ]; then
	if [ "$(printf '%s' "$ocid_values" | sort -u | grep -c .)" -ne 1 ]; then
		err "CI_RUNNER_COMPARTMENT_OCID differs across the workflows that pin it ($ocid_required $ocid_optional); every literal must name the same compartment"
	else
		ok "CI_RUNNER_COMPARTMENT_OCID pinned as a literal in $ocid_required, and consistent wherever else it appears"
	fi
fi

# 7. Fork-checkout hygiene. Every checkout of the code under test must set
#    BOTH `persist-credentials: false` and `allow-unsafe-pr-checkout: true`.
#
#    The second is what makes the fork lane work at all: actions/checkout
#    refuses fork code under pull_request_target without it, and because the
#    action is pinned to the moving `@v4` tag, that guard arrived upstream and
#    silently broke every fork PR at Clone Repository with no change in this
#    repo. Asserting it means a future removal fails CI loudly instead of
#    breaking outside contributors invisibly again.
#
#    The first is the control that opt-in leans on. Once we tell checkout to
#    fetch untrusted code in a base-repo context, `persist-credentials: false`
#    is what keeps a writable token out of a workspace that fork code will be
#    built in. It was previously convention; opting past a security guard is
#    exactly when convention stops being good enough.
fork_bad_persist=""
fork_bad_optin=""
while read -r line_no flags; do
	case "$flags" in
	*P*) ;;
	*) fork_bad_persist="$fork_bad_persist $PIPELINE:$line_no" ;;
	esac
	case "$flags" in
	*A*) ;;
	*) fork_bad_optin="$fork_bad_optin $PIPELINE:$line_no" ;;
	esac
done <<EOF
$(awk '
    function flush() {
        if (is_fork) printf "%d %s%s\n", start, (has_persist ? "P" : "-"), (has_optin ? "A" : "-")
        is_fork = 0; has_persist = 0; has_optin = 0
    }
    # Anchored to the start of the line so a YAML KEY is required, not just a
    # mention of one. Unanchored, the comment above these settings — which
    # names `persist-credentials: false` while explaining why it matters —
    # satisfied the match by itself, and that half of the check could never
    # fail. Caught by mutation testing; a guard defeated by its own
    # documentation is worse than no guard, because it reports ok.
    /^      - (name|uses):/ { flush(); start = NR }
    /inputs\.checkout_repository/                       { is_fork = 1 }
    /^[[:space:]]*persist-credentials:[[:space:]]*false/     { has_persist = 1 }
    /^[[:space:]]*allow-unsafe-pr-checkout:[[:space:]]*true/ { has_optin = 1 }
    END { flush() }
' "$PIPELINE")
EOF
if [ -n "$fork_bad_persist" ]; then
	err "checkout of fork-authored code without 'persist-credentials: false' at:${fork_bad_persist}; a writable token must never reach a workspace holding untrusted code"
fi
if [ -n "$fork_bad_optin" ]; then
	err "checkout of fork-authored code without 'allow-unsafe-pr-checkout: true' at:${fork_bad_optin}; actions/checkout refuses fork code under pull_request_target without it, which breaks every fork PR at clone"
fi
if [ -z "$fork_bad_persist$fork_bad_optin" ]; then
	ok "fork-code checkouts set persist-credentials:false and allow-unsafe-pr-checkout:true"
fi

# 8. Every `run:` block in every workflow must be parseable by its shell.
#
# A workflow is not compiled, so a `run:` block with a syntax error is a
# runtime failure on a schedule nobody is watching. 4330a24f shipped exactly
# that: two apostrophes inside a single-quoted jq program in
# ci-runner-reaper.yml closed the quote, the step died with "syntax error near
# unexpected token |" on every 30-minute run, and the reaper — the guard
# against the June 2026 leak of ~209 instances, ~$6k — silently terminated
# nothing. It was found by reading a run log, not by CI.
#
# `bash -n` parses without executing, so this is safe and fast. It catches the
# whole quoting/heredoc/unbalanced-block class. It does NOT catch semantic
# faults (unset variables, wrong OCIDs) — for that this would need actionlint.
#
# Only default-shell and bash blocks are checked. A `shell: python` or
# `shell: pwsh` block is skipped rather than mis-parsed as bash; that skip is
# reported so a silent gap cannot masquerade as coverage.
bad_syntax=""
skipped_shells=""
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
while IFS= read -r wf; do
	# Extract every run: block with its job/step identity. Ruby is already a
	# hard dependency of this repo's tooling and ships with a YAML parser, so
	# the blocks come from a real parse rather than an indentation heuristic
	# that block scalars would defeat.
	ruby -ryaml -e '
      wf = ARGV[0]; out = ARGV[1]
      doc = YAML.load_file(wf) rescue nil
      exit 0 unless doc.is_a?(Hash) && doc["jobs"].is_a?(Hash)
      doc["jobs"].each do |job_id, job|
        next unless job.is_a?(Hash) && job["steps"].is_a?(Array)
        job["steps"].each_with_index do |step, i|
          next unless step.is_a?(Hash) && step["run"].is_a?(String)
          shell = (step["shell"] || job.dig("defaults", "run", "shell") ||
                   doc.dig("defaults", "run", "shell") || "bash").to_s
          label = "#{job_id} / #{step["name"] || "step #{i + 1}"}"
          # bash -e {0}, bash -euo pipefail {0} etc. are all bash.
          kind = shell.split(/\s/).first
          File.write(File.join(out, "block-#{job_id}-#{i}.sh"), step["run"])
          File.open(File.join(out, "index.txt"), "a") { |f|
            f.puts("block-#{job_id}-#{i}.sh\t#{kind}\t#{label}")
          }
        end
      end' "$wf" "$scratch" 2>/dev/null || continue

	[ -f "$scratch/index.txt" ] || continue
	while IFS=$'\t' read -r blockfile kind label; do
		case "$kind" in
		bash | sh | "") ;;
		*)
			skipped_shells="${skipped_shells}
  ${wf}: ${label} (shell: ${kind})"
			continue
			;;
		esac
		if ! errout="$(bash -n "$scratch/$blockfile" 2>&1)"; then
			bad_syntax="${bad_syntax}
  ${wf}: ${label}
    ${errout}"
		fi
	done <"$scratch/index.txt"
	rm -f "$scratch"/index.txt "$scratch"/block-*.sh
done < <(find .github/workflows -name '*.yml' -o -name '*.yaml' | sort)

if [ -n "$bad_syntax" ]; then
	err "workflow run: block(s) fail 'bash -n' and will die at runtime:${bad_syntax}"
else
	ok "every workflow run: block parses under bash -n"
fi
if [ -n "$skipped_shells" ]; then
	printf 'note: non-bash run: blocks not syntax-checked:%s\n' "$skipped_shells"
fi

cat >"$scratch/check-canary.rb" <<'RUBY'
require "yaml"
doc = YAML.load_file(ARGV[0])
jobs = doc.fetch("jobs")
%w[retry_within_window perf_report notify_failure].each do |job_id|
  condition = jobs.dig(job_id, "if").to_s
  expected = "outputs.canary != #{39.chr}true#{39.chr}"
  puts "#{job_id} does not exclude canaries" unless condition.include?(expected)
end
gate = jobs.dig("schedule_gate", "steps").find { |step| step["id"] == "resolve" }
body = gate && gate["run"].to_s
puts "canary duration is not bounded to 1800s" unless body&.include?("duration_seconds=1800")
puts "canary checkpoint slots are not cleared" unless body&.include?("checkpoints=()")
puts "canary retry attempts are not rejected" unless body&.include?("INPUT_RETRY_ATTEMPT")
puts "protection injection is not restricted to canaries" unless body&.include?("inject_protection_breach requires canary")
puts "preflight-only runs are not bounded to four hours" unless body&.include?("duration_seconds=14400")
puts "preflight-only runs do not clear checkpoint slots" unless body&.include?('if [ "${INPUT_CANARY:-false}" = "true" ] || [ "${INPUT_PREFLIGHT_ONLY:-false}" = "true" ]; then')
puts "preflight-only output is missing" unless body&.include?('echo "preflight_only=${INPUT_PREFLIGHT_ONLY:-false}"')
puts "preflight-only controls can combine with incompatible modes" unless body&.include?("preflight_only cannot be combined")
retry_condition = jobs.dig("retry_within_window", "if").to_s
puts "preflight-only dispatches can restart as soaks" unless retry_condition.include?("preflight_only != 'true'")
injection = jobs.dig("soak", "steps").find { |step| step["name"] == "Configure injected protection breach" }
puts "protection injection step is missing or not gate-controlled" unless injection&.dig("if").to_s == "needs.schedule_gate.outputs.inject_protection_breach == 'true'"
publish_steps = jobs.dig("perf_report", "steps")
control_checkout = publish_steps.find { |step| step["name"] == "Checkout CI control files" }
puts "dashboard publisher does not check out CI control files under ci-control" unless control_checkout&.dig("with", "path") == "ci-control"
renderer = publish_steps.find { |step| step["name"] == "Render dashboard charts" }
renderer_body = renderer && renderer["run"].to_s
puts "dashboard publisher renderer does not use the ci-control checkout" unless renderer_body&.include?("--manifest-path ci-control/scripts/soak-charts/Cargo.toml") && renderer_body&.include?("ci-control/scripts/soak-charts/target/release/soak-charts")
puts "OCI scheduled slots are not handled before manual dispatches" unless body&.include?('if [ -n "$INPUT_SCHEDULED_SLOT" ]; then')
puts "OCI scheduled inputs are not isolated from manual controls" unless body&.include?("scheduled_slot_epoch cannot be combined")
puts "Friday routing does not consistently target master" unless body&.scan("target_ref=master")&.length.to_i >= 2
puts "daily routing does not consistently target dev" unless body&.scan("target_ref=dev")&.length.to_i >= 2
puts "OCI scheduled runs are not deduplicated" unless body&.include?("scheduled slot already belongs to run")
puts "OCI scheduled runs do not reject dispatch delays over 900 seconds" unless body&.include?('[ "$trigger_delay" -gt 900 ]')
raw = File.read(ARGV[0])
puts "OCI scheduled runs are not serialized by slot" unless raw.include?("merge-recovery-soak-slot-")
puts "scheduled_slot_epoch input is missing" unless raw.include?("scheduled_slot_epoch:")
puts "scheduled run names are not slot-stable" unless raw.include?("Merge Recovery Soak [scheduled:{0}]")
soak = jobs.fetch("soak")
steps = Array(soak["steps"])
preflight = steps.find { |step| step["id"] == "integration_preflight" }
dispatch = steps.find { |step| step["name"] == "Dispatch integration preflight success status" }
pending = jobs.fetch("preflight_pending")
pending_body = pending.dig("steps", 0, "run").to_s
finalizer = jobs.fetch("preflight_finalize")
finalizer_body = finalizer.dig("steps", 0, "run").to_s
puts "full integration preflight is missing" unless preflight
puts "full integration preflight must skip canaries" unless preflight&.dig("if").to_s.include?("canary != 'true'")
puts "full integration preflight must use the bounded driver" unless preflight&.dig("run").to_s.include?("run-integration-preflight.sh")
puts "full integration preflight must use the harness-owned suite profile" unless preflight&.dig("env", "PREFLIGHT_PROFILE_FILE").to_s.end_with?("/system-integration/integration-tests/test/full-suite.txt")
puts "full integration preflight must subtract its elapsed time from the soak" unless preflight&.dig("run").to_s.include?("DURATION_SECONDS - elapsed")
puts "preflight-only runs still enforce a soak reserve" unless preflight&.dig("run").to_s.include?('[ "$PREFLIGHT_ONLY" != "true" ] && [ "$remaining" -lt 3600 ]')
puts "preflight-only pytest does not use the absolute window" unless preflight&.dig("run").to_s.include?("PREFLIGHT_END_EPOCH - started - 600")
puts "preflight-only pytest does not use the job window" unless preflight&.dig("run").to_s.include?("190 * 60")
puts "preflight-only launch timeout is not bounded" unless jobs.dig("launch_runner", "timeout-minutes").to_s.include?("preflight_only == 'true' && 30")
puts "preflight-only soak timeout is not bounded" unless soak["timeout-minutes"].to_s.include?("preflight_only == 'true' && 190")
puts "target SHA is not resolved before runner launch" unless jobs.dig("schedule_gate", "outputs", "target_sha").to_s.include?("resolve.outputs.target_sha") && body&.include?('target_sha="$(gh api')
puts "code checkout is not pinned to the resolved target SHA" unless steps.find { |step| step["name"] == "Checkout code under test" }&.dig("with", "ref").to_s.include?("outputs.target_sha")
puts "long-running soak job retains status write permission" if soak.dig("permissions", "statuses")
puts "pending status job lacks status write permission" unless pending.dig("permissions", "statuses") == "write"
puts "pending status job does not retry status publication" unless pending_body.include?("for attempt in 1 2 3")
puts "pending status job does not serialize status ownership" unless pending.dig("concurrency", "group").to_s.include?("soak-preflight-status-")
puts "pending status job does not reject older run attempts" unless pending_body.include?("current_is_newer") && pending_body.include?("current_attempt")
puts "pending status target does not identify one run attempt" unless pending.dig("steps", 0, "env", "RUN_URL").to_s.include?("github.run_attempt")
puts "soak does not wait for pending status publication" unless Array(soak["needs"]).include?("preflight_pending")
puts "successful preflight does not dispatch the isolated status workflow" unless dispatch&.dig("run").to_s.include?("soak-preflight-status.yml")
puts "success status target does not identify one run attempt" unless dispatch&.dig("env", "RUN_URL").to_s.include?("github.run_attempt")
puts "final status job lacks status write permission" unless finalizer.dig("permissions", "statuses") == "write"
puts "final status job does not cover all soak outcomes" unless finalizer["if"].to_s.start_with?("always()")
puts "final status job does not serialize with early success publication" unless finalizer.dig("concurrency", "group").to_s.include?("soak-preflight-status-")
puts "final status job does not enforce run ownership" unless finalizer_body.include?('current_run_id" != "$own_run_id') && finalizer_body.include?("current_is_newer")
puts "final status target does not identify one run attempt" unless finalizer.dig("steps", 0, "env", "RUN_URL").to_s.include?("github.run_attempt")
puts "soak job does not expose its preflight result" unless soak.dig("outputs", "preflight").to_s.include?("integration_preflight.outputs.result")
puts "soak job does not expose telemetry reset status" unless soak.dig("outputs", "telemetry_reset").to_s.include?("reset_telemetry.outcome")
segment_steps = steps.select { |step| step["name"].to_s.match?(/^Soak (segment [1-5]|final segment)$/) }
puts "expected six soak segments behind the preflight" unless segment_steps.length == 6
segment_steps.each do |step|
  condition = step["if"].to_s
  duration = step.dig("with", "duration_seconds").to_s
  puts "#{step['name']} runs during preflight-only dispatches" unless condition.include?("preflight_only != 'true'")
  puts "#{step['name']} does not stop after a preflight failure" unless condition.include?("integration_preflight.outputs.result == 'passed'")
  puts "#{step['name']} does not stop after telemetry cleanup fails" unless condition.include?("reset_telemetry.outcome == 'success'")
  puts "#{step['name']} no longer permits canaries" unless condition.include?("canary == 'true'")
  puts "#{step['name']} does not use the post-preflight budget" unless duration.include?("integration_preflight.outputs.remaining_seconds")
end
publisher_condition = jobs.dig("perf_report", "if").to_s
puts "dashboard publication runs for preflight-only dispatches" unless publisher_condition.include?("preflight_only != 'true'")
puts "dashboard publication does not require a completed preflight" unless publisher_condition.include?("needs.soak.outputs.preflight == 'passed'")
puts "dashboard publication does not require clean soak telemetry" unless publisher_condition.include?("needs.soak.outputs.telemetry_reset == 'success'")
runner = File.read(ARGV[1])
puts "preflight is not resource-limited to one worker" unless runner.include?("-n 1 --dist=loadgroup")
puts "preflight does not collect the complete suite before execution" unless runner.include?("--collect-only -q")
puts "preflight does not execute capability-gated tests" unless runner.scan("--run-all-node-capability-tests").length == 2
puts "preflight can pass after running fewer tests than it collected" unless runner.include?("tests == expected_tests")
puts "preflight does not reject skipped tests" unless runner.include?("skipped == 0")
puts "preflight runner contains a deselection" if runner.include?("--deselect")
status_doc = YAML.load_file(ARGV[2])
status_job = status_doc.dig("jobs", "report")
status_body = status_job&.dig("steps", 0, "run").to_s
puts "isolated status workflow lacks status write permission" unless status_doc.dig("permissions", "statuses") == "write"
puts "isolated status workflow accepts direct operator dispatches" unless status_body.include?('GITHUB_ACTOR" != "github-actions[bot]')
puts "isolated status workflow can overwrite a terminal failure" unless status_body.include?("failure|error")
puts "isolated status workflow does not require a current pending status" unless status_body.include?("pending)")
puts "isolated status workflow does not identify source run attempts" unless status_body.include?("source_attempt")
puts "isolated status workflow does not reject older source runs" unless status_body.include?("current_is_newer")
RUBY
soak_errors="$(ruby "$scratch/check-canary.rb" \
	.github/workflows/merge-recovery-soak.yml \
	scripts/run-integration-preflight.sh \
	.github/workflows/soak-preflight-status.yml)"
if [ -n "$soak_errors" ]; then
	err "soak workflow invariants failed: $(printf '%s' "$soak_errors" | tr '\n' ';')"
else
	ok "soak canaries are isolated and scheduled routing maps daily to dev and weekend to master"
fi

# Charts are cosmetic and must never block the publish that makes soak
# history durable (PR #232 review): a manifest-listed SVG that cannot be
# fetched or fails the content sniff is dropped with a warning, and the
# carried manifest is re-filtered so it only advertises files the deploy
# actually ships. Hostile FILENAMES in a manifest remain fatal — that is a
# poisoned manifest, not a cosmetic blip.
for publisher in \
	.github/workflows/merge-recovery-soak.yml \
	.github/workflows/soak-checkpoint-publish.yml \
	.github/workflows/soak-dashboard-pages.yml; do
	if grep -Fq 'dropping chart ${f} (HTTP ${code}) rather than blocking the publish' "$publisher" &&
		grep -Fq 'mv -f "site/data/${m}.filtered" "site/data/${m}"' "$publisher" &&
		grep -Fq 'lists a suspicious filename; refusing to republish it' "$publisher"; then
		ok "$publisher drops unfetchable chart SVGs and re-filters the manifest instead of blocking the publish"
	else
		err "$publisher must drop unfetchable manifest-listed SVGs (warning + manifest re-filter) while keeping hostile filenames fatal"
	fi
done

# The renderers must stage chart output outside site/ and only copy a fully
# successful series in, so a mid-render crash can never publish a truncated
# SVG over the carried set.
for renderer_wf in \
	.github/workflows/merge-recovery-soak.yml \
	.github/workflows/soak-dashboard-pages.yml; do
	if grep -Fq -- '--out-dir "chart-stage-$2"' "$renderer_wf" &&
		grep -Fq 'cp "chart-stage-$2"/* site/data/' "$renderer_wf"; then
		ok "$renderer_wf stages chart renders before publishing them"
	else
		err "$renderer_wf renders charts directly into site/data, risking a partially overwritten chart set on failure"
	fi
done

if [ "$fail" -ne 0 ]; then
	printf '::error::%s\n' "workflow security invariants violated; see errors above"
	exit 1
fi

echo "All workflow security invariants hold."
