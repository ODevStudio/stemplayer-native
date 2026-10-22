use crate::DeviceReport;
use std::hash::{DefaultHasher, Hash, Hasher};

pub(crate) fn endpoint_id(hardware_id: &str) -> String {
    let digest = |domain: u8| {
        let mut hasher = DefaultHasher::new();
        domain.hash(&mut hasher);
        hardware_id.hash(&mut hasher);
        hasher.finish()
    };
    format!("endpoint-v1-{:016x}{:016x}", digest(0), digest(1))
}

pub(crate) fn selected_index<'a>(
    reports: impl Iterator<Item = &'a DeviceReport>,
    selected_id: Option<&str>,
) -> Option<usize> {
    let mut matches = reports.enumerate().filter(|(_, report)| {
        report.stereo_supported && selected_id.map_or(report.is_default, |id| report.id == id)
    });
    let (index, _) = matches.next()?;
    matches.next().is_none().then_some(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(hardware_id: &str) -> DeviceReport {
        DeviceReport {
            id: endpoint_id(hardware_id),
            name: "Synthetic output".to_owned(),
            is_default: false,
            default_configuration: None,
            supported_configurations: vec![],
            stereo_supported: true,
        }
    }

    #[test]
    fn reordered_endpoints_keep_the_selected_identity() {
        let first = report("synthetic-headphones");
        let second = report("synthetic-speakers");
        let selection = first.id.clone();
        assert_eq!(
            selected_index([&first, &second].into_iter(), Some(&selection)),
            Some(0)
        );
        assert_eq!(
            selected_index([&second, &first].into_iter(), Some(&selection)),
            Some(1)
        );
        assert_eq!(
            selected_index([&second].into_iter(), Some(&selection)),
            None
        );
    }

    #[test]
    fn ambiguous_and_legacy_identifiers_fail_closed() {
        let first = report("synthetic-endpoint");
        let duplicate = first.clone();
        assert_eq!(
            selected_index([&first, &duplicate].into_iter(), Some(&first.id)),
            None
        );
        assert_eq!(selected_index([&first].into_iter(), Some("device-1")), None);
    }

    #[test]
    fn endpoint_tokens_do_not_contain_hardware_identifiers() {
        let hardware_id = "synthetic-private-endpoint-identity";
        let id = endpoint_id(hardware_id);
        assert_eq!(id, endpoint_id(hardware_id));
        assert_ne!(id, endpoint_id("different-synthetic-endpoint"));
        assert!(!id.contains(hardware_id));
        assert!(id.len() < 128);
    }
}
