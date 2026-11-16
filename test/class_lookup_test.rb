# frozen_string_literal: true

require 'minitest/autorun'
require 'open3'
require 'rbconfig'

# The class of the value the extension wraps (Arbiter) is fixed when it
# loads. Nothing a caller does later, redefining the constant or
# `Object.const_get`, reaches an arbiter being built. Each case runs in a
# fresh process: the class is fixed once per process, at load.
class ClassLookupTest < Minitest::Test
  LIB = File.expand_path('../lib', __dir__)

  PRELUDE = <<~RUBY
    require 'arbiter_machines'
    A = ArbiterMachines
  RUBY

  # Runs `body` after PRELUDE in a new Ruby; answers its last line of output.
  def isolated(body)
    out, err, status = Open3.capture3(RbConfig.ruby, '-I', LIB, '-e', PRELUDE + body)
    assert status.success?, "exited #{status.exitstatus}: #{err}"
    out.lines.last&.chomp
  end

  # Bug: an Arbiter constant redefined before the first `new` made the value
  # an instance of the new class, without #acquire.
  def test_a_redefined_arbiter_constant_does_not_change_the_arbiter_class
    assert_equal 'true granted 0', isolated(<<~RUBY)
      original = A::Arbiter
      A.send(:remove_const, :Arbiter)
      A.const_set(:Arbiter, Class.new)
      arbiter = original.new([1, 5], [10], :inheritance)
      puts [arbiter.instance_of?(original), arbiter.acquire(0, 0), arbiter.owner(0)].join(' ')
    RUBY
  end

  # Bug: building the first arbiter ran `Object.const_get`, Ruby code in the
  # middle of a native constructor.
  def test_building_an_arbiter_runs_no_const_get
    assert_equal '0 Arbiter', isolated(<<~RUBY)
      hits = 0
      Object.define_singleton_method(:const_get) do |*args|
        hits += 1
        super(*args)
      end
      arbiter = A::Arbiter.new([1, 5], [10], :inheritance)
      puts [hits, arbiter.class.name.split('::').last].join(' ')
    RUBY
  end

  # Bug: a redefined `Object.const_get` that raised hit an `unwrap` in the
  # extension: an uncatchable fatal, and the process exited.
  def test_a_raising_const_get_does_not_kill_the_process
    assert_equal 'granted released', isolated(<<~RUBY)
      Object.define_singleton_method(:const_get) { |*| raise IOError, 'lookup ran in a constructor' }
      arbiter = A::Arbiter.new([1, 5], [10], :inheritance)
      puts [arbiter.acquire(0, 0), arbiter.release(0, 0)].join(' ')
    RUBY
  end
end
