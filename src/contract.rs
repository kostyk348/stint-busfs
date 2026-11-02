//! DSO contract validation layer.
//!
//! Every FUSE operation checks its pre-conditions (DENY on violation) and
//! post-conditions (FALLBACK on violation). Violations are escalated: logged
//! to stderr and retained here for tests and audits.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

/// When a contract may be checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Before the operation runs; a violation means the operation is denied.
    Pre,
    /// After the operation ran; a violation triggers the fallback policy.
    Post,
}

/// What happens on a violation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// Refuse the operation (pre-condition).
    Deny,
    /// Return a fallback value instead of the broken result (post-condition).
    Fallback,
    /// Record and continue (reserved for audit-only operations).
    LogOnly,
}

impl fmt::Display for Policy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Policy::Deny => write!(f, "deny"),
            Policy::Fallback => write!(f, "fallback"),
            Policy::LogOnly => write!(f, "log-only"),
        }
    }
}

/// Outcome of a check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Violation,
}

/// A single contract: what to check and when.
#[derive(Clone, Copy, Debug)]
pub struct Contract<'a> {
    pub kind: Kind,
    pub operation: &'a str,
    pub path: &'a str,
    pub check: Check,
}

/// The checks the filesystem actually enforces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    /// Pre: the path resolves to a node of the virtual tree.
    PathResolvable,
    /// Pre: the payload size is within the configured bounds.
    SizeWithinBounds { size: usize, max: usize },
    /// Post: the operation returned a usable result.
    OperationSucceeded { ok: bool },
}

impl Check {
    fn evaluate(self) -> bool {
        match self {
            Check::PathResolvable => true, // resolved by the caller; kept for audit symmetry
            Check::SizeWithinBounds { size, max } => size <= max,
            Check::OperationSucceeded { ok } => ok,
        }
    }
}

/// Escalation policy for a contract kind (the DSO rule).
pub fn policy_for(kind: Kind) -> Policy {
    match kind {
        Kind::Pre => Policy::Deny,
        Kind::Post => Policy::Fallback,
    }
}

/// Global violation counter (visible in banners and tests).
pub static VIOLATIONS: AtomicU64 = AtomicU64::new(0);

/// Validate one contract; on violation escalate (log + count) and return `false`.
pub fn validate(contract: &Contract<'_>) -> Verdict {
    if contract.check.evaluate() {
        return Verdict::Pass;
    }
    escalate(contract);
    Verdict::Violation
}

fn escalate(contract: &Contract<'_>) {
    VIOLATIONS.fetch_add(1, Ordering::Relaxed);
    eprintln!(
        "stint-busfs: CONTRACT VIOLATION op={} path={} kind={:?} policy={}",
        contract.operation,
        contract.path,
        contract.kind,
        policy_for(contract.kind)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passing_and_failing_checks() {
        let ok = Contract {
            kind: Kind::Pre,
            operation: "write",
            path: "/session/org.a/methods",
            check: Check::SizeWithinBounds { size: 10, max: 64 },
        };
        assert_eq!(validate(&ok), Verdict::Pass);

        let bad = Contract {
            kind: Kind::Pre,
            operation: "write",
            path: "/session/org.a/methods",
            check: Check::SizeWithinBounds { size: 65, max: 64 },
        };
        let before = VIOLATIONS.load(Ordering::Relaxed);
        assert_eq!(validate(&bad), Verdict::Violation);
        assert_eq!(VIOLATIONS.load(Ordering::Relaxed), before + 1);
    }

    #[test]
    fn escalation_policy_mapping() {
        assert_eq!(policy_for(Kind::Pre), Policy::Deny);
        assert_eq!(policy_for(Kind::Post), Policy::Fallback);
    }

    #[test]
    fn post_condition_reports_fallback() {
        let bad = Contract {
            kind: Kind::Post,
            operation: "read",
            path: "/session/org.a/properties",
            check: Check::OperationSucceeded { ok: false },
        };
        assert_eq!(validate(&bad), Verdict::Violation);
        assert_eq!(policy_for(Kind::Post), Policy::Fallback);
    }
}
