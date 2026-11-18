use bikebook_data_checks::*;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The published files' layout, versioned apart from the schemas: raise it whenever that layout changes, which also
/// gives every file a new name
const FORMAT: u64 = 1;
const PHOTO_URL: &str = "https://bikebook.bikeindex.org";
/// How long a file outlives the last manifest naming it, for a client still holding an older manifest
const RETENTION: Duration = Duration::from_secs(24 * 60 * 60);
const ELECTRIC_PROPULSIONS: [&str; 3] = ["pedal_assist", "throttle", "pedal_assist_and_throttle"];

fn main() {
    let output = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("Usage: generate_catalog <output directory>"),
    );
    let catalog = Catalog::load();
    let models = models(&catalog);
    let stored: BTreeSet<String> = stored_paths(&output)
        .into_iter()
        .filter(|path| path != "manifest.json" && path != "retired.json")
        .collect();

    let blocks: BTreeMap<String, String> = blocks(&models)
        .into_iter()
        .map(|(block, block_models)| {
            let body = json!({"format": FORMAT, "models": block_models});
            let path = write_hashed(&output, &stored, &format!("models/{block}"), &body);
            (block, path)
        })
        .collect();
    let classifications: Map<String, Value> = catalog
        .files
        .iter()
        .filter(|(path, _)| path.starts_with("e_vehicle_classifications/"))
        .map(|(path, data)| (Catalog::expected_id(path), data.clone()))
        .collect();
    let files: BTreeMap<&str, String> = [
        ("index", index(&models)),
        (
            "vocabulary",
            json!({"format": FORMAT, "e_vehicle_classifications": classifications}),
        ),
    ]
    .into_iter()
    .map(|(name, body)| (name, write_hashed(&output, &stored, name, &body)))
    .collect();

    let schema_version = Schemas::load()
        .current_version("vehicle_model")
        .expect("there's no vehicle_model schema");
    let published_at = humantime::format_rfc3339_seconds(SystemTime::now()).to_string();
    let mut manifest =
        json!({"format": FORMAT, "published_at": published_at, "schema_version": schema_version});
    files
        .iter()
        .for_each(|(name, path)| manifest[name] = json!(path));
    manifest["blocks"] = json!(blocks);
    write(&output, "manifest.json", &manifest);

    let referenced: BTreeSet<&String> = files.values().chain(blocks.values()).collect();
    let unreferenced = stored
        .iter()
        .filter(|path| !referenced.contains(path))
        .collect();
    retire(&output, unreferenced);
    println!(
        "{} models in {} blocks, to {}",
        models.len(),
        blocks.len(),
        output.display()
    );
}

/// Every vehicle model by id, an overlay resolved over its base
fn models(catalog: &Catalog) -> BTreeMap<String, Value> {
    catalog
        .files
        .iter()
        .filter(|(path, _)| path.starts_with("vehicle_models/"))
        .map(|(path, data)| {
            let resolved = catalog
                .resolve(data)
                .unwrap_or_else(|error| panic!("{path}: {error}"));
            (Catalog::expected_id(path), resolved)
        })
        .collect()
}

/// The models by `<maker>/<first year>`, the directories their files are in
fn blocks(models: &BTreeMap<String, Value>) -> BTreeMap<String, Map<String, Value>> {
    models
        .iter()
        .fold(BTreeMap::new(), |mut blocks, (id, model)| {
            let block = unprefixed(id).rsplit_once('/').unwrap().0;
            blocks
                .entry(block.to_string())
                .or_insert_with(Map::new)
                .insert(id.clone(), model.clone());
            blocks
        })
}

fn index(models: &BTreeMap<String, Value>) -> Value {
    let manufacturers: BTreeMap<String, &Value> = models
        .iter()
        .map(|(id, model)| (manufacturer_slug(id).to_string(), &model["manufacturer"]))
        .collect();
    json!({
        "format": FORMAT,
        "manufacturers": manufacturers,
        "stock_photos": PHOTO_URL,
        "models": models.iter().map(|(id, model)| index_row(id, model)).collect::<Vec<_>>(),
    })
}

fn index_row(id: &str, model: &Value) -> Value {
    let price = year_records(model)
        .into_iter()
        .filter(|(_, record)| {
            record
                .get("original_msrp")
                .is_some_and(|msrp| !msrp.is_null())
        })
        .max_by_key(|(year, _)| *year)
        .map(|(_, record)| record);
    let photo = model["stock_photo"].as_str().map(|url| {
        url.strip_prefix(&format!("{PHOTO_URL}/{}.", unprefixed(id)))
            .unwrap_or(url)
    });
    let classifications: BTreeSet<&str> = model["motors"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|motor| motor["operating_modes"].as_array().into_iter().flatten())
        .flat_map(|mode| {
            mode["e_vehicle_classifications"]
                .as_array()
                .into_iter()
                .flatten()
        })
        .filter_map(Value::as_str)
        .collect();
    json!({
        "id": id,
        "manufacturer": manufacturer_slug(id),
        "model": model["model"],
        "display": display(model),
        "first_year": model["first_year"],
        "final_year": model["final_year"],
        "primary_activity": model["primary_activity"],
        "vehicle_type": model["type"],
        "model_configuration": model["model_configuration"],
        "electric": electric(model),
        "e_vehicle_classifications": classifications,
        "suspension": suspension(model),
        "markets": model["markets"],
        "msrp_cents": price.map_or(&Value::Null, |record| &record["original_msrp"]),
        "msrp_currency": price.map_or(&Value::Null, |record| &record["original_msrp_currency"]),
        "manufacturer_name": model["manufacturer"],
        "model_group": model["model_group"],
        "handlebar_type": model["handlebar_type"],
        "photo": photo,
    })
}

fn unprefixed(id: &str) -> &str {
    id.strip_prefix("m/").unwrap_or(id)
}

fn manufacturer_slug(id: &str) -> &str {
    id.split('/').nth(1).unwrap_or_default()
}

/// Each year's record with its year, from a map keyed by year or an array of records naming theirs
fn year_records(model: &Value) -> Vec<(i64, &Value)> {
    match &model["years"] {
        Value::Object(years) => years
            .iter()
            .map(|(year, record)| (year.parse().unwrap_or_default(), record))
            .collect(),
        Value::Array(records) => records
            .iter()
            .map(|record| (record["year"].as_i64().unwrap_or_default(), record))
            .collect(),
        _ => vec![],
    }
}

fn display(model: &Value) -> String {
    let name = [&model["manufacturer"], &model["model"]]
        .into_iter()
        .filter_map(Value::as_str)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    match (&model["final_year"], &model["first_year"]) {
        (Value::Number(year), _) => format!("{name} ({year})"),
        (_, Value::Number(_)) => format!("{name} (current)"),
        _ => name,
    }
}

/// null when the model states no propulsion
fn electric(model: &Value) -> Option<bool> {
    let propulsion = model["propulsion"]
        .as_array()
        .filter(|items| !items.is_empty())?;
    Some(propulsion.iter().any(|item| {
        item.as_str()
            .is_some_and(|item| ELECTRIC_PROPULSIONS.contains(&item))
    }))
}

/// A frameset has no fork, so its rear shock makes it a full suspension frame
fn suspension(model: &Value) -> &'static str {
    let ends = model["suspension"].as_object();
    let front = ends.is_some_and(|ends| ends.contains_key("front"));
    let rear = ends.is_some_and(|ends| ends.contains_key("rear"));
    match (
        front || (rear && model["model_configuration"] == "frame"),
        rear,
    ) {
        (true, true) => "full",
        (true, false) => "hardtail",
        (false, true) => "rear",
        (false, false) => "rigid",
    }
}

fn stored_paths(root: &Path) -> Vec<String> {
    fn walk(directory: &Path, root: &Path) -> Vec<String> {
        fs::read_dir(directory)
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(|entry| {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, root)
                } else {
                    vec![
                        path.strip_prefix(root)
                            .unwrap()
                            .to_string_lossy()
                            .into_owned(),
                    ]
                }
            })
            .collect()
    }
    walk(root, root)
}

/// Named by its content, so a client caches it for good, and left alone when a previous run wrote it
fn write_hashed(root: &Path, stored: &BTreeSet<String>, name: &str, body: &Value) -> String {
    let text = body.to_string();
    let digest: String = Sha256::digest(&text)
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let path = format!("{name}.{digest}.json");
    if !stored.contains(&path) {
        write_text(root, &path, &text);
    }
    path
}

fn write(root: &Path, path: &str, body: &Value) {
    write_text(root, path, &body.to_string())
}

/// Renamed into place, since a later run trusts any file already under a hashed name
fn write_text(root: &Path, path: &str, text: &str) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    let temporary = file.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temporary, text).unwrap();
    fs::rename(temporary, file).unwrap();
}

/// Deletes a file once it's gone unreferenced for RETENTION, noting when each unreferenced one went
fn retire(root: &Path, unreferenced: BTreeSet<&String>) {
    let now = SystemTime::now();
    let since: Map<String, Value> = fs::read_to_string(root.join("retired.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let retired: BTreeMap<&String, SystemTime> = unreferenced
        .into_iter()
        .map(|path| {
            let at = since
                .get(path.as_str())
                .and_then(Value::as_str)
                .and_then(|at| humantime::parse_rfc3339(at).ok())
                .unwrap_or(now);
            (path, at)
        })
        .collect();
    let (expired, kept): (Vec<_>, Vec<_>) = retired
        .into_iter()
        .partition(|(_, at)| now.duration_since(*at).unwrap_or_default() > RETENTION);
    expired
        .iter()
        .for_each(|(path, _)| fs::remove_file(root.join(path)).unwrap());
    let kept: Map<String, Value> = kept
        .into_iter()
        .map(|(path, at)| {
            (
                path.clone(),
                json!(humantime::format_rfc3339_seconds(at).to_string()),
            )
        })
        .collect();
    write(root, "retired.json", &Value::Object(kept));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suspension_names_the_ends() {
        let cases = [
            (json!({}), "rigid"),
            (json!({"suspension": {"front": {}}}), "hardtail"),
            (json!({"suspension": {"rear": {}}}), "rear"),
            (json!({"suspension": {"front": {}, "rear": {}}}), "full"),
            (
                json!({"suspension": {"rear": {}}, "model_configuration": "frame"}),
                "full",
            ),
        ];
        for (model, expected) in cases {
            assert_eq!(suspension(&model), expected, "{model}");
        }
    }

    #[test]
    fn display_names_the_final_year_or_current() {
        let model = json!({"manufacturer": "Trek", "model": "Fuel+", "first_year": 2024});
        assert_eq!(display(&model), "Trek Fuel+ (current)");
        let ended = json!({"manufacturer": "Trek", "model": "Fuel+", "first_year": 2024, "final_year": 2025});
        assert_eq!(display(&ended), "Trek Fuel+ (2025)");
    }

    #[test]
    fn electric_is_null_without_propulsion() {
        assert_eq!(electric(&json!({})), None);
        assert_eq!(electric(&json!({"propulsion": ["human"]})), Some(false));
        assert_eq!(
            electric(&json!({"propulsion": ["human", "throttle"]})),
            Some(true)
        );
    }

    #[test]
    fn index_row_prices_the_latest_priced_year_and_keys_the_photo() {
        let model = json!({
            "stock_photo": "https://bikebook.bikeindex.org/trek/2024/fuel.png",
            "years": {"2024": {"original_msrp": 100, "original_msrp_currency": "USD"}, "2025": {"original_msrp": 200, "original_msrp_currency": "EUR"}, "2026": {}},
        });
        let row = index_row("m/trek/2024/fuel", &model);
        assert_eq!(
            (&row["msrp_cents"], &row["msrp_currency"]),
            (&json!(200), &json!("EUR"))
        );
        assert_eq!(row["photo"], "png");
        assert_eq!(row["manufacturer"], "trek");
    }

    #[test]
    fn blocks_group_by_maker_and_first_year() {
        let models = BTreeMap::from([
            ("m/trek/2024/a".to_string(), json!(1)),
            ("m/trek/2024/b".to_string(), json!(2)),
            ("m/trek/2025/c".to_string(), json!(3)),
        ]);
        let blocks = blocks(&models);
        assert_eq!(
            blocks.keys().collect::<Vec<_>>(),
            ["trek/2024", "trek/2025"]
        );
        assert_eq!(blocks["trek/2024"].len(), 2);
    }

    #[test]
    fn retire_deletes_only_what_has_been_unreferenced_past_retention() {
        let root = std::env::temp_dir().join(format!("generate_catalog_{}", std::process::id()));
        let long_ago =
            humantime::format_rfc3339_seconds(SystemTime::now() - RETENTION * 2).to_string();
        write(&root, "retired.json", &json!({"old.json": long_ago}));
        ["old.json", "recent.json"]
            .iter()
            .for_each(|path| write_text(&root, path, "{}"));
        let (old, recent) = ("old.json".to_string(), "recent.json".to_string());
        retire(&root, BTreeSet::from([&old, &recent]));
        assert!(!root.join("old.json").exists());
        assert!(root.join("recent.json").exists());
        let retired: Value =
            serde_json::from_str(&fs::read_to_string(root.join("retired.json")).unwrap()).unwrap();
        assert_eq!(
            retired.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["recent.json"]
        );
        fs::remove_dir_all(root).unwrap();
    }
}
