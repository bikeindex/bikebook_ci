mod bike_data;
mod comparison;
mod completeness;
mod index;
mod markets;
mod ruby;

use bike_data::BikeData;
use bikebook_data_checks::*;
use index::Row;
use markets::{Classifications, Markets};
use md5::{Digest, Md5};
use rayon::prelude::*;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The published files' layouts, oldest first, each with its own manifest. A new one is published beside the one before
/// it until every reader has moved to it: see the format-update skill
const FORMATS: [u64; 2] = [3, 4];
/// How long a file outlives the last manifest naming it, for a client still holding an older manifest
const RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

fn snapshot(json: &str) -> Value {
    serde_json::from_str(json).unwrap()
}

fn main() {
    let output = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("Usage: generate_catalog <output directory>"),
    );
    let catalog = Catalog::load();
    let schemas = Schemas::load();
    let schema = |name: &str| {
        let version = schemas
            .current_version(name)
            .unwrap_or_else(|| panic!("there's no {name} schema"));
        (
            schemas.documents[&Schemas::id(name, &version)].clone(),
            version,
        )
    };
    let (vocabulary_schema, _) = schema("vocabulary");
    let (vehicle_schema, schema_version) = schema("vehicle_model");
    let enumerated = |field: &str| -> Vec<String> {
        let definition = &vehicle_schema["$defs"][field];
        let definition = definition["$ref"].as_str().map_or(definition, |reference| {
            &vocabulary_schema["$defs"][reference.rsplit('/').next().unwrap()]
        });
        definition["enum"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|slug| Some(slug.as_str()?.to_string()))
            .collect()
    };
    let bike_data = BikeData::fetch();
    println!("bike_data at {}", bike_data.commit);
    let activities = bike_data.primary_activities();
    let markets = Markets::load(&vocabulary_schema);
    let records: Map<String, Value> = catalog
        .files
        .iter()
        .filter(|(path, _)| path.starts_with("e_vehicle_classifications/"))
        .map(|(path, data)| {
            let record = data
                .as_object()
                .unwrap()
                .iter()
                .filter(|(key, _)| *key != "id");
            (
                Catalog::expected_id(path),
                Value::Object(
                    record
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect(),
                ),
            )
        })
        .collect();
    let classifications = Classifications::new(records.clone());

    let models = models(&catalog);
    let incomplete: BTreeMap<&String, Vec<String>> = models
        .par_iter()
        .map(|(id, model)| (id, completeness::messages(model)))
        .filter(|(_, messages)| !messages.is_empty())
        .collect();
    let stored: BTreeSet<String> = stored_paths(&output)
        .into_iter()
        .filter(|path| path != "retired.json")
        .collect();

    let rows: Vec<Row> = models
        .iter()
        .map(|(id, vehicle)| Row {
            id,
            vehicle,
            incomplete: incomplete.contains_key(id),
        })
        .collect();
    let index_snapshot = snapshot(include_str!("../../../catalog_snapshot/index.json"));
    let vocabulary_snapshot = snapshot(include_str!("../../../catalog_snapshot/vocabulary.json"));
    let families: BTreeMap<&str, &str> = activities
        .iter()
        .map(|activity| (&*activity.slug, &*activity.family))
        .collect();
    let bike_data_names = bike_data.names(&activities);
    let names: Map<String, Value> = vehicle_schema["$defs"]
        .as_object()
        .unwrap()
        .keys()
        .filter_map(|field| {
            let named = match bike_data_names.get(field.as_str()) {
                Some(source) => enumerated(field)
                    .into_iter()
                    .map(|slug| {
                        let name = source
                            .get(&slug)
                            .unwrap_or_else(|| panic!("bike_data names no {field} {slug}"));
                        (slug, json!(name))
                    })
                    .collect(),
                None => vocabulary_snapshot["names"].get(field)?.clone(),
            };
            Some((field.clone(), named))
        })
        .collect();
    let mut kit = snapshot(include_str!("../../../catalog_snapshot/kit.json"));
    let components = enumerated("components.type");
    kit["component_groups"] = json!(bike_data.component_groups(
        &kit["component_groups"],
        &components.iter().map(String::as_str).collect::<Vec<_>>()
    ));
    let comparisons = comparison::with_rider_heights(
        rows.par_iter()
            .map(|row| {
                let family = row.vehicle["primary_activity"]
                    .as_str()
                    .and_then(|activity| families.get(activity));
                (
                    family.map(|family| family.to_string()),
                    comparison::call(row.vehicle, &classifications),
                    comparison::rider_height_sizes(row.vehicle),
                )
            })
            .collect(),
    );
    let index_rows: Vec<Value> = rows
        .par_iter()
        .map(|row| row.to_json(&markets, &classifications))
        .collect();
    let variants = market_variants(&catalog);
    let published_at = humantime::format_rfc3339_seconds(SystemTime::now()).to_string();
    let referenced: BTreeSet<String> = FORMATS
        .iter()
        .flat_map(|&format| {
            let blocks = write_blocks(&output, &stored, format, &models, &incomplete);
            let (index_models, listed_comparisons) =
                listed(format, &index_rows, &comparisons, &variants);
            let index = json!({
                "format": format,
                "manufacturers": manufacturers(&rows),
                "primary_activities": activities.iter().map(|activity| json!({"slug": activity.slug, "name": activity.name, "family": activity.family})).collect::<Vec<_>>(),
                "vehicle_types": names["type"].as_object().unwrap().iter().map(|(slug, name)| json!({"slug": slug, "name": name})).collect::<Vec<_>>(),
                "filter_options": index_snapshot["filter_options"],
                "stock_photos": index::PHOTO_URL,
                "models": index_models,
            });
            let vocabulary = json!({
                "format": format,
                "names": names,
                "e_vehicle_classifications": records,
                "currencies": vocabulary_snapshot["currencies"],
                "wheel_sizes": bike_data.wheel_sizes(),
            });
            let comparison_fields: Map<String, Value> = comparison::FIELDS
                .iter()
                .map(|field| {
                    (
                        field.to_string(),
                        listed_comparisons
                            .iter()
                            .map(|each| each.get(*field).cloned().unwrap_or(Value::Null))
                            .collect(),
                    )
                })
                .collect();
            let files: Vec<(&str, String)> = [
                ("index", index),
                ("vocabulary", vocabulary),
                ("kit", json!({"format": format, "kit": kit})),
                (
                    "comparison",
                    json!({"format": format, "fields": comparison_fields}),
                ),
            ]
            .into_iter()
            .map(|(name, body)| (name, write_hashed(&output, &stored, name, &body)))
            .collect();
            let mut manifest = json!({"format": format, "published_at": published_at, "schema_version": schema_version});
            files
                .iter()
                .for_each(|(name, path)| manifest[name] = json!(path));
            manifest["blocks"] = json!(blocks);
            let manifests = manifest_paths(format);
            manifests
                .iter()
                .for_each(|path| write(&output, path, &manifest));
            files
                .into_iter()
                .map(|(_, path)| path)
                .chain(blocks.into_values())
                .chain(manifests)
                .collect::<Vec<_>>()
        })
        .collect();

    retire(
        &output,
        stored
            .iter()
            .filter(|path| !referenced.contains(*path))
            .collect(),
    );
    println!(
        "{} models in {} blocks, in formats {FORMATS:?}, to {}",
        models.len(),
        blocks(&models).len(),
        output.display()
    );
}

/// Format 3's readers read `manifest.json`, from before each format had its own
fn manifest_paths(format: u64) -> Vec<String> {
    let own = format!("manifest.{format}.json");
    if format == 3 {
        vec![own, "manifest.json".to_string()]
    } else {
        vec![own]
    }
}

/// Each model's blocks, by `<maker>/<first year>`, named by their content
fn write_blocks(
    output: &Path,
    stored: &BTreeSet<String>,
    format: u64,
    models: &BTreeMap<String, Value>,
    incomplete: &BTreeMap<&String, Vec<String>>,
) -> BTreeMap<String, String> {
    blocks(models)
        .into_par_iter()
        .map(|(block, ids)| {
            let block_models: Map<String, Value> = ids
                .iter()
                .map(|id| (id.to_string(), jsonb_order(&models[*id])))
                .collect();
            let block_incomplete: Map<String, Value> = ids
                .iter()
                .filter_map(|id| Some((id.to_string(), json!(incomplete.get(id)?))))
                .collect();
            let body =
                json!({"format": format, "models": block_models, "incomplete": block_incomplete});
            let models_text = ids.iter().fold(format.to_string(), |text, id| {
                text + id.as_str() + &jsonb_text(&models[*id])
            });
            let path = format!("models/{block}.{}.json", digest(&models_text));
            write_new(output, stored, &path, &body.to_string());
            (block, path)
        })
        .collect()
}

/// Each base's market variants: the overlays over it that state their own markets
fn market_variants(catalog: &Catalog) -> BTreeMap<String, Vec<String>> {
    catalog
        .files
        .iter()
        .filter(|(path, data)| {
            path.starts_with("vehicle_models/") && !ruby::blank(data.get("markets"))
        })
        .filter_map(|(path, data)| {
            Some((
                data.get("base")?.as_str()?.to_string(),
                Catalog::expected_id(path),
            ))
        })
        .fold(BTreeMap::new(), |mut variants, (base, id)| {
            variants.entry(base).or_insert_with(Vec::new).push(id);
            variants
        })
}

/// The index's models, and their comparisons in step with them. From format 4 a market variant isn't a row of its own
/// but one of its base's `variants`, so a lookup can pick the one sold in the registrant's country
fn listed<'a>(
    format: u64,
    rows: &[Value],
    comparisons: &'a [Map<String, Value>],
    variants: &BTreeMap<String, Vec<String>>,
) -> (Vec<Value>, Vec<&'a Map<String, Value>>) {
    if format < 4 {
        return (rows.to_vec(), comparisons.iter().collect());
    }
    let markets: BTreeMap<&str, &Value> = rows
        .iter()
        .map(|row| (row["id"].as_str().unwrap(), &row["markets"]))
        .collect();
    let variant_ids: BTreeSet<&str> = variants.values().flatten().map(String::as_str).collect();
    rows.iter()
        .zip(comparisons)
        .filter(|(row, _)| !variant_ids.contains(row["id"].as_str().unwrap()))
        .map(|(row, comparison)| {
            let listed: Vec<Value> = variants
                .get(row["id"].as_str().unwrap())
                .into_iter()
                .flatten()
                .map(|id| json!({"id": id, "markets": markets[id.as_str()]}))
                .collect();
            let mut row = row.clone();
            row["variants"] = json!(listed);
            (row, comparison)
        })
        .unzip()
}

/// Every vehicle model by id, an overlay resolved over its base
fn models(catalog: &Catalog) -> BTreeMap<String, Value> {
    catalog
        .files
        .par_iter()
        .filter(|(path, _)| path.starts_with("vehicle_models/"))
        .map(|(path, data)| {
            let resolved = catalog
                .resolve(data)
                .unwrap_or_else(|error| panic!("{path}: {error}"));
            (Catalog::expected_id(path), resolved)
        })
        .collect()
}

/// The models' ids by `<maker>/<first year>`, the directories their files are in
fn blocks(models: &BTreeMap<String, Value>) -> BTreeMap<String, Vec<&String>> {
    models.keys().fold(BTreeMap::new(), |mut blocks, id| {
        let block = id
            .strip_prefix("m/")
            .unwrap_or(id)
            .rsplit_once('/')
            .unwrap()
            .0;
        blocks
            .entry(block.to_string())
            .or_insert_with(Vec::new)
            .push(id);
        blocks
    })
}

/// Each maker's name, ordered by it
fn manufacturers(rows: &[Row]) -> Map<String, Value> {
    let names: BTreeMap<&str, &Value> = rows
        .iter()
        .map(|row| (row.manufacturer(), &row.vehicle["manufacturer"]))
        .collect();
    let mut sorted: Vec<(&str, &Value)> = names.into_iter().collect();
    sorted.sort_by(|left, right| left.1.as_str().cmp(&right.1.as_str()));
    sorted
        .into_iter()
        .map(|(slug, name)| (slug.to_string(), name.clone()))
        .collect()
}

/// Postgres's jsonb key order, which the models were published in: shorter keys first, then bytewise
fn jsonb_order(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|left, right| (left.0.len(), left.0).cmp(&(right.0.len(), right.0)));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key.clone(), jsonb_order(value)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(jsonb_order).collect()),
        other => other.clone(),
    }
}

/// Postgres's jsonb text, which a block's name hashes, as the catalog's first publisher named them
fn jsonb_text(value: &Value) -> String {
    match value {
        Value::String(string) => {
            let escaped: String = string
                .chars()
                .map(|character| match character {
                    '"' => "\\\"".to_string(),
                    '\\' => "\\\\".to_string(),
                    '\u{8}' => "\\b".to_string(),
                    '\u{c}' => "\\f".to_string(),
                    '\n' => "\\n".to_string(),
                    '\r' => "\\r".to_string(),
                    '\t' => "\\t".to_string(),
                    control if control < ' ' => format!("\\u{:04x}", control as u32),
                    other => other.to_string(),
                })
                .collect();
            format!("\"{escaped}\"")
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(jsonb_text).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(_) => {
            let Value::Object(ordered) = jsonb_order(value) else {
                unreachable!()
            };
            let entries: Vec<String> = ordered
                .iter()
                .map(|(key, value)| {
                    format!(
                        "{}: {}",
                        jsonb_text(&Value::from(key.as_str())),
                        jsonb_text(value)
                    )
                })
                .collect();
            format!("{{{}}}", entries.join(", "))
        }
        other => other.to_string(),
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

/// Named by its content, so a client caches it for good
fn write_hashed(root: &Path, stored: &BTreeSet<String>, name: &str, body: &Value) -> String {
    let text = body.to_string();
    let path = format!("{name}.{}.json", digest(&text));
    write_new(root, stored, &path, &text);
    path
}

fn digest(text: &str) -> String {
    Md5::digest(text)
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A previous run's file under the same hashed name holds the same
fn write_new(root: &Path, stored: &BTreeSet<String>, path: &str, text: &str) {
    if !stored.contains(path) {
        write_text(root, path, text);
    }
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
    let (expired, kept): (Vec<_>, Vec<_>) = unreferenced
        .into_iter()
        .map(|path| {
            let at = since
                .get(path.as_str())
                .and_then(Value::as_str)
                .and_then(|at| humantime::parse_rfc3339(at).ok())
                .unwrap_or(now);
            (path, at)
        })
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
    fn jsonb_order_puts_shorter_keys_first_at_every_depth() {
        let ordered = jsonb_order(&json!({"bb": 1, "a": [{"zz": 1, "c": 2}], "ab": 3}));
        assert_eq!(
            ordered.to_string(),
            r#"{"a":[{"c":2,"zz":1}],"ab":3,"bb":1}"#
        );
    }

    #[test]
    fn jsonb_text_spaces_its_separators_and_escapes_controls() {
        let value = json!({"bb": [1, 2.5], "a": "x\"\u{1}\n", "c": {}});
        assert_eq!(
            jsonb_text(&value),
            r#"{"a": "x\"\u0001\n", "c": {}, "bb": [1, 2.5]}"#
        );
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
    fn a_market_variant_is_an_overlay_stating_its_markets() {
        let catalog = Catalog {
            files: BTreeMap::from([
                (
                    "vehicle_models/acme/2025/one_eu.json".to_string(),
                    json!({"base": "m/acme/2025/one", "markets": ["EU"]}),
                ),
                (
                    "vehicle_models/acme/2025/one_red.json".to_string(),
                    json!({"base": "m/acme/2025/one"}),
                ),
                (
                    "vehicle_models/acme/2025/two_gb.json".to_string(),
                    json!({"markets": ["GB"]}),
                ),
            ]),
        };
        assert_eq!(
            market_variants(&catalog),
            BTreeMap::from([(
                "m/acme/2025/one".to_string(),
                vec!["m/acme/2025/one_eu".to_string()]
            )])
        );
    }

    #[test]
    fn listed_puts_a_market_variant_on_its_base_from_format_4() {
        let rows = [
            json!({"id": "a", "markets": ["US"]}),
            json!({"id": "a_eu", "markets": ["EU"]}),
            json!({"id": "b", "markets": ["US"]}),
        ];
        let comparisons: Vec<Map<String, Value>> = ["a", "a_eu", "b"]
            .iter()
            .map(|id| json!({"of": id}).as_object().unwrap().clone())
            .collect();
        let variants = BTreeMap::from([("a".to_string(), vec!["a_eu".to_string()])]);
        let (legacy, legacy_comparisons) = listed(3, &rows, &comparisons, &variants);
        assert_eq!(legacy, rows);
        assert_eq!(legacy_comparisons.len(), 3);
        let (listed, listed_comparisons) = listed(4, &rows, &comparisons, &variants);
        assert_eq!(
            listed,
            [
                json!({"id": "a", "markets": ["US"], "variants": [{"id": "a_eu", "markets": ["EU"]}]}),
                json!({"id": "b", "markets": ["US"], "variants": []}),
            ]
        );
        assert_eq!(
            listed_comparisons
                .iter()
                .map(|comparison| &comparison["of"])
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
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
