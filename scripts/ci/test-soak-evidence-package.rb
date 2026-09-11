require 'tmpdir'
require 'fileutils'
require_relative 'check-soak-evidence-package'

class EvidencePackageTests
  def initialize
    @count = 0
  end

  def archive(path, files)
    Zlib::GzipWriter.open(path) do |gzip|
      Gem::Package::TarWriter.new(gzip) do |tar|
        files.each { |name, bytes| tar.add_file_simple(name, 0600, bytes.bytesize) { |entry| entry.write(bytes) } }
      end
    end
  end

  def fixture
    Dir.mktmpdir('soak-package-test-') do |root|
      package = File.join(root, 'package')
      Dir.mkdir(package)
      File.write(File.join(package, 'README.md'), "Reruns are digest-only. The archive is [EVIDENCE_ROOT]/raw-streams.tar.gz.\n")
      files = %w[b40-cycle.txt baseline-sample.txt initial-result.txt final-real.txt final-real-driver.txt final-real--cycle.txt final-real.txt.bak composed-tests.txt final-tests.txt supporting-tests.txt emergency-tests.txt real-system/cycle.txt real-system/retrieved/results/b40/driver.log real-system/retrieved/results/b38/driver.log tlc-case.log].to_h { |name| [name, "#{name}\n"] }
      entries = files.map do |path,bytes|
        name = path.start_with?('real-system/') ? SoakEvidencePackage.real_system_name(path, 'b40') : path
        {'raw_path' => path, 'raw_sha256' => Digest::SHA256.hexdigest(bytes), 'raw_bytes' => bytes.bytesize, 'stream_name' => name}
      end
      published = entries.reject { |entry| SoakEvidencePackage.rerun?(entry.fetch('stream_name')) }.map do |entry|
        name = entry.fetch('stream_name').sub(/\.log\z/, '.txt')
        File.write(File.join(package,name),files.fetch(entry.fetch('raw_path')))
        {'path' => "docs/cbc-evidence/example/#{name}", 'raw_path' => entry.fetch('raw_path'), 'original_sha256' => entry.fetch('raw_sha256'), 'original_bytes' => entry.fetch('raw_bytes'), 'published_sha256' => entry.fetch('raw_sha256'), 'published_bytes' => entry.fetch('raw_bytes')}
      end
      raw = File.join(root,'raw-streams.tar.gz')
      archive(raw,files)
      manifest = {'package_path' => 'docs/cbc-evidence/example', 'cycle_prefix' => 'b40', 'raw_archive' => {'raw_path' => '[EVIDENCE_ROOT]/raw-streams.tar.gz', 'raw_sha256' => Digest::SHA256.file(raw).hexdigest, 'raw_bytes' => File.size(raw), 'entries' => entries}, 'published_streams' => published}
      save(package,manifest)
      yield package,raw,manifest,files
    end
    @count += 1
  end

  def save(package, manifest)
    File.write(File.join(package,'manifest.jsonc'), "// Package test metadata.\n" + JSON.pretty_generate(manifest) + "\n")
  end

  def reject(message)
    begin
      yield
    rescue StandardError => error
      raise "Unexpected rejection: #{error.message}" unless error.message.include?(message)
      return
    end
    raise "The validator accepted an invalid case: #{message}"
  end

  def run
    fixture do |package,raw,manifest,_|
      parsed,entries = SoakEvidencePackage.validate_export(package)
      SoakEvidencePackage.validate_archive(parsed,entries,raw)
      raise 'The final-real exception differs.' unless manifest.fetch('published_streams').any? { |entry| entry.fetch('path').end_with?('/final-real.txt') }
      raise 'The final-real exception is too broad.' if Dir.children(package).any? { |name| name.start_with?('final-real') && name != 'final-real.txt' }
      raise 'A rerun was copied.' if Dir.children(package).any? { |name| SoakEvidencePackage.rerun?(name) }
    end
    fixture do |package,raw,_,_|
      File.unlink(raw)
      SoakEvidencePackage.validate_export(package)
    end
    fixture do |package,_,_,_|
      File.unlink(File.join(package,'b40-cycle.txt'))
      reject('A published entry is missing: b40-cycle.txt') { SoakEvidencePackage.validate_export(package) }
    end
    fixture do |package,_,manifest,_|
      manifest.fetch('published_streams').reject! { |entry| entry.fetch('path').end_with?('/b40-cycle.txt') }
      save(package,manifest)
      reject('A cycle stream has no published entry') { SoakEvidencePackage.validate_export(package) }
    end
    fixture do |package,raw,manifest,files|
      archive(raw,files.reject { |name,_| name == 'tlc-case.log' })
      manifest.fetch('raw_archive').merge!('raw_sha256' => Digest::SHA256.file(raw).hexdigest, 'raw_bytes' => File.size(raw))
      save(package,manifest)
      parsed,entries = SoakEvidencePackage.validate_export(package)
      reject('The raw archive is missing an inventoried member.') { SoakEvidencePackage.validate_archive(parsed,entries,raw) }
    end
    fixture do |package,raw,manifest,_|
      manifest.fetch('raw_archive').fetch('entries').last['raw_sha256'] = '0' * 64
      save(package,manifest)
      parsed,entries = SoakEvidencePackage.validate_export(package)
      reject('An archived digest differs: tlc-case.log') { SoakEvidencePackage.validate_archive(parsed,entries,raw) }
    end
    fixture do |package,raw,manifest,_|
      manifest.fetch('raw_archive').fetch('entries').last['raw_bytes'] += 1
      save(package,manifest)
      parsed,entries = SoakEvidencePackage.validate_export(package)
      reject('An archived byte count differs: tlc-case.log') { SoakEvidencePackage.validate_archive(parsed,entries,raw) }
    end
    fixture do |package,_,manifest,files|
      name = 'composed-tests.txt'
      bytes = files.fetch(name)
      File.write(File.join(package,name),bytes)
      manifest.fetch('published_streams') << {'path' => "docs/cbc-evidence/example/#{name}", 'raw_path' => name, 'original_sha256' => Digest::SHA256.hexdigest(bytes), 'original_bytes' => bytes.bytesize, 'published_sha256' => Digest::SHA256.hexdigest(bytes), 'published_bytes' => bytes.bytesize}
      save(package,manifest)
      reject('A rerun stream must remain digest-only.') { SoakEvidencePackage.validate_export(package) }
    end
    fixture do |package,_,_,files|
      File.write(File.join(package,'composed-tests.txt'),files.fetch('composed-tests.txt'))
      reject('A rerun file is present in the published package') { SoakEvidencePackage.validate_export(package) }
    end
    fixture do |package,_,_,_|
      File.write(File.join(package,'.gitignore'), "!*.log\n")
      reject('must not contain a local .gitignore') { SoakEvidencePackage.validate_export(package) }
    end
    fixture do |package,_,manifest,_|
      manifest.fetch('raw_archive').fetch('entries').last['raw_path'] = '../escape'
      save(package,manifest)
      reject('unsafe or duplicate path') { SoakEvidencePackage.validate_export(package) }
    end
    fixture do |package,_,_,_|
      File.write(File.join(package,'manifest.jsonc'), '/* unterminated')
      reject('unexpected token') { SoakEvidencePackage.validate_export(package) }
    end
    fixture do |package,_,_,_|
      File.write(File.join(package,'b40-cycle.txt'), 'different')
      reject('Published bytes differ: b40-cycle.txt') { SoakEvidencePackage.validate_export(package) }
    end
    fixture do |package,_,_,files|
      File.write(File.join(package,'final-real--cycle.txt'),files.fetch('final-real--cycle.txt'))
      reject('A rerun file is present in the published package') { SoakEvidencePackage.validate_export(package) }
    end
    fixture do |package,_,manifest,_|
      entry = manifest.fetch('raw_archive').fetch('entries').find { |e| e.fetch('raw_path') == 'real-system/retrieved/results/b38/driver.log' }
      entry['stream_name'] = 'b40-real-system--retrieved--results--b38--driver.log'
      save(package,manifest)
      reject('A prior-cycle retrieval must remain digest-only.') { SoakEvidencePackage.validate_export(package) }
    end
    fixture do |package,_,manifest,files|
      path = 'real-system/retrieved/results/b38/driver.log'
      entry = manifest.fetch('raw_archive').fetch('entries').find { |e| e.fetch('raw_path') == path }
      name = 'b40-prior-driver.txt'
      File.write(File.join(package,name),files.fetch(path))
      manifest.fetch('published_streams') << {'path' => "docs/cbc-evidence/example/#{name}", 'raw_path' => path, 'original_sha256' => entry.fetch('raw_sha256'), 'original_bytes' => entry.fetch('raw_bytes'), 'published_sha256' => entry.fetch('raw_sha256'), 'published_bytes' => entry.fetch('raw_bytes')}
      save(package,manifest)
      reject('A prior-cycle retrieval must remain digest-only.') { SoakEvidencePackage.validate_export(package) }
    end
    fixture do |package,_,manifest,_|
      stream = manifest.fetch('published_streams').find { |e| e.fetch('raw_path') == 'real-system/cycle.txt' }
      stream['path'] = 'docs/cbc-evidence/example/final-real--current.txt'
      save(package,manifest)
      reject('A current-cycle result must use its cycle prefix.') { SoakEvidencePackage.validate_export(package) }
    end
    fixture do |package,_,manifest,_|
      manifest['cycle_prefix'] = '../b40'
      save(package,manifest)
      reject('The cycle prefix is invalid.') { SoakEvidencePackage.validate_export(package) }
    end
    fixture do |package,_,manifest,_|
      names = manifest.fetch('published_streams').map { |e| File.basename(e.fetch('path')) }
      raise 'The current-cycle results are missing.' unless names.include?('b40-real-system--cycle.txt') && names.include?('b40-real-system--retrieved--results--b40--driver.txt')
      raise 'A prior-cycle retrieval was published.' if names.any? { |n| n.include?('b38') }
      SoakEvidencePackage.validate_export(package)
    end
    puts "PASS: All #{@count} evidence packaging regression cases passed."
  end
end

EvidencePackageTests.new.run
