use crate::{BoundedText, Severity};
use serde::Serialize;
use std::sync::Arc;

const MAX_DIAGNOSTIC_DETAILS: usize = 16;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DiagnosticDetail {
    pub key: BoundedText<64>,
    pub value: BoundedText<128>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PublicDiagnostic {
    code: BoundedText<64>,
    message: BoundedText<256>,
    severity: Severity,
    count: u64,
    details: Arc<[DiagnosticDetail]>,
}

impl PublicDiagnostic {
    pub fn new(
        code: impl AsRef<str>,
        message: impl AsRef<str>,
        severity: Severity,
        details: Vec<DiagnosticDetail>,
    ) -> Option<Self> {
        if details.len() > MAX_DIAGNOSTIC_DETAILS {
            return None;
        }
        Some(Self {
            code: BoundedText::new(code.as_ref()).ok()?,
            message: BoundedText::truncated(message),
            severity,
            count: 1,
            details: details.into(),
        })
    }

    pub fn incremented(mut self) -> Self {
        self.count = self.count.saturating_add(1);
        self
    }

    pub const fn count(&self) -> u64 {
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_saturate() {
        let mut diagnostic =
            PublicDiagnostic::new("D_UNDERRUN", "Output underrun", Severity::Warning, vec![])
                .unwrap();
        diagnostic.count = u64::MAX;
        assert_eq!(diagnostic.incremented().count(), u64::MAX);
    }
}
