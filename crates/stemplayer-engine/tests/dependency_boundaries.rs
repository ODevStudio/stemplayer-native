use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

#[test]
fn engine_dependency_direction_is_enforced() {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--locked", "--no-deps", "--format-version", "1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    let packages = metadata["packages"].as_array().unwrap();
    let dependencies: BTreeMap<_, BTreeSet<_>> = packages
        .iter()
        .map(|package| {
            let name = package["name"].as_str().unwrap().to_owned();
            let dependencies = package["dependencies"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|dependency| dependency["name"].as_str())
                .map(str::to_owned)
                .collect();
            (name, dependencies)
        })
        .collect();

    let engine = &dependencies["stemplayer-engine"];
    let allowed = BTreeSet::from([
        "serde_json".to_owned(),
        "stemplayer-decode".to_owned(),
        "stemplayer-domain".to_owned(),
        "stemplayer-format".to_owned(),
        "stemplayer-mixer".to_owned(),
        "stemplayer-output".to_owned(),
        "stemplayer-timeline".to_owned(),
    ]);
    assert_eq!(engine, &allowed);
    assert_eq!(
        dependencies["stemplayer-domain"],
        BTreeSet::from(["serde".to_owned(), "serde_json".to_owned()])
    );
    assert!(!engine.contains("stemplayer-ui"));
    assert!(!engine.contains("stemplayer-platform"));
    assert!(!engine.contains("stemplayer-network"));
}

#[test]
fn every_workspace_package_is_inside_the_native_root() {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--locked", "--no-deps", "--format-version", "1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    let root = Path::new(metadata["workspace_root"].as_str().unwrap());
    for package in metadata["packages"].as_array().unwrap() {
        let manifest = Path::new(package["manifest_path"].as_str().unwrap());
        assert!(manifest.starts_with(root), "{}", manifest.display());
    }
}
