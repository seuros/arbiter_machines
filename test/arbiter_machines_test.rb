# frozen_string_literal: true

require 'minitest/autorun'
require 'arbiter_machines'

class ArbiterMachinesTest < Minitest::Test
  Arbiter = ArbiterMachines::Arbiter
  Blocked = ArbiterMachines::Blocked

  def arbiter
    Arbiter.new([1, 5, 10], [10, 10], :inheritance)
  end

  # Everything an arbiter shows of itself: equal before and after means nothing moved.
  def snapshot(arbiter)
    [
      (0...arbiter.resource_count).map { |resource| arbiter.owner(resource) },
      (0...arbiter.task_count).map { |task| arbiter.waiting_on(task) },
      arbiter.effective_priorities
    ]
  end

  def assert_unchanged(arbiter, expected)
    before = snapshot(arbiter)
    assert_equal expected, yield
    assert_equal before, snapshot(arbiter)
  end

  def test_transitive_inheritance_release_and_explicit_retry
    a = arbiter
    assert_equal :granted, a.acquire(0, 0)
    assert_equal :granted, a.acquire(1, 1)
    assert_equal Blocked.new(0), a.acquire(1, 0)
    assert_equal Blocked.new(1), a.acquire(2, 1)
    assert_equal [10, 10, 10], a.effective_priorities
    assert_equal [0, 1, 0, 1], [a.owner(0), a.owner(1), a.waiting_on(1), a.waiting_on(2)]
    assert_equal :cancelled, a.cancel_wait(2)
    assert_equal [5, 5, 10], a.effective_priorities
    assert_equal :released, a.release(0, 0)
    assert_equal [nil, nil], [a.owner(0), a.waiting_on(1)]
    assert_equal [1, 5, 10], a.effective_priorities
    assert_equal :granted, a.acquire(1, 0)
  end

  def test_every_refusal_is_a_symbol_and_changes_nothing
    a = arbiter
    a.acquire(0, 0)
    a.acquire(1, 1)
    assert_equal Blocked.new(1), a.acquire(0, 1)
    assert_unchanged(a, :deadlock) { a.acquire(1, 0) }
    assert_unchanged(a, :unknown_task) { a.acquire(3, 0) }
    assert_unchanged(a, :unknown_resource) { a.acquire(0, 2) }
    assert_unchanged(a, :wrong_owner) { a.release(2, 0) }
    assert_unchanged(a, :unknown_task) { a.release(3, 0) }
    assert_unchanged(a, :unknown_resource) { a.release(0, 2) }
    assert_unchanged(a, :unknown_task) { a.cancel_wait(3) }
    assert_equal :released, a.release(0, 0)
    # The arbiter keeps no history, so a second release cannot be proven a repeat.
    assert_unchanged(a, :unowned) { a.release(0, 0) }
    ceiling = Arbiter.new([1, 10], [8], :immediate_ceiling)
    assert_unchanged(ceiling, :above_ceiling) { ceiling.acquire(1, 0) }
  end

  def test_repeats_are_honoured_and_change_nothing
    a = arbiter
    assert_equal :granted, a.acquire(0, 0)
    assert_unchanged(a, :already_owned) { a.acquire(0, 0) }
    assert_equal Blocked.new(0), a.acquire(1, 0)
    assert_unchanged(a, Blocked.new(0)) { a.acquire(1, 0) }
    assert_equal :cancelled, a.cancel_wait(1)
    assert_unchanged(a, :not_waiting) { a.cancel_wait(1) }
    assert_unchanged(a, :not_waiting) { a.cancel_wait(0) }
  end

  def test_immediate_ceiling
    a = Arbiter.new([1, 5, 10], [8, 10], :immediate_ceiling)
    assert_equal :immediate_ceiling, a.protocol
    assert_equal :granted, a.acquire(0, 0)
    assert_equal [8, 5, 10], a.effective_priorities
    assert_equal Blocked.new(0), a.acquire(1, 1)
    assert_equal :granted, a.acquire(2, 1)
    assert_equal :above_ceiling, a.acquire(2, 0)
    assert_equal [8, 5, 10], a.effective_priorities
  end

  def test_exact_independent_task_and_resource_counts
    assert_equal [8, 8], [Arbiter::TASK_CAPACITY, Arbiter::RESOURCE_CAPACITY]
    (0..8).each do |tasks|
      (0..8).each do |resources|
        a = Arbiter.new([0] * tasks, [0] * resources, :inheritance)
        assert_equal [tasks, resources, :inheritance], [a.task_count, a.resource_count, a.protocol]
        assert_equal [0] * tasks, a.effective_priorities
        assert_equal :unknown_task, a.acquire(tasks, 0)
        assert_equal :unknown_task, a.cancel_wait(tasks)
        assert_nil a.owner(resources)
        assert_nil a.waiting_on(tasks)
        next if tasks.zero?

        assert_equal :unknown_resource, a.acquire(0, resources)
        next if resources.zero?

        assert_equal :granted, a.acquire(tasks - 1, resources - 1)
        assert_equal :released, a.release(tasks - 1, resources - 1)
      end
    end
  end

  def test_blocked_is_validated_like_every_other_value
    assert_equal 3, Blocked.new(3).resource
    assert_equal Blocked.new(resource: 3), Blocked.new(3)
    assert_raises(RangeError) { Blocked.new(-1) }
    assert_raises(RangeError) { Blocked.new(2**64) }
    [0.5, '1', nil].each { |v| assert_raises(TypeError) { Blocked.new(v) } }
    assert_raises(NoMethodError) { ArbiterMachines.__blocked(1) }
  end

  def test_strict_integers_and_exception_classes
    a = arbiter
    assert_equal :granted, a.acquire(1, 0)
    [0.5, '1', nil, Object.new].each do |v|
      assert_raises(TypeError) { a.acquire(v, 0) }
      assert_raises(TypeError) { a.release(1, v) }
      assert_raises(TypeError) { a.cancel_wait(v) }
      assert_raises(TypeError) { a.owner(v) }
      assert_raises(TypeError) { Arbiter.new([v], [0], :inheritance) }
    end
    assert_raises(RangeError) { a.acquire(0, -1) }
    assert_raises(RangeError) { a.waiting_on(2**64) }
    [-1, 2**32].each do |v|
      assert_raises(RangeError) { Arbiter.new([0], [v], :inheritance) }
    end
    assert_raises(ArgumentError) { a.acquire(0) }
    assert_equal :already_owned, a.acquire(1, 0)
  end

  def test_rejected_configuration_raises
    assert_raises(RangeError) { Arbiter.new([0] * 9, [0], :inheritance) }
    assert_raises(RangeError) { Arbiter.new([0], [0] * 9, :inheritance) }
    assert_raises(RangeError) { Arbiter.new([1], [1], :unknown) }
    assert_raises(TypeError) { Arbiter.new([1], [1], 'inheritance') }
    assert_raises(TypeError) { Arbiter.new(1, [1], :inheritance) }
    assert_raises(ArgumentError) { Arbiter.new([1], [1]) }
  end

  def test_an_array_subclass_is_refused
    subclass = Class.new(Array)
    error = assert_raises(TypeError) { Arbiter.new(subclass[1], [1], :inheritance) }
    assert_equal 'base must be an Array of Integers', error.message
    error = assert_raises(TypeError) { Arbiter.new([1], subclass[1], :inheritance) }
    assert_equal 'ceilings must be an Array of Integers', error.message
    assert_equal 1, Arbiter.new([1], [1].tap(&:singleton_class), :inheritance).task_count
  end

  def test_frozen_is_checked_before_the_arguments
    a = arbiter.freeze
    assert_raises(FrozenError) { a.acquire(0.5, nil) }
    assert_raises(FrozenError) { a.release('1', -1) }
    assert_raises(FrozenError) { a.cancel_wait(nil) }
  end

  def test_gc_freeze_snapshots_and_independent_instances
    a = arbiter
    a.acquire(0, 0)
    a.acquire(2, 0)
    priorities = a.effective_priorities
    priorities[0] = 0
    GC.start
    GC.compact
    assert_equal [10, 5, 10], a.effective_priorities
    assert_equal [1, 5, 10], arbiter.effective_priorities
    a.freeze
    assert_raises(FrozenError) { a.acquire(0, 1) }
    assert_raises(FrozenError) { a.release(0, 0) }
    assert_raises(FrozenError) { a.cancel_wait(2) }
    assert_equal [10, 5, 10], a.effective_priorities
  end
end
