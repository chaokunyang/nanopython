//! Reference-counting and periodic cycle-detection primitives used by NanoPython.
//! The runtime backend currently owns object memory directly; this crate captures
//! shared policy and diagnostics hooks.

#[derive(Debug, Clone)]
pub struct GcPolicy {
    pub cycle_candidate_threshold: usize,
    pub allocation_threshold_bytes: usize,
}

impl Default for GcPolicy {
    fn default() -> Self {
        Self {
            cycle_candidate_threshold: 10_000,
            allocation_threshold_bytes: 8 * 1024 * 1024,
        }
    }
}

impl GcPolicy {
    pub fn should_run_cycle_detector(
        &self,
        candidate_count: usize,
        allocated_bytes_since_last_pass: usize,
    ) -> bool {
        candidate_count >= self.cycle_candidate_threshold
            || allocated_bytes_since_last_pass >= self.allocation_threshold_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::GcPolicy;

    #[test]
    fn triggers_by_candidate_threshold() {
        let policy = GcPolicy::default();
        assert!(policy.should_run_cycle_detector(10_000, 0));
    }

    #[test]
    fn triggers_by_allocation_threshold() {
        let policy = GcPolicy::default();
        assert!(policy.should_run_cycle_detector(0, 8 * 1024 * 1024));
    }
}
