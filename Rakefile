# frozen_string_literal: true

require 'bundler/gem_tasks'
require 'minitest/test_task'
require 'rb_sys/extensiontask'

Minitest::TestTask.create(:minitest) do |task|
  task.libs << 'test'
  task.libs << 'lib'
  task.test_globs = ['test/**/*_test.rb']
end

GEMSPEC = Gem::Specification.load('arbiter_machines.gemspec')
SUPPORTED_NATIVE_PLATFORMS = %w[
  arm64-darwin
  x86_64-darwin
  aarch64-linux
  x86_64-linux
  x86_64-linux-musl
].freeze

native_ext = RbSys::ExtensionTask.new('arbiter_machines_native', GEMSPEC) do |extension|
  extension.lib_dir = 'lib/arbiter_machines_native'
  extension.tmp_dir = 'tmp/rb_sys'
  extension.cross_platform = SUPPORTED_NATIVE_PLATFORMS if ENV.key?('RUBY_TARGET')
end

# A single-version cross build stages its binary at the host binary's path,
# which the gem package depends on, so packaging would also compile for the
# host and fail where the host linker can't target the platform.
if ENV.key?('RUBY_TARGET')
  host_binary = "#{native_ext.lib_dir}/#{native_ext.name}.#{RbConfig::CONFIG['DLEXT']}"
  Rake::Task[host_binary].clear_prerequisites if Rake::Task.task_defined?(host_binary)
end

desc 'Compile and run Ruby boundary tests'
task test: %i[compile minitest]
namespace :native do
  desc 'Compile release native extension'
  task build: ['rb_sys:env:release', 'compile']
end
desc 'Test Rust core'
task(:cargo) { sh 'cargo', 'test', '-p', 'arbiter-machines' }
desc 'Strict Rust lint check'
task(:clippy) { sh 'cargo', 'clippy', '--workspace', '--all-targets', '--', '-D', 'warnings' }
desc 'Validate RBS signatures'
task(:rbs) { sh 'rbs', '-I', 'sig', 'validate' }
task default: %i[test cargo rbs]

Dir[File.join(__dir__, 'tasks/*.rake')].sort.each { |f| load f }
