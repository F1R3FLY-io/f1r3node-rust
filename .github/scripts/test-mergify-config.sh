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

CODEOWNERS="$ROOT/.github/CODEOWNERS" DEV_REQUIRED="$DEV_REQUIRED" MASTER_REQUIRED="$MASTER_REQUIRED" ruby -ryaml -e '
  doc = YAML.load_file(ARGV[0])
  repo = "F1R3FLY-io/f1r3node-rust"
  heavy = ["Integration Gate (amd64)", "Integration Gate (arm64)"]
  review = ["#approved-reviews-by>=1", "#changes-requested-reviews-by=0"]
  maintainer = {"or" => ["#approved-reviews-by>=1", "author=@F1R3FLY-io/ci-maintainers"]}
  heavy_or = {"or" => [
    {"and" => ["label=ci-heavy"] + heavy.map { |h| "check-success=#{h}" }},
    {"and" => ["label=ci-heavy-waived", "author=@F1R3FLY-io/ci-maintainers"]},
  ]}
  maintained = ["#review-requested=0", "#changes-requested-reviews-by=0"]
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
    "dev" => ENV.fetch("DEV_REQUIRED").split("|") - heavy + ["Promotion Source"],
    "master" => ENV.fetch("MASTER_REQUIRED").split("|") + heavy,
  }.each do |name, required|
    q = queues[name] or next
    (required - checks.(q)).each { |c| errors << "queue #{name} does not require #{c}" }
  end

  %w[dev master ca-item ca-release].each do |name|
    q = queues[name] or next
    c = conds.(q)
    if %w[dev ca-item].include?(name)
      errors << "queue #{name} does not require ci-heavy with both Integration gates, or a ci-maintainers waiver" unless Array(q["queue_conditions"]).include?(heavy_or)
      errors << "queue #{name} requires ci-heavy outside the waiver block" if c.include?("label=ci-heavy")
    else
      (heavy - checks.(q)).each { |h| errors << "queue #{name} does not require #{h}" }
      errors << "queue #{name} does not require the ci-heavy label" unless c.include?("label=ci-heavy")
      errors << "queue #{name} accepts the ci-heavy-waived label" if q["queue_conditions"].to_s.include?("ci-heavy-waived")
    end
    errors << "queue #{name} accepts a fork head" unless c.include?("head-repo-full-name=#{repo}")
    if %w[dev ca-item].include?(name)
      raw = Array(q["queue_conditions"])
      errors << "queue #{name} does not require an approval or a ci-maintainers author" unless raw.include?(maintainer)
      errors << "queue #{name} accepts a ci-maintainers author without the any-of block" if c.include?("author=@F1R3FLY-io/ci-maintainers")
      (maintained - c).each { |r| errors << "queue #{name} does not require #{r}" }
    else
      (review - c).each { |r| errors << "queue #{name} does not require #{r}" }
      errors << "queue #{name} accepts a merge without an approval" if Array(q["queue_conditions"]).any? { |x| x.is_a?(Hash) }
    end
  end

  if (q = queues["dev"])
    owners = Array(q["queue_conditions"]).find do |x|
      x.is_a?(Hash) && x["or"].is_a?(Array) && x["or"].any? { |y| y.to_s.start_with?("-files~=") }
    end
    if owners.nil?
      errors << "queue dev does not enforce the CODEOWNERS paths"
    else
      %w[author=@F1R3FLY-io/ci-maintainers approved-reviews-by=@F1R3FLY-io/ci-maintainers].each do |x|
        errors << "the dev code-owner condition does not accept #{x}" unless owners["or"].include?(x)
      end
      errors << "the dev code-owner condition has #{owners["or"].size} alternatives, not 3" unless owners["or"].size == 3
      re = Regexp.new(owners["or"].find { |y| y.to_s.start_with?("-files~=") }.delete_prefix("-files~="))
      File.readlines(ENV.fetch("CODEOWNERS")).each do |line|
        pattern = line.strip.split(/\s+/).first
        next if pattern.nil? || pattern.start_with?("#")
        samples = if pattern.start_with?("/")
          pattern.end_with?("/") ? [pattern[1..] + "x"] : [pattern[1..]]
        else
          [pattern, "casper/#{pattern}"]
        end
        samples.each { |p| errors << "the dev code-owner pattern misses #{p} (CODEOWNERS #{pattern})" unless re.match?(p) }
      end
      %w[casper/src/lib.rs docs/ci.md scripts/bench/run.sh .github/dependabot.yml README.md].each do |p|
        errors << "the dev code-owner pattern matches #{p}, which CODEOWNERS does not own" if re.match?(p)
      end
    end
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
    ["head-repo-full-name=#{repo}", "-label=ci-heavy", "-closed", "-draft", "-head~=^backmerge/ca-release-", "-head~=^(dev|master|ca-canary|ca-release)$"].each do |x|
      errors << "the ci-heavy rule does not require #{x}" unless c.include?(x)
    end
    waiver = {"or" => ["-label=ci-heavy-waived", "base~=^(master|ca-release)$"]}
    errors << "the ci-heavy rule ignores a waiver into dev or ca-canary, or honors one into master or ca-release" unless Array(label.first["conditions"]).include?(waiver)
    errors << "the ci-heavy rule still waits for an approval" if c.any? { |x| x.include?("approved-reviews-by") }
    ca_heads = {"or" => ["-base=ca-canary", "head~=^ca/"]}
    errors << "the ci-heavy rule labels a ca-canary pull request that is not a ca/ item" unless Array(label.first["conditions"]).include?(ca_heads)
  end

  prio = doc["priority_rules"]
  if !prio.is_a?(Array) || prio.empty?
    errors << "there are no priority rules"
  else
    match = lambda do |c, pr|
      case c
      when Hash
        c["or"] ? c["or"].any? { |x| match.(x, pr) } : c["and"].all? { |x| match.(x, pr) }
      when /\Alabel=(.+)\z/ then pr[:labels].include?($1)
      when /\Ahead~=(.+)\z/ then Regexp.new($1).match?(pr[:head])
      when /\Abase=(.+)\z/ then pr[:base] == $1
      else errors << "the priority rules use the unknown condition #{c}"; false
      end
    end
    rank = lambda do |head, base, labels = []|
      pr = {head: head, base: base, labels: labels}
      r = prio.find { |x| Array(x["conditions"]).all? { |c| match.(c, pr) } }
      [r ? r["priority"] : 2000, r ? r["allow_checks_interruption"] : false]
    end
    {
      ["hotfix/security-tls", "master"] => 6000, ["fix/rustsec-2026-0387", "dev"] => 6000,
      ["hotfix/crypto-bigint-0.7.9", "master", ["security"]] => 6000, ["feature/x", "dev", ["security"]] => 6000,
      ["hotfix/tls", "master"] => 5000, ["fix/tls", "dev"] => 5000, ["fix/tls", "master"] => 2000, ["hotfix/tls", "dev"] => 2000,
      ["promote/20261020-040859", "master"] => 4000, ["backmerge/master-0123456789ab", "dev"] => 4000,
      ["ca/sync-dev-0123456789ab", "ca-canary"] => 4000, ["promote/ca-release-20261020-040859", "ca-release"] => 4000,
      ["perf/x", "dev"] => 3000, ["perf/x", "master"] => 3000, ["ci/x", "dev"] => 2500,
      ["feat/x", "dev"] => 2000, ["feature/x", "dev"] => 2000, ["docs/x", "dev"] => 2000,
    }.each do |(head, base, labels), want|
      got, interrupt = rank.(head, base, labels || [])
      errors << "#{head} into #{base}#{labels ? " with #{labels.join(",")}" : ""} gets priority #{got}, not #{want}" unless got == want
      errors << "#{head} into #{base} interrupts the checks that run" if interrupt && want != 6000
      errors << "#{head} into #{base} does not interrupt the checks that run" if want == 6000 && interrupt != true
    end
    prio.each do |r|
      errors << "the priority rule #{r["name"]} does not set allow_checks_interruption" unless [true, false].include?(r["allow_checks_interruption"])
    end
    sec = rules.select { |r| Array(r.dig("actions", "label", "add")).include?("security") }
    if sec.size != 1
      errors << "#{sec.size} pull request rules add the security label, not 1"
    else
      re = Regexp.new(Array(sec.first["conditions"]).map(&:to_s).map { |x| x[/\Ahead~=(.+)\z/, 1] }.compact.first.to_s)
      %w[hotfix/security-tls fix/RUSTSEC-2026-0387].each { |h| errors << "the security label rule skips #{h}" unless re.match?(h) }
      %w[hotfix/tls feature/x].each { |h| errors << "the security label rule labels #{h}" if re.match?(h) }
    end
  end

  abort errors.map { |e| "FAIL: #{e}" }.join("\n") unless errors.empty?
' "$CONFIG"

printf 'PASS: Mergify has a queue and a queue rule for dev, master, ca-canary, and ca-release. The queue is serial, and each queue checks one pull request in place, without retries, and with identical queue and merge conditions. Each queue merges with a merge commit. The dev and master queues require every ruleset check. The master and ca-release queues require ci-heavy and both Integration gates. The dev and ca-item queues require them or a ci-heavy-waived label with a ci-maintainers author. Each heavy queue requires a review and a head of this repository. One rule adds ci-heavy to every pull request, except a protected head, a ca-canary pull request that is not a ca/ item, and a waived pull request into dev or ca-canary. The priority rules rank security fixes first, then fix/ into dev and hotfix/ into master, the release flow, perf/, ci/, and every other head, and only security fixes interrupt running checks.\n'
