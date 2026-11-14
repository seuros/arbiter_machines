# frozen_string_literal: true

require_relative 'arbiter_machines/version'

module ArbiterMachines
  # The core's `Acquisition::Blocked`: the task must wait, and its wait on
  # `resource` (a resource index) is now recorded. Like every value the core
  # can answer, `resource` is a non-negative Integer that fits a usize: anything
  # else raises RangeError (TypeError for a non-Integer).
  Blocked = Data.define(:resource) do
    def initialize(resource:)
      resource = ArbiterMachines.__send__(:__blocked, resource)
      super
    end
  end
end

# Native is required: there is no pure-Ruby fallback. It validates and builds
# the Data value above.
require 'arbiter_machines_native/arbiter_machines_native'

module ArbiterMachines
  private_class_method :__blocked

  # Sealed: no subclass can skip the check by overriding `initialize`, and no
  # bare `allocate` builds a value that never ran it.
  Blocked.singleton_class.undef_method(:allocate)
  Blocked.define_singleton_method(:inherited) { |_| raise TypeError, "#{Blocked} cannot be subclassed" }
end
