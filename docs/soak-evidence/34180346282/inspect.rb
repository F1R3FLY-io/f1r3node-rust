require 'csv'
require 'digest'
require 'fileutils'
require 'json'
require 'time'

root, output = ARGV
abort 'Usage: ruby inspect.rb INPUT_DIRECTORY OUTPUT_DIRECTORY' unless root && output
FileUtils.mkdir_p(output)

read_json = ->(name) { JSON.parse(File.read(File.join(root, name))) }
write_json = ->(name, value) { File.write(File.join(output, name), JSON.pretty_generate(value) + "\n") }
members = read_json.call('archive-members-sha256.json')
iterations = read_json.call('extracted/iterations.json')
summary = read_json.call('extracted/summary.json')
abort 'Unexpected candidate' unless summary.fetch('target_sha') == '0f5d2b7414786cd27b8a686ca636485c35a5edce'
abort 'Unexpected iteration inventory' unless iterations.map { |row| row.fetch('iteration') } == (1..51).to_a
abort 'Unexpected provider' unless iterations.all? { |row| %w[docker subprocess].include?(row.fetch('provider')) }
abort 'Unexpected failure inventory' unless iterations.reject { |row| row.fetch('exit_code').zero? }.map { |row| row.fetch('iteration') } == [42]
abort 'Summary disagrees with iteration inventory' unless summary.fetch('iterations') == 51 && summary.fetch('failures') == 1

phase_pattern = /^\s+Phase (\w+): inclusion p50=([\d.]+)s p95=([\d.]+)s, finalization p50=([\d.]+)s p95=([\d.]+)s, LFB #(\d+)->#(\d+) \(([\d.]+) blk\/min\), unfinalized=(\d+)/
phases = []
CSV.open(File.join(output, 'iterations.csv'), 'w') do |csv|
  csv << %w[iteration provider exit_code start_utc finish_utc duration_s reported_p50_ms reported_p95_ms reported_p99_ms reported_latency_samples unfinalized log_path]
  iterations.each do |item|
    id = item.fetch('iteration')
    provider = item.fetch('provider')
    path = format('iteration-%05d-%s/pytest.log', id, provider)
    found = []
    File.foreach(File.join(root, 'extracted', path)).with_index(1) do |line, number|
      break if line.include?(' Captured log call ')
      match = phase_pattern.match(line)
      next unless match
      values = match.captures
      found << [id, provider, values[0], *values[1..4].map { |v| Float(v) }, Integer(values[5]), Integer(values[6]), Float(values[7]), Integer(values[8]), path, number]
    end
    abort "Unexpected phase count: #{path}" unless found.length == 5 && found.map { |row| row[2] }.uniq.length == 5
    phases.concat(found)
    latency = item.fetch('finalization_latency')
    csv << [id, provider, item.fetch('exit_code'), Time.at(item.fetch('started_at')).utc.iso8601, Time.at(item.fetch('finished_at')).utc.iso8601, item.fetch('duration_s'), latency['p50_ms'], latency['p95_ms'], latency['p99_ms'], latency['samples'], found.sum { |row| row[10] }, path]
  end
end
CSV.open(File.join(output, 'phases.csv'), 'w') do |csv|
  csv << %w[iteration provider phase inclusion_p50_s inclusion_p95_s finalization_p50_s finalization_p95_s lfb_start lfb_end lfb_blocks_per_minute unfinalized log_path log_line]
  phases.each { |row| csv << row }
end
abort 'Unexpected unfinalized count' unless phases.sum { |row| row[10] } == 5

primary = members.reject { |path, _| path.include?('/log-archive/') }
by_digest = primary.group_by { |_, info| info.fetch('sha256') }
nested = members.select { |path, _| path.include?('/log-archive/') }
matches = nested.select { |path, _| path.end_with?('/node-metrics-timeseries.csv') }.map do |path, info|
  {path: path, bytes: info.fetch('bytes'), identical_primary_paths: (by_digest[info.fetch('sha256')] || []).map(&:first)}
end
write_json.call('storage.json', {
  unit: 'bytes',
  artifact_uncompressed_bytes: members.values.sum { |info| info.fetch('bytes') },
  artifact_files: members.length,
  primary_node_csv_bytes: primary.select { |path, _| path.match?(%r{\Aiteration-[^/]+/node-metrics-timeseries.csv\z}) }.values.sum { |info| info.fetch('bytes') },
  failure_archive_bytes: nested.values.sum { |info| info.fetch('bytes') },
  failure_archive_files: nested.length,
  failure_archive_node_log_bytes: nested.select { |path, _| path.end_with?('.log') }.values.sum { |info| info.fetch('bytes') },
  failure_archive_csv_matches: matches,
  passive_argument_bytes_including_nul: File.binread(File.join(root, 'extracted/summary.json')).sub(/\n+\z/, '').bytesize + 1,
  limitation: 'File lengths do not measure filesystem allocation or total writer growth.'
})

headers = {
  'node-metrics-timeseries.csv' => %w[elapsed_s node metric value],
  'resource-timeseries.csv' => %w[elapsed_s node memory_mb cpu_percent memory_limit_mb],
  'resource-percore-timeseries.csv' => %w[elapsed_s node core cpu_percent]
}
iterations.each do |item|
  directory = format('iteration-%05d-%s', item.fetch('iteration'), item.fetch('provider'))
  headers.each do |name, expected|
    CSV.open(File.join(root, 'extracted', directory, name)) do |csv|
      abort "Unexpected CSV header: #{directory}/#{name}" unless csv.shift == expected
    end
  end
end

raw = [40, 42, 44].map do |id|
  path = format('iteration-%05d-subprocess/node-metrics-timeseries.csv', id)
  names = Hash.new(0)
  nodes = {}
  metric_cache = {}
  last_counters = {}
  decreases = []
  rows = 0
  CSV.open(File.join(root, 'extracted', path)) do |csv|
    abort 'Unexpected raw CSV schema' unless csv.shift == headers.fetch('node-metrics-timeseries.csv')
    csv.each do |row|
      abort "Malformed row in #{path}" unless row.length == 4
      time = Float(row[0])
      value = Float(row[3])
      abort "Nonfinite row in #{path}" unless time.finite? && value.finite?
      name = metric_cache[row[2]] ||= row[2].split('{', 2).first
      names[name] += 1
      node = nodes[row[1]] ||= {rows: 0, first_elapsed_s: time, last_elapsed_s: time, distinct_sample_times: 0, max_observed_gap_s: 0, queues: {}}
      abort "Reversed sample clock in #{path}" if time < node[:last_elapsed_s]
      if node[:rows].zero? || time != node[:last_elapsed_s]
        node[:distinct_sample_times] += 1
        node[:max_observed_gap_s] = [node[:max_observed_gap_s], time - node[:last_elapsed_s]].max
      end
      node[:rows] += 1
      node[:last_elapsed_s] = time
      if %w[block_processing_queue_pending proposer_queue_pending].include?(name)
        gauge = node[:queues][row[2]] ||= {peak: value, peak_elapsed_s: time, unit: 'pending work items'}
        gauge.merge!(peak: value, peak_elapsed_s: time) if value > gauge[:peak]
      end
      if name.match?(/_(count|calls)\z/) || name.start_with?('block_validation_repeat_deploy_carrier_')
        key = [row[1], row[2]]
        previous = last_counters[key]
        decreases << {node: row[1], metric: row[2], elapsed_s: time, previous: previous, value: value} if previous && value < previous
        last_counters[key] = value
      end
      rows += 1
    end
  end
  {path: path, rows: rows, nodes: nodes, metric_name_row_counts: names.sort.to_h, selected_cumulative_series_decreases: decreases}
end
write_json.call('raw-metrics-inspection.json', {
  headers_checked: 153,
  complete_csv_parse_iterations: [40, 42, 44],
  clock_unit: 'seconds since the resource monitor started',
  restart_policy: 'Each iteration and node label stays separate. Counter decreases are observations, not proof of process restart. No rate or duration bound is derived across gaps or resets.',
  limitations: 'The collector omits metric type declarations and process-start metrics. Missing metrics are not zero. Sample gaps do not establish an exact number of missing samples.',
  iterations: raw
})
puts "PASS: 51 outcomes, 255 distinct phase reports, 153 CSV headers, and #{raw.sum { |row| row[:rows] }} selected raw CSV rows."
