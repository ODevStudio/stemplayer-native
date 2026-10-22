use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

#[test]
fn windows_product_excludes_retired_linux_dbus_packages() {
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--filter-platform",
            "x86_64-pc-windows-msvc",
            "--manifest-path",
            concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    let packages = metadata["packages"].as_array().unwrap();
    let app = packages
        .iter()
        .find(|package| package["name"] == "stemplayer")
        .unwrap();
    let dependencies: BTreeMap<_, Vec<_>> = metadata["resolve"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| {
            (
                node["id"].as_str().unwrap(),
                node["deps"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|dependency| dependency["pkg"].as_str().unwrap())
                    .collect(),
            )
        })
        .collect();
    let reachable = std::iter::successors(
        Some(BTreeSet::from([app["id"].as_str().unwrap()])),
        |reachable| {
            let expanded: BTreeSet<_> = reachable
                .iter()
                .copied()
                .chain(
                    reachable
                        .iter()
                        .flat_map(|id| dependencies[id].iter().copied()),
                )
                .collect();
            (expanded.len() > reachable.len()).then_some(expanded)
        },
    )
    .last()
    .unwrap();
    let retired: Vec<_> = packages
        .iter()
        .filter(|package| reachable.contains(package["id"].as_str().unwrap()))
        .filter_map(|package| package["name"].as_str())
        .filter(|name| matches!(*name, "zbus_names" | "zvariant" | "zvariant_derive"))
        .collect();
    assert!(
        retired.is_empty(),
        "Retired D-Bus packages entered the Windows product graph: {retired:?}"
    );
}
