# frozen_string_literal: true

require 'minitest/autorun'
require 'arbiter_machines'

# The native boundary: sealed values, classes resolved at init, and
# clone-then-keep for `acquire`, whose answer can be a Ruby object.
class BindingTest < Minitest::Test
  Arbiter = ArbiterMachines::Arbiter
  Blocked = ArbiterMachines::Blocked

  def snapshot(arbiter)
    [
      (0...arbiter.resource_count).map { |resource| arbiter.owner(resource) },
      (0...arbiter.task_count).map { |task| arbiter.waiting_on(task) },
      arbiter.effective_priorities
    ]
  end

  # An arbiter where task 0 holds resource 0, so `acquire(1, 0)` answers a Blocked.
  def held
    Arbiter.new([1, 5], [10], :inheritance).tap { |a| a.acquire(0, 0) }
  end

  # Runs the block with `Blocked.new` replaced by `body`, which gets the
  # original `new` and the arguments.
  def with_blocked_new(body)
    singleton = Blocked.singleton_class
    singleton.alias_method(:__original_new, :new)
    singleton.remove_method(:new)
    original = Blocked.method(:__original_new)
    Blocked.define_singleton_method(:new) { |*args, **kw| body.call(original, *args, **kw) }
    yield
  ensure
    singleton.remove_method(:new)
    singleton.alias_method(:new, :__original_new)
    singleton.remove_method(:__original_new)
  end

  # `class ForgedBlocked < Blocked; end` raises TypeError, yet Ruby binds the
  # constant before `inherited` runs: the subclass leaks. Its `initialize`
  # skips the check by binding `Data#initialize` directly.
  def forged_class
    return Object.const_get(:ForgedBlocked) if Object.const_defined?(:ForgedBlocked)

    eval('class ForgedBlocked < ArbiterMachines::Blocked; end', TOPLEVEL_BINDING, __FILE__, __LINE__)
    flunk 'a named subclass of Blocked was accepted'
  rescue TypeError
    Object.const_get(:ForgedBlocked).class_eval do
      def initialize(**) = Data.instance_method(:initialize).bind_call(self, resource: -1)
    end
    Object.const_get(:ForgedBlocked)
  end

  def test_blocked_is_sealed
    assert_raises(TypeError) { Class.new(Blocked) { def initialize(**) = nil } }
    assert_raises(NoMethodError) { Blocked.allocate }
    assert_raises(RangeError) { Blocked.new(1).with(resource: -1) }
    # The named form raises too, but the constant leaks: the native boundary,
    # not the seal, is the authority.
    assert_operator forged_class, :<, Blocked
  end

  def test_a_forged_blocked_is_never_an_arbiter_answer
    forged = forged_class.new(resource: 0)
    assert_equal(-1, forged.resource)
    # The arbiter reads no Blocked from Ruby, so a forged one reaches nothing,
    # and it never equals a real answer.
    a = held
    refute_equal forged, a.acquire(1, 0)
    assert_instance_of Blocked, a.acquire(1, 0)
  end

  def test_blocked_class_is_resolved_once_at_init
    a = held
    ArbiterMachines.send(:remove_const, :Blocked)
    ArbiterMachines.const_set(:Blocked, String)
    GC.start
    answer = a.acquire(1, 0)
  ensure
    ArbiterMachines.send(:remove_const, :Blocked)
    ArbiterMachines.const_set(:Blocked, Blocked)
    assert_instance_of Blocked, answer
    assert_equal Blocked.new(0), answer
  end

  def test_a_raising_answer_applies_nothing
    a = held
    before = snapshot(a)
    with_blocked_new(->(*) { raise IOError, 'boom' }) do
      assert_raises(IOError) { a.acquire(1, 0) }
    end
    assert_equal before, snapshot(a)
    assert_nil a.waiting_on(1)
    assert_equal Blocked.new(0), a.acquire(1, 0)
  end

  def test_a_reentrant_answer_raises_and_applies_nothing
    a = held
    reenter = lambda do |original, *args, **kw|
      a.release(0, 0)
      original.call(*args, **kw)
    end
    with_blocked_new(reenter) do
      assert_raises(RuntimeError) { a.acquire(1, 0) }
    end
    # Only the re-entrant release happened; no stale wait was recorded.
    assert_equal [[nil], [nil, nil], [1, 5]], snapshot(a)
    assert_equal :granted, a.acquire(1, 0)
  end

  def test_an_answer_that_freezes_the_arbiter_applies_nothing
    a = held
    before = snapshot(a)
    freeze = lambda do |original, *args, **kw|
      a.freeze
      original.call(*args, **kw)
    end
    with_blocked_new(freeze) do
      assert_raises(FrozenError) { a.acquire(1, 0) }
    end
    assert_equal before, snapshot(a)
  end

  def test_reads_inside_the_answer_see_the_state_before
    a = held
    seen = nil
    peek = lambda do |original, *args, **kw|
      seen = a.waiting_on(1)
      original.call(*args, **kw)
    end
    with_blocked_new(peek) { assert_equal Blocked.new(0), a.acquire(1, 0) }
    assert_nil seen
    assert_equal 0, a.waiting_on(1)
  end

  def test_the_extension_alone_is_a_name_error
    lib = File.expand_path('../lib', __dir__)
    script = 'begin; require "arbiter_machines_native/arbiter_machines_native"; ' \
             'rescue NameError => e; print e.class; end'
    output = IO.popen([RbConfig.ruby, '-I', lib, '-e', script], err: %i[child out], &:read)
    assert_equal 'NameError', output
  end
end
