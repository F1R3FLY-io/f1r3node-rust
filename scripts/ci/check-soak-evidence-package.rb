require 'json'
require 'digest'
require 'zlib'
require 'rubygems/package'

module SoakEvidencePackage
  def self.rerun?(name)
    return false if name == 'final-real.txt'
    %w[composed-* final-* supporting-* emergency-* tlc-*.log].any? { |pattern| File.fnmatch?(pattern, name) }
  end

  def self.prior_cycle_retrieval?(path, cycle)
    source_cycle = path[%r{\Areal-system/retrieved/results/(b[1-9][0-9]*)(?=/|[-.])}, 1]
    !source_cycle.nil? && source_cycle != cycle
  end

  def self.real_system_name(path, cycle)
    prefix = prior_cycle_retrieval?(path, cycle) ? 'composed-prior-cycle' : cycle
    "#{prefix}-#{path.gsub('/', '--')}"
  end

  def self.relative_path?(path)
    path.is_a?(String) && !path.empty? && !path.start_with?('/') &&
      !path.match?(/[\x00-\x1f\x7f\\]/) && path.split('/', -1).none? { |part| ['', '.', '..'].include?(part) }
  end

  def self.digest_record?(record, digest, bytes)
    record[digest].is_a?(String) && record[digest].match?(/\A[a-f0-9]{64}\z/) &&
      record[bytes].is_a?(Integer) && record[bytes] >= 0
  end

  def self.validate_export(package)
    raise 'A new evidence package must not contain a local .gitignore.' if File.exist?(File.join(package, '.gitignore'))
    manifest = JSON.parse(File.read(File.join(package, 'manifest.jsonc')))
    prefix = manifest.fetch('package_path')
    raise 'The package path is unsafe.' unless relative_path?(prefix)
    cycle = manifest.fetch('cycle_prefix')
    raise 'The cycle prefix is invalid.' unless cycle.is_a?(String) && cycle.match?(/\Ab[1-9][0-9]*\z/)
    archive = manifest.fetch('raw_archive')
    raise 'The raw archive identity is invalid.' unless digest_record?(archive, 'raw_sha256', 'raw_bytes')
    location = archive.fetch('raw_path')
    raise 'The raw archive location must use [EVIDENCE_ROOT].' unless location.start_with?('[EVIDENCE_ROOT]/') && relative_path?(location.delete_prefix('[EVIDENCE_ROOT]/'))
    entries = {}
    names = {}
    archive.fetch('entries').each do |entry|
      path = entry.fetch('raw_path')
      raise 'The raw archive contains an unsafe or duplicate path.' unless relative_path?(path) && !entries.key?(path)
      raise 'A raw archive digest or byte count is invalid.' unless digest_record?(entry, 'raw_sha256', 'raw_bytes')
      if entry.key?('stream_name')
        name = entry.fetch('stream_name')
        raise 'A stream name is unsafe or duplicated.' unless relative_path?(name) && !name.include?('/') && !names.key?(name)
        raise 'A prior-cycle retrieval must remain digest-only.' if prior_cycle_retrieval?(path, cycle) && !rerun?(name)
        names[name] = path
      end
      entries[path] = entry
    end
    raise 'The raw archive inventory is empty.' if entries.empty?
    published = {}
    used_raw = {}
    manifest.fetch('published_streams').each do |stream|
      path = stream.fetch('path')
      raise 'A published path is outside the package.' unless path.start_with?(prefix + '/')
      name = path.delete_prefix(prefix + '/')
      raise 'A published path is unsafe or duplicated.' unless relative_path?(name) && !name.include?('/') && !published.key?(name)
      entry = entries.fetch(stream.fetch('raw_path'))
      raise 'A prior-cycle retrieval must remain digest-only.' if prior_cycle_retrieval?(entry.fetch('raw_path'), cycle)
      if entry.fetch('raw_path').start_with?('real-system/')
        raise 'A current-cycle result must use its cycle prefix.' unless name == real_system_name(entry.fetch('raw_path'), cycle).sub(/\.log\z/, '.txt')
      end
      raise 'A rerun stream must remain digest-only.' if rerun?(entry.fetch('stream_name')) || rerun?(name)
      raise 'A raw stream has multiple published entries.' if used_raw.key?(stream.fetch('raw_path'))
      raise 'A published entry has no matching raw identity.' unless entry.fetch('raw_sha256') == stream.fetch('original_sha256') && entry.fetch('raw_bytes') == stream.fetch('original_bytes')
      raise 'A published digest or byte count is invalid.' unless digest_record?(stream, 'published_sha256', 'published_bytes')
      file = File.join(package, name)
      raise "A published entry is missing: #{name}" unless File.file?(file) && !File.symlink?(file)
      raise "Published bytes differ: #{name}" unless File.size(file) == stream.fetch('published_bytes') && Digest::SHA256.file(file).hexdigest == stream.fetch('published_sha256')
      published[name] = stream
      used_raw[stream.fetch('raw_path')] = true
    end
    entries.each_value do |entry|
      next unless entry.key?('stream_name')
      name = entry.fetch('stream_name')
      if rerun?(name)
        raise "A rerun file is present in the published package: #{name}" if File.exist?(File.join(package, name))
      else
        raise "A cycle stream has no published entry: #{name}" unless used_raw.key?(entry.fetch('raw_path'))
      end
    end
    allowed = published.keys + %w[manifest.jsonc README.md .gitattributes]
    Dir.children(package).each do |name|
      raise "An unlisted package member is present: #{name}" unless allowed.include?(name)
      raise "A package member is not a regular file: #{name}" unless File.file?(File.join(package, name)) && !File.symlink?(File.join(package, name))
    end
    raise 'The package README is missing.' unless File.file?(File.join(package, 'README.md'))
    [manifest, entries]
  end

  def self.validate_repository(root)
    inventory = JSON.parse(File.read(File.join(root, 'docs/claims/soak-claim-inventory.jsonc')))
    inputs = inventory.fetch('candidate').fetch('inputs_sha256')
    paths = Dir.glob(File.join(root, 'docs/cbc-evidence/*/*'), File::FNM_DOTMATCH)
    paths.each do |path|
      next unless File.file?(path) || File.symlink?(path)
      raise "A rerun file remains in the repository: #{File.basename(path)}" if rerun?(File.basename(path))
    end
    inputs.each_key do |path|
      next unless path.start_with?('docs/cbc-evidence/')
      raise "A rerun must not have a candidate binding: #{path}" if rerun?(File.basename(path))
    end
    packages = []
    paths.select { |p| File.basename(p) == 'manifest.jsonc' }.sort.each do |path|
      data = JSON.parse(File.read(path))
      next unless data.key?('packaging')
      package = File.dirname(path)
      manifest, = validate_export(package)
      relative = package.delete_prefix(root.chomp('/') + '/')
      raise 'The package location differs from its manifest.' unless manifest.fetch('package_path') == relative
      registered = inventory.fetch('evidence').values.any? { |entry| entry.fetch('path') == "#{relative}/manifest.jsonc" }
      raise "The evidence package is not registered: #{relative}" unless registered
      Dir.children(package).each do |name|
        key = "#{relative}/#{name}"
        raise "A package binding is missing or stale: #{key}" unless inputs[key] == Digest::SHA256.file(File.join(package, name)).hexdigest
      end
      packages << relative
    end
    packages
  end

  def self.validate_archive(manifest, entries, path)
    archive = manifest.fetch('raw_archive')
    raise 'The raw archive bytes differ.' unless File.size(path) == archive.fetch('raw_bytes') && Digest::SHA256.file(path).hexdigest == archive.fetch('raw_sha256')
    seen = {}
    total = 0
    Zlib::GzipReader.open(path) do |gzip|
      Gem::Package::TarReader.new(gzip) do |tar|
        tar.each do |entry|
          name = entry.full_name
          raise 'The raw archive contains an unsafe member.' unless entry.file? && relative_path?(name) && !seen.key?(name)
          expected = entries.fetch(name)
          total += entry.header.size
          raise 'The raw archive exceeds its expansion limit.' if total > 512 * 1024 * 1024
          raise "An archived byte count differs: #{name}" unless entry.header.size == expected.fetch('raw_bytes')
          digest = Digest::SHA256.new
          digest.update(entry.read(65_536)) until entry.eof?
          raise "An archived digest differs: #{name}" unless digest.hexdigest == expected.fetch('raw_sha256')
          seen[name] = true
        end
      end
    end
    raise 'The raw archive is missing an inventoried member.' unless seen.keys.sort == entries.keys.sort
    true
  end
end

if $PROGRAM_NAME == __FILE__
  repository = ARGV[0] == '--repository'
  valid = repository ? [1, 2].include?(ARGV.size) : ARGV.size == 1 || (ARGV.size == 3 && ARGV[1] == '--raw-archive')
  unless valid
    warn 'Usage: ruby scripts/ci/check-soak-evidence-package.rb <package> [--raw-archive <archive>] | --repository [root]'
    exit 2
  end
  begin
    if repository
      packages = SoakEvidencePackage.validate_repository(File.expand_path(ARGV[1] || Dir.pwd))
      puts "PASS: Publication bindings are current. Package count: #{packages.size}. No rerun files or bindings remain."
    else
      manifest, entries = SoakEvidencePackage.validate_export(ARGV[0])
      SoakEvidencePackage.validate_archive(manifest, entries, ARGV[2]) if ARGV.size == 3
      puts 'PASS: Published evidence matches its manifest. Reruns remain digest-only.'
      puts 'PASS: Every raw archive member matches its inventory.' if ARGV.size == 3
    end
  rescue StandardError => error
    warn "FAIL: #{error.message}"
    exit 1
  end
end
