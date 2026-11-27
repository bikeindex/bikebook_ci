use bikebook_data_checks::*;
use rayon::prelude::*;
use serde_json::{Map, Value, json};

fn order_errors(schemas: &Schemas, path: &str, data: &Value) -> Vec<String> {
    match schemas.document_for(path, data) {
        Err(error) => vec![format!("{path}: {error}")],
        Ok(document) => schemas
            .key_order_errors(&document, data)
            .into_iter()
            .map(|error| format!("{path}: {error}"))
            .collect(),
    }
}

#[test]
fn every_file_is_in_schema_key_order() {
    let schemas = Schemas::load();
    let problems = Catalog::load()
        .files
        .par_iter()
        .flat_map_iter(|(path, data)| order_errors(&schemas, path, data))
        .collect();
    assert_none("key order", problems);
}

fn swapped(value: &Value, first: &str, second: &str) -> Value {
    let map = value.as_object().unwrap();
    let mut keys: Vec<&String> = map.keys().collect();
    let (a, b) = (
        keys.iter().position(|key| *key == first).unwrap(),
        keys.iter().position(|key| *key == second).unwrap(),
    );
    keys.swap(a, b);
    Value::Object(
        keys.into_iter()
            .map(|key| (key.clone(), map[key].clone()))
            .collect::<Map<_, _>>(),
    )
}

#[test]
fn a_reordered_file_is_reported() {
    let schemas = Schemas::load();
    let path = "vehicle_models/porsche/2025/cross.json";
    let model = &parsed(&[path])[path];
    assert_eq!(order_errors(&schemas, path, model), Vec::<String>::new());

    let top = swapped(model, "stock_photo", "geometry_photo");
    assert!(
        order_errors(&schemas, path, &top)
            .iter()
            .any(|error| error.contains("(root)"))
    );

    let mut nested = model.clone();
    let year = nested["years"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    let colorway = nested["years"][&year]["colorways"][0].clone();
    nested["years"][&year]["colorways"][0] = swapped(&colorway, "name", "stock_photo");
    assert!(
        order_errors(&schemas, path, &nested)
            .iter()
            .any(|error| error.contains(&format!("/years/{year}/colorways/0")))
    );

    let mut years = model.clone();
    let mut reversed: Vec<(String, Value)> = years["years"]
        .as_object()
        .unwrap()
        .clone()
        .into_iter()
        .collect();
    reversed.push(("1999".to_string(), reversed[0].1.clone()));
    reversed.reverse();
    years["years"] = Value::Object(reversed.into_iter().collect());
    assert!(
        order_errors(&schemas, path, &years)
            .iter()
            .any(|error| error.contains("/years: keys must be ordered"))
    );
}

#[test]
fn a_market_variant_is_ordered_as_an_overlay() {
    let schemas = Schemas::load();
    let path = "vehicle_models/porsche/2025/cross.json";
    let mut model = parsed(&[path])[path].clone();
    model["market_variants"] =
        json!({"CA": {"model": "Cross CA", "frame": false, "markets": ["CA"]}});
    assert_eq!(order_errors(&schemas, path, &model), Vec::<String>::new());
    model["market_variants"] = json!({"CA": {"markets": ["CA"], "model": "Cross CA"}});
    assert!(
        order_errors(&schemas, path, &model)
            .iter()
            .any(|error| error.contains("/market_variants/CA: keys must be ordered"))
    );
}

#[test]
fn a_reordered_file_is_put_back_in_order() {
    let schemas = Schemas::load();
    let path = "vehicle_models/porsche/2025/cross.json";
    let model = &parsed(&[path])[path];
    let document = schemas.document_for(path, model).unwrap();
    let mut reordered = swapped(model, "stock_photo", "geometry_photo");
    let year = model["years"].as_object().unwrap().keys().next().unwrap();
    let colorway = &model["years"][year]["colorways"][0];
    reordered["years"][year]["colorways"][0] = swapped(colorway, "name", "stock_photo");
    assert_eq!(
        pretty(&schemas.in_key_order(&document, &reordered)),
        pretty(model)
    );
}

#[test]
fn a_market_variant_removal_stays_in_place() {
    let schemas = Schemas::load();
    let path = "vehicle_models/porsche/2025/cross.json";
    let mut model = parsed(&[path])[path].clone();
    let document = schemas.document_for(path, &model).unwrap();
    model["market_variants"] =
        json!({"CA": {"markets": ["CA"], "frame": false, "model": "Cross CA"}});
    let ordered = schemas.in_key_order(&document, &model);
    assert_eq!(
        pretty(&ordered["market_variants"]["CA"]),
        pretty(&json!({"model": "Cross CA", "frame": false, "markets": ["CA"]}))
    );
}

#[test]
fn an_object_its_schema_names_no_keys_of_is_sorted() {
    let schemas = Schemas::load();
    let path = "vehicle_models/porsche/2025/cross.json";
    let mut model = parsed(&[path])[path].clone();
    model["sizes"][0]["geometry"]["extra_measurements"] =
        json!({"seat_post": 300, "chainstay_length": 430});
    assert!(
        order_errors(&schemas, path, &model)
            .iter()
            .any(|error| error
                .contains("/sizes/0/geometry/extra_measurements: keys must be ordered"))
    );
}

#[test]
fn a_market_variant_false_that_isnt_a_removal_is_ordered() {
    let schemas = Schemas::load();
    let path = "vehicle_models/porsche/2025/cross.json";
    let mut model = parsed(&[path])[path].clone();
    model["market_variants"] = json!({
        "CA": {"markets": ["CA"], "suspension": {"front": {"lockout": false, "travel": 120}}}
    });
    assert!(
        order_errors(&schemas, path, &model)
            .iter()
            .any(|error| error
                .contains("/market_variants/CA/suspension/front: keys must be ordered"))
    );
}
