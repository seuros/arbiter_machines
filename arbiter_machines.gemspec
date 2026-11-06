# frozen_string_literal: true

require_relative 'lib/arbiter_machines/version'

Gem::Specification.new do |spec|
  spec.name = 'arbiter_machines'
  spec.version = ArbiterMachines::VERSION
  spec.authors = ['Abdelkader Boudih']
  spec.email = ['oss@seuros.com']
  spec.summary = 'Native bounded priority inheritance and resource ceiling decisions'
  spec.description = 'Thin Ruby adapter to the standalone allocation-free arbiter-machines Rust core. ' \
                     'Initial unstable local implementation; caller enforces all decisions.'
  spec.homepage = 'https://github.com/seuros/arbiter_machines'
  spec.license = 'MIT'
  spec.required_ruby_version = '>= 4.0.0'
  spec.metadata['source_code_uri'] = spec.homepage
  spec.metadata['bug_tracker_uri'] = "#{spec.homepage}/issues"
  spec.metadata['rubygems_mfa_required'] = 'true'
  spec.metadata['cargo_crate_name'] = 'arbiter_machines_native'
  spec.metadata['cargo_manifest_path'] = 'ext/arbiter_machines_native/ffi/Cargo.toml'
  spec.files = Dir.glob(%w[lib/**/*.rb sig/**/*.rbs ext/**/*.{rb,rs,toml,md}
                          ext/**/LICENSE Cargo.toml Cargo.lock README.md CHANGELOG.md LICENSE.txt])
                  .select { |path| File.file?(path) }
  spec.require_paths = ['lib']
  spec.extensions = ['ext/arbiter_machines_native/extconf.rb']
  spec.add_dependency 'rb_sys', '~> 0.9', '>= 0.9.130'
end
