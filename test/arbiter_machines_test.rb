# frozen_string_literal: true

require 'minitest/autorun'
require 'arbiter_machines'

class ArbiterMachinesTest < Minitest::Test
  def arbiter
    ArbiterMachines::Arbiter.new(priorities: [1, 5, 10], ceilings: [10, 10], protocol: :inheritance)
  end

  def test_transitive_inheritance_release_and_explicit_retry
    a = arbiter
    assert_equal [:granted, nil], a.acquire(task: 0, resource: 0)
    assert_equal [:already_owned, nil], a.acquire(task: 0, resource: 0)
    assert_equal [:granted, nil], a.acquire(task: 1, resource: 1)
    assert_equal [:blocked, 0], a.acquire(task: 1, resource: 0)
    assert_equal [:blocked, 1], a.acquire(task: 2, resource: 1)
    assert_equal [10, 10, 10], a.effective_priorities
    refute a.release(task: 2, resource: 0)
    assert a.cancel_wait(task: 2)
    assert_equal [5, 5, 10], a.effective_priorities
    assert a.release(task: 0, resource: 0)
    assert_equal [1, 5, 10], a.effective_priorities
    assert_equal [:granted, nil], a.acquire(task: 1, resource: 0)
  end

  def test_deadlock_does_not_replace_prior_state
    a = arbiter
    a.acquire(task: 0, resource: 0)
    a.acquire(task: 1, resource: 1)
    assert_equal [:blocked, 1], a.acquire(task: 0, resource: 1)
    assert_equal [:deadlock, nil], a.acquire(task: 1, resource: 0)
    assert_equal [1, 5, 10], a.effective_priorities
    assert a.cancel_wait(task: 0)
    assert_equal [:blocked, 0], a.acquire(task: 1, resource: 0)
    assert_equal [5, 5, 10], a.effective_priorities
  end

  def test_immediate_ceiling_and_invalid_declarations
    a = ArbiterMachines::Arbiter.new(priorities: [1, 5, 10], ceilings: [8, 10], protocol: :immediate_ceiling)
    assert_equal [:granted, nil], a.acquire(task: 0, resource: 0)
    assert_equal [8, 5, 10], a.effective_priorities
    assert_equal [:blocked, 0], a.acquire(task: 1, resource: 1)
    assert_equal [:granted, nil], a.acquire(task: 2, resource: 1)
    assert_equal [:invalid, nil], a.acquire(task: 2, resource: 0)
    assert_equal [8, 5, 10], a.effective_priorities
  end

  def test_exact_independent_task_and_resource_counts
    (1..8).each do |tasks|
      (1..8).each do |resources|
        a = ArbiterMachines::Arbiter.new(priorities: [0] * tasks, ceilings: [0] * resources, protocol: :inheritance)
        assert_equal tasks, a.task_count
        assert_equal resources, a.resource_count
        assert_equal [0] * tasks, a.effective_priorities
        assert_equal [:granted, nil], a.acquire(task: tasks - 1, resource: resources - 1)
        assert_equal [:invalid, nil], a.acquire(task: tasks, resource: 0)
        assert_equal [:invalid, nil], a.acquire(task: 0, resource: resources)
        refute a.cancel_wait(task: tasks)
        assert a.release(task: tasks - 1, resource: resources - 1)
      end
    end
  end

  def test_strict_integer_and_complete_call_validation
    a = arbiter
    assert_raises(TypeError) { a.acquire(task: 0, resource: 0.1) }
    assert_raises(ArgumentError) { a.acquire(task: 0, resource: 0, extra: true) }
    assert_equal [:granted, nil], a.acquire(task: 1, resource: 0)
    [0.5, '1', Object.new].each do |v|
      assert_raises(TypeError) { a.acquire(task: v, resource: 0) }
      assert_raises(TypeError) { a.release(task: 1, resource: v) }
      assert_raises(TypeError) { a.cancel_wait(task: v) }
      assert_raises(TypeError) do
        ArbiterMachines::Arbiter.new(priorities: [v], ceilings: [0], protocol: :inheritance)
      end
    end
    [-1, 2**32].each do |v|
      assert_raises(RangeError) { a.acquire(task: 0, resource: v) }
      assert_raises(RangeError) do
        ArbiterMachines::Arbiter.new(priorities: [0], ceilings: [v], protocol: :inheritance)
      end
    end
    assert_equal [:already_owned, nil], a.acquire(task: 1, resource: 0)
  end

  def test_constructor_limits_and_protocols
    [[], [0] * 9].each do |values|
      assert_raises(ArgumentError) do
        ArbiterMachines::Arbiter.new(priorities: values, ceilings: [0], protocol: :inheritance)
      end
      assert_raises(ArgumentError) do
        ArbiterMachines::Arbiter.new(priorities: [0], ceilings: values, protocol: :inheritance)
      end
    end
    assert_raises(ArgumentError) do
      ArbiterMachines::Arbiter.new(priorities: [1], ceilings: [1], protocol: :unknown)
    end
  end

  def test_gc_freeze_snapshots_and_independent_instances
    a = arbiter
    a.acquire(task: 0, resource: 0)
    a.acquire(task: 2, resource: 0)
    snapshot = a.effective_priorities
    snapshot[0] = 0
    GC.start
    GC.compact
    assert_equal [10, 5, 10], a.effective_priorities
    assert_equal [1, 5, 10], arbiter.effective_priorities
    a.freeze
    assert_raises(FrozenError) { a.acquire(task: 0, resource: 1) }
    assert_raises(FrozenError) { a.release(task: 0, resource: 0) }
    assert_raises(FrozenError) { a.cancel_wait(task: 2) }
    assert_equal [10, 5, 10], a.effective_priorities
  end
end
