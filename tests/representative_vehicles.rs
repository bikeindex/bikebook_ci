use bikebook_data_checks::*;

/// `bin/check_schema_change` reads each id as `m/<path>`, a file in vehicle_models, and a market's model,
/// `<id>@<market>`, as its id's
fn missing(ids: &[String]) -> Vec<String> {
    ids.iter()
        .filter(|id| {
            !Catalog::path(id.split('@').next().unwrap()).is_some_and(|path| {
                path.starts_with("vehicle_models/") && root().join(path).is_file()
            })
        })
        .map(|id| format!("{id} isn't a vehicle model in this catalog"))
        .collect()
}

#[test]
fn every_representative_vehicle_is_in_the_catalog() {
    let ids = representative_vehicle_ids();
    assert!(!ids.is_empty());
    assert_none("representative vehicles", missing(&ids));
}

#[test]
fn a_representative_vehicle_missing_from_the_catalog_is_reported() {
    let ids = [
        "m/porsche/2025/cross",
        "m/porsche/2025/cross@GB",
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

#[test]
fn the_representative_vehicles_are_in_alphabetical_order() {
    let ids = representative_vehicle_ids();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
}
