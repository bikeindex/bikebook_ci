use bikebook_data_checks::*;
use rayon::prelude::*;
use serde_json::{Value, json};

const PHOTO_URL: &str = "https://bikebook.bikeindex.org/";

fn photo_stem(url: &str) -> &str {
    let without = url.strip_prefix(PHOTO_URL).unwrap_or(url);
    without.rsplit_once('.').map_or(without, |(stem, _)| stem)
}

fn unprefixed(id: &str) -> &str {
    id.split_once('/').map_or(id, |(_, rest)| rest)
}

fn catalog_problems(catalog: &Catalog) -> Vec<String> {
    catalog
        .models()
        .collect::<Vec<_>>()
        .par_iter()
        .flat_map_iter(|&(path, data)| {
            file_problems(catalog, path, data)
                .into_iter()
                .map(move |problem| format!("{path}: {problem}"))
        })
        .collect()
}

fn file_problems(catalog: &Catalog, path: &str, data: &Value) -> Vec<String> {
    let mut problems = vec![];
    let id = data.get("id").and_then(Value::as_str).unwrap_or_default();
    if id != Catalog::expected_id(path) {
        problems.push(format!(
            "id {id} isn't its path's {}",
            Catalog::expected_id(path)
        ));
    }
    if Catalog::kind(path) == "e_vehicle_classification" {
        return problems;
    }

    let base = data.get("base").and_then(Value::as_str);
    if base.is_some() && data.get("geometry_photo").is_some() {
        problems.push("a market variant takes its base's geometry_photo".to_string());
    }
    let resolved = match catalog.resolve(data) {
        Ok(resolved) => resolved,
        Err(error) => {
            problems.push(error);
            return problems;
        }
    };

    let motors: Vec<&str> = match resolved.get("motors") {
        Some(Value::Array(motors)) => motors
            .iter()
            .filter_map(|motor| motor.get("id")?.as_str())
            .collect(),
        _ => vec![],
    };
    for motor in motors {
        if catalog.by_id(motor).is_none() {
            problems.push(format!("its motor {motor} isn't in this repo"));
        }
    }
    if Catalog::kind(path) != "vehicle_model" {
        return problems;
    }

    let year = id.split('/').nth(2).unwrap_or_default();
    let first_year = resolved
        .get("first_year")
        .map(|year| year.to_string())
        .unwrap_or_default();
    if first_year != year {
        problems.push(format!(
            "first_year {first_year} isn't the id's year {year}"
        ));
    }
    if !years(&resolved).contains(&first_year) {
        problems.push(format!("no years record for its model year {first_year}"));
    }

    // a photo is stored under its model's id, alone or suffixed with its market or one it, or a market variant's file, is
    // sold in; a colorway's under that and its paint
    let sold = [Some(&resolved), base.and_then(|base| catalog.by_id(base))]
        .into_iter()
        .flatten()
        .filter_map(|model| model.get("markets")?.as_array())
        .flatten()
        .filter_map(|market| Some(market.as_str()?.to_string()));
    let markets: Vec<String> = path
        .split_once('@')
        .map(|(_, market)| market.to_string())
        .into_iter()
        .chain(sold)
        .collect();
    let owners: Vec<String> = std::iter::once(None)
        .chain(markets.iter().map(Some))
        .map(|market| Catalog::photo_stem(unprefixed(id), market.map(String::as_str)))
        .collect();
    if let Some(photo) = resolved.get("stock_photo").and_then(Value::as_str)
        && !owners.iter().any(|owner| owner == photo_stem(photo))
    {
        problems.push(format!("stock_photo isn't keyed by the id: {photo}"));
    }
    if let Some(photo) = resolved.get("geometry_photo").and_then(Value::as_str)
        && !owners
            .iter()
            .any(|owner| photo_stem(photo) == format!("{owner}-geometry"))
    {
        problems.push(format!("geometry_photo isn't keyed by the id: {photo}"));
    }
    let mut found = vec![];
    strings(
        resolved.get("years").unwrap_or(&Value::Null),
        "/years".to_string(),
        &mut found,
    );
    for (at, _, photo) in found
        .iter()
        .filter(|(at, key, _)| key == "stock_photo" && at.contains("/colorways/"))
    {
        let stem = photo_stem(photo);
        if !owners
            .iter()
            .any(|owner| stem == *owner || stem.starts_with(&format!("{owner}-")))
        {
            problems.push(format!("{at} isn't keyed by the id: {photo}"));
        }
    }
    problems
}

#[test]
fn every_file_names_its_own_id_and_what_it_references() {
    assert_none("catalog", catalog_problems(&Catalog::load()));
}

#[test]
fn a_misplaced_or_dangling_file_is_reported() {
    let path = "vehicle_models/porsche/2025/cross.json";
    let model = &parsed(&[path])[path];
    let mut misplaced = model.clone();
    misplaced["id"] = "m/porsche/2024/cross".into();
    misplaced["first_year"] = 2027.into();
    misplaced["stock_photo"] = "https://bikebook.bikeindex.org/porsche/2025/sport.webp".into();
    misplaced["motors"] = json!([{"id": "c/nobody/2025/motor"}]);
    let problems = catalog_problems(&Catalog::new([(path.to_string(), misplaced)].into()));
    for expected in [
        "isn't its path's",
        "first_year 2027",
        "no years record",
        "stock_photo isn't keyed",
        "c/nobody/2025/motor",
    ] {
        assert!(
            problems.iter().any(|problem| problem.contains(expected)),
            "{expected}: {problems:?}"
        );
    }
}

#[test]
fn a_market_variant_is_checked_as_a_model_and_its_id_found() {
    let motor_path = "components/tq/2025/hpr60.json";
    let path = "vehicle_models/porsche/2025/cross.json";
    let mut files = parsed(&[motor_path, path]);
    files.get_mut(motor_path).unwrap()["market_variants"] = json!({"US": {"markets": ["US"]}});
    let model = files.get_mut(path).unwrap();
    model["motors"] = json!([{"id": "c/tq/2025/hpr60-us"}]);
    model["market_variants"] = json!({
        "GB": {"markets": ["GB"], "motors": {"0": {"id": "c/tq/2025/hpr60@US"}},
            "stock_photo": "https://bikebook.bikeindex.org/porsche/2025/cross-gb.webp"},
        "CA": {"markets": ["CA"], "motors": {"0": {"id": "c/tq/2025/hpr60@CA"}},
            "stock_photo": "https://bikebook.bikeindex.org/porsche/2025/cross-fr.webp"}});
    let problems = catalog_problems(&Catalog::new(files));
    assert_eq!(
        problems,
        [
            "m/porsche/2025/cross@CA: its motor c/tq/2025/hpr60@CA isn't in this repo".to_string(),
            "m/porsche/2025/cross@CA: stock_photo isn't keyed by the id: https://bikebook.bikeindex.org/porsche/2025/cross-fr.webp".to_string()
        ]
    );
}
