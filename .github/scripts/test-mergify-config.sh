#!/usr/bin/env bash
# Checks that .mergify.yml merges every pull request into dev, master,
# ca-canary, and ca-release, and that no queue can merge without the checks of
# its branch.
#
# DEV_REQUIRED and MASTER_REQUIRED copy the required status checks of the
# devProtect and masterProtect rulesets. A change of a ruleset changes the
# matching list here and the merge_conditions of .mergify.yml.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONFIG="${1:-$ROOT/.mergify.yml}"

DEV_REQUIRED='Static Checks|Script Tests|Soak Harness Tests|cargo-deny|Test (block-storage)|Test (comm)|Test (crypto)|Test (graphz)|Test (models)|Test (rholang)|Test (node)|Test (shared)|Integration Gate (amd64)|Integration Gate (arm64)|Test (casper 1/2)|Test (casper 2/2)|Test (rspace_plus_plus)|Commit Policy'
MASTER_REQUIRED='Static Checks|Script Tests|Soak Harness Tests|Test Gate (casper)|Test (rspace_plus_plus)|Test (rholang)|Test (shared)|Test (node)|Test (models)|Test (crypto)|Test (block-storage)|Test (comm)|Test (graphz)|cargo-deny|Commit Policy|Promotion Source'

DEV_REQUIRED="$DEV_REQUIRED" MASTER_REQUIRED="$MASTER_REQUIRED" ruby -ryaml -e '
  doc = YAML.load_file(ARGV[0])
  repo = "F1R3FLY-io/f1r3node-rust"
  heavy = ["Integration Gate (amd64)", "Integration Gate (arm64)"]
  review = ["#approved-reviews-by>=1", "#changes-requested-reviews-by=0"]
  queues = doc.fetch("queue_rules").to_h { |q| [q.fetch("name"), q] }
  rules = doc.fetch("pull_request_rules")
  errors = []
  checks = ->(q) { Array(q["merge_conditions"]).map { |c| c.to_s[/\Acheck-success=(.+)\z/, 1] }.compact }
  conds = ->(q) { Array(q["queue_conditions"]).map(&:to_s) }

  expected = %w[dev master ca-item ca-daily ca-release]
  errors << "the queues are #{queues.keys.sort.join(", ")}, not #{expected.sort.join(", ")}" unless queues.keys.sort == expected.sort

  mq = doc["merge_queue"] || {}
  errors << "the merge queue is not serial" unless mq["mode"] == "serial"
  errors << "the merge queue checks more than one pull request at a time" unless mq["max_parallel_checks"] == 1

  queues.each do |name, q|
    errors << "queue #{name} does not check one pull request at a time" unless q["batch_size"] == 1
    errors << "queue #{name} does not merge with a merge commit" unless q["merge_method"] == "merge"
    errors << "queue #{name} does not update by a merge of the base" unless q["update_method"] == "merge"
    errors << "queue #{name} retries its checks" unless q["max_checks_retries"] == 0
    errors << "queue #{name} has merge_conditions that differ from its queue_conditions" unless q["merge_conditions"] == q["queue_conditions"]
  end

  {
    "dev" => ENV.fetch("DEV_REQUIRED").split("|") + ["Promotion Source"],
    "master" => ENV.fetch("MASTER_REQUIRED").split("|") + heavy,
  }.each do |name, required|
    q = queues[name] or next
    (required - checks.(q)).each { |c| errors << "queue #{name} does not require #{c}" }
  end

  %w[dev master ca-item ca-release].each do |name|
    q = queues[name] or next
    c = conds.(q)
    (heavy - checks.(q)).each { |h| errors << "queue #{name} does not require #{h}" }
    errors << "queue #{name} does not require the ci-heavy label" unless c.include?("label=ci-heavy")
    errors << "queue #{name} accepts a fork head" unless c.include?("head-repo-full-name=#{repo}")
    (review - c).each { |r| errors << "queue #{name} does not require #{r}" }
  end

  {"dev" => "base=dev", "master" => "base=master", "ca-item" => "base=ca-canary",
   "ca-daily" => "base=ca-canary", "ca-release" => "base=ca-release"}.each do |name, base|
    q = queues[name] or next
    errors << "queue #{name} does not require #{base}" unless conds.(q).include?(base)
    routed = rules.any? do |r|
      r.dig("actions", "queue", "name") == name && Array(r["conditions"]).map(&:to_s).include?(base)
    end
    errors << "no pull request rule queues #{base} into #{name}" unless routed
  end

  if (q = queues["master"])
    pattern = conds.(q).map { |c| c[/\Ahead~=(.+)\z/, 1] }.compact.first
    if pattern.nil?
      errors << "queue master has no head pattern"
    else
      re = Regexp.new(pattern)
      %w[promote/20261020-040859 hotfix/fix-tls].each { |h| errors << "queue master refuses #{h}" unless re.match?(h) }
      %w[dev feature/x promote/ca-release-20261020-040859 backmerge/master-0123456789ab].each { |h| errors << "queue master accepts #{h}" if re.match?(h) }
    end
  end

  if (q = queues["dev"])
    pattern = conds.(q).map { |c| c[/\A-head~=(.+)\z/, 1] }.compact.first
    if pattern.nil?
      errors << "queue dev does not refuse the protected heads"
    else
      re = Regexp.new(pattern)
      %w[dev master ca-canary ca-release].each { |h| errors << "queue dev accepts the protected head #{h}" unless re.match?(h) }
      %w[feature/x backmerge/master-0123456789ab devtools].each { |h| errors << "queue dev refuses #{h}" if re.match?(h) }
    end
  end

  label = rules.select { |r| Array(r.dig("actions", "label", "add")).include?("ci-heavy") }
  if label.size != 1
    errors << "#{label.size} pull request rules add ci-heavy, not 1"
  else
    c = Array(label.first["conditions"]).map(&:to_s)
    base = c.map { |x| x[/\Abase~=(.+)\z/, 1] }.compact.first
    if base.nil?
      errors << "the ci-heavy rule has no base pattern"
    else
      re = Regexp.new(base)
      %w[dev master ca-canary ca-release].each { |b| errors << "the ci-heavy rule skips #{b}" unless re.match?(b) }
      %w[staging feature/x].each { |b| errors << "the ci-heavy rule labels #{b}" if re.match?(b) }
    end
    ["head-repo-full-name=#{repo}", "-label=ci-heavy", "-draft", "-head~=^backmerge/ca-release-", *review].each do |x|
      errors << "the ci-heavy rule does not require #{x}" unless c.include?(x)
    end
  end

  abort errors.map { |e| "FAIL: #{e}" }.join("\n") unless errors.empty?
' "$CONFIG"

printf 'PASS: Mergify has a queue and a queue rule for dev, master, ca-canary, and ca-release. The queue is serial, and each queue checks one pull request in place, without retries, and with identical queue and merge conditions. Each queue merges with a merge commit. The dev and master queues require every ruleset check. The heavy queues require ci-heavy, both Integration gates, a review, and a head of this repository. One rule adds ci-heavy to an approved pull request.\n'
