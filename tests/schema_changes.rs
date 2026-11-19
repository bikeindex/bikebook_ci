use bikebook_data_checks::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::process::Command;

fn representative_vehicle_ids() -> Vec<String> {
    csv::Reader::from_path(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/representative_vehicles.csv"
    ))
    .unwrap()
    .records()
    .map(|record| record.unwrap()[0].to_string())
    .collect()
}

/// The data files the branch under test changes, from where it left `BIKEBOOK_BASE` (default `origin/main`)
fn changed_paths() -> Vec<String> {
    let base = std::env::var("BIKEBOOK_BASE").unwrap_or("origin/main".to_string());
    let output = Command::new("git")
        .current_dir(root())
        .args(["diff", "--name-only", "--no-renames"])
        .arg(format!("{base}...HEAD"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git diff from {base}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(String::from)
        .collect()
}

fn linked_component_paths(files: &BTreeMap<String, Value>, key: &str) -> Vec<String> {
    files
        .values()
        .flat_map(|data| {
            let mut found = vec![];
            strings(data, String::new(), &mut found);
            found
                .into_iter()
                .filter(|(_, found_key, id)| found_key == key && id.starts_with("c/"))
                .filter_map(|(_, _, id)| Catalog::path(id))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// A schema change carries its new shape into the representative vehicles, the components they link and those
/// components' bases, leaving every other data file for a later change
fn schema_change_problems(
    changed: &[String],
    vehicle_ids: &[String],
    read: impl Fn(&[String]) -> BTreeMap<String, Value>,
) -> Vec<String> {
    if !changed
        .iter()
        .any(|path| path.starts_with(&format!("{SCHEMA_DIRECTORY}/")))
    {
        return vec![];
    }
    let vehicles = vehicle_ids
        .iter()
        .filter_map(|id| Catalog::path(id))
        .collect::<Vec<_>>();
    let components = linked_component_paths(&read(&vehicles), "id");
    let bases = linked_component_paths(&read(&components), "base");
    changed
        .iter()
        .filter(|path| {
            DATA_DIRECTORIES
                .iter()
                .any(|directory| path.starts_with(&format!("{directory}/")))
                && ![&vehicles, &components, &bases]
                    .iter()
                    .any(|paths| paths.contains(path))
        })
        .map(|path| {
            format!("{path}: a schema change edits only the representative vehicles and the components they link")
        })
        .collect()
}

fn missing(ids: &[String]) -> Vec<String> {
    ids.iter()
        .filter(|id| {
            !Catalog::path(id).is_some_and(|path| {
                path.starts_with("vehicle_models/") && root().join(path).is_file()
            })
        })
        .map(|id| format!("{id} isn't a vehicle model in this catalog"))
        .collect()
}

#[test]
fn a_schema_change_edits_only_the_representative_vehicles() {
    assert_none(
        "schema changes",
        schema_change_problems(&changed_paths(), &representative_vehicle_ids(), parsed),
    );
}

#[test]
fn a_schema_change_editing_other_data_files_is_reported() {
    let files = BTreeMap::from([
        (
            "vehicle_models/trek/2026/fuel.json".to_string(),
            json!({"motors": [{"id": "c/tq/2025/hpr60"}]}),
        ),
        (
            "components/tq/2025/hpr60.json".to_string(),
            json!({"base": "c/tq/2024/hpr50"}),
        ),
    ]);
    let read = |paths: &[String]| {
        files
            .clone()
            .into_iter()
            .filter(|(path, _)| paths.contains(path))
            .collect()
    };
    let changed = [
        "schemas/vehicle_model.json",
        "vehicle_models/trek/2026/fuel.json",
        "components/tq/2025/hpr60.json",
        "components/tq/2024/hpr50.json",
        "README.markdown",
        "vehicle_models/trek/2026/marlin.json",
        "components/bosch/2025/cx.json",
        "e_vehicle_classifications/us/class_1.json",
    ]
    .map(String::from);
    let vehicle_ids = ["m/trek/2026/fuel".to_string()];
    assert_eq!(
        schema_change_problems(&changed, &vehicle_ids, read),
        [
            "vehicle_models/trek/2026/marlin.json: a schema change edits only the representative vehicles and the components they link",
            "components/bosch/2025/cx.json: a schema change edits only the representative vehicles and the components they link",
            "e_vehicle_classifications/us/class_1.json: a schema change edits only the representative vehicles and the components they link"
        ]
    );
    assert_eq!(
        schema_change_problems(&changed[5..], &vehicle_ids, read),
        Vec::<String>::new()
    );
}

#[test]
fn every_representative_vehicle_is_in_the_catalog() {
    let ids = representative_vehicle_ids();
    assert!(!ids.is_empty());
    assert_none("representative vehicles", missing(&ids));
}

#[test]
fn the_representative_vehicles_are_in_alphabetical_order() {
    let ids = representative_vehicle_ids();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
}

#[test]
fn a_representative_vehicle_missing_from_the_catalog_is_reported() {
    let ids = [
        "m/specialized/2025/haul_st",
        "m/specialized/2025/haul",
        "c/shimano/deore",
    ]
    .map(String::from);
    assert_eq!(
        missing(&ids),
        [
            "m/specialized/2025/haul isn't a vehicle model in this catalog",
            "c/shimano/deore isn't a vehicle model in this catalog"
        ]
    );
}
