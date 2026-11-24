mod bike_data;
mod comparison;
mod completeness;
mod index;
mod markets;
mod ruby;
mod schema_viewer;

use bike_data::BikeData;
use bikebook_data_checks::*;
use flate2::{Compression, write::GzEncoder};
use index::Row;
use markets::{Classifications, Markets};
use md5::{Digest, Md5};
use rayon::prelude::*;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The published files' layouts, oldest first, each with its own manifest. A new one is published beside the one before
/// it until every reader has moved to it: see the format-update skill
const FORMATS: [u64; 2] = [5, 6];
/// How long a file outlives the last manifest naming it, for a client still holding an older manifest
const RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

fn snapshot(json: &str) -> Value {
    serde_json::from_str(json).unwrap()
}

fn with(object: &Value, added: impl IntoIterator<Item = (String, Value)>) -> Value {
    object
        .as_object()
        .unwrap()
        .clone()
        .into_iter()
        .chain(added)
        .collect()
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let output = PathBuf::from(
        arguments
            .first()
            .expect("Usage: generate_catalog <output directory> [--schema-viewer]"),
    );
    if arguments
        .get(1)
        .is_some_and(|argument| argument == "--schema-viewer")
    {
        return schema_viewer::publish(&output);
    }
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

    let all = models(&catalog);
    let stored: BTreeSet<String> = stored_paths(&output)
        .into_iter()
        .filter(|path| path != "retired.json")
        .collect();

    let index_snapshot = snapshot(include_str!("../../../catalog_snapshot/index.json"));
    let vocabulary_snapshot = snapshot(include_str!("../../../catalog_snapshot/vocabulary.json"));
    let families: BTreeMap<String, &str> = activities
        .iter()
        .map(|activity| (activity.slug.clone(), &*activity.family))
        .collect();
    let bike_data_names = bike_data.names(&activities);
    let names: Map<String, Value> = vehicle_schema["$defs"]
        .as_object()
        .unwrap()
        .keys()
        .filter_map(|field| {
            let named: Value = match bike_data_names.get(field.as_str()) {
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
    let kit = json!({"kit": kit});
    let variants = market_variants(&catalog);
    let published_at = humantime::format_rfc3339_seconds(SystemTime::now()).to_string();
    let incomplete: BTreeMap<&String, Vec<String>> = all
        .par_iter()
        .map(|(id, model)| (id, completeness::messages(model)))
        .filter(|(_, messages)| !messages.is_empty())
        .collect();
    let rows: Vec<Row> = all
        .iter()
        .map(|(id, vehicle)| Row {
            id,
            vehicle,
            incomplete: incomplete.contains_key(id),
        })
        .collect();
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
    let (index_models, listed_comparisons) = listed(&index_rows, &comparisons, &variants);
    let index = json!({
        "manufacturers": manufacturers(&rows),
        "primary_activities": activities.iter().map(|activity| json!({"slug": activity.slug, "name": activity.name, "family": activity.family})).collect::<Vec<_>>(),
        "vehicle_types": names["type"].as_object().unwrap().iter().map(|(slug, name)| json!({"slug": slug, "name": name})).collect::<Vec<_>>(),
        "filter_options": index_snapshot["filter_options"],
        "stock_photos": index::PHOTO_URL,
        "models": index_models,
    });
    let vocabulary = json!({
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
    let comparison = json!({"fields": comparison_fields});
    let block_bodies = block_bodies(&all, &incomplete);
    let referenced: BTreeSet<String> = FORMATS
        .iter()
        .flat_map(|&format| {
            let blocks = write_blocks(&output, format, &block_bodies);
            let written: Vec<(&str, String)> = files(format, &index, &vocabulary, &kit, &comparison)
                .into_iter()
                .map(|(name, parts)| {
                    let fields = parts.into_iter().flat_map(|part| part.as_object().unwrap().clone());
                    (name, write_hashed(&output, name, &with(&json!({"format": format}), fields)))
                })
                .chain(schema_viewer::published(&output, format).map(|path| ("schema_viewer", path)))
                .collect();
            let mut manifest = json!({"format": format, "published_at": published_at, "schema_version": schema_version});
            written
                .iter()
                .for_each(|(name, path)| manifest[name] = json!(path));
            manifest["blocks"] = json!(blocks);
            let manifest_path = format!("manifest.{format}.json");
            write(&output, &manifest_path, &gzip(&manifest.to_string()));
            written
                .into_iter()
                .map(|(_, path)| path)
                .chain(blocks.into_values())
                .chain([manifest_path])
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
        all.len(),
        blocks(&all).len(),
        output.display()
    );
}

/// The files a manifest names but its blocks and schema viewer, each as the objects whose fields it holds. From format
/// 6 the vocabulary holds the kit
fn files<'a>(
    format: u64,
    index: &'a Value,
    vocabulary: &'a Value,
    kit: &'a Value,
    comparison: &'a Value,
) -> Vec<(&'static str, Vec<&'a Value>)> {
    let vocabulary = if format < 6 {
        vec![("vocabulary", vec![vocabulary]), ("kit", vec![kit])]
    } else {
        vec![("vocabulary", vec![vocabulary, kit])]
    };
    [("index", vec![index])]
        .into_iter()
        .chain(vocabulary)
        .chain([("comparison", vec![comparison])])
        .collect()
}

/// Each block, by `<maker>/<first year>`: its models as published, their `incomplete` messages, and the text its name
/// hashes, less the format
fn block_bodies(
    models: &BTreeMap<String, Value>,
    incomplete: &BTreeMap<&String, Vec<String>>,
) -> BTreeMap<String, (Value, Value, String)> {
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
            let models_text: String = ids
                .iter()
                .map(|id| id.to_string() + &jsonb_text(&models[*id]))
                .collect();
            (
                block,
                (json!(block_models), json!(block_incomplete), models_text),
            )
        })
        .collect()
}

/// Each block's file, named by its content
fn write_blocks(
    output: &Path,
    format: u64,
    bodies: &BTreeMap<String, (Value, Value, String)>,
) -> BTreeMap<String, String> {
    bodies
        .par_iter()
        .map(|(block, (models, incomplete, models_text))| {
            let body = json!({"format": format, "models": models, "incomplete": incomplete});
            let path = format!(
                "models/{block}.{}.json",
                digest(&format!("{format}{models_text}"))
            );
            write_new(output, &path, &body.to_string());
            (block.clone(), path)
        })
        .collect()
}

/// Each vehicle model's `market_variants` that state their own markets, by its id
fn market_variants(catalog: &Catalog) -> BTreeMap<String, Vec<String>> {
    catalog
        .variants
        .iter()
        .filter(|(key, data)| {
            Catalog::kind(key) == "vehicle_model" && !ruby::blank(data.get("markets"))
        })
        .fold(BTreeMap::new(), |mut variants, (key, _)| {
            let (id, _) = key.split_once('@').unwrap();
            variants
                .entry(id.to_string())
                .or_insert_with(Vec::new)
                .push(key.clone());
            variants
        })
}

/// The index's models, and their comparisons in step with them. A market variant isn't a row of its own but one of its
/// base's `variants`, so a lookup can pick the one sold in the registrant's country
fn listed<'a>(
    rows: &[Value],
    comparisons: &'a [Map<String, Value>],
    variants: &BTreeMap<String, Vec<String>>,
) -> (Vec<Value>, Vec<&'a Map<String, Value>>) {
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

/// Every vehicle model by citation key, a market variant resolved over its file
fn models(catalog: &Catalog) -> BTreeMap<String, Value> {
    catalog
        .models()
        .collect::<Vec<_>>()
        .into_par_iter()
        .filter(|(path, _)| Catalog::kind(path) == "vehicle_model")
        .map(|(path, data)| {
            let resolved = catalog
                .resolve(data)
                .unwrap_or_else(|error| panic!("{path}: {error}"));
            (Catalog::citation_key(path), resolved)
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
fn write_hashed(root: &Path, name: &str, body: &Value) -> String {
    let text = body.to_string();
    let path = format!("{name}.{}.json", digest(&text));
    write_new(root, &path, &text);
    path
}

fn digest(text: &str) -> String {
    Md5::digest(text)
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A previous run's file under the same hashed name holds the same, unless it's from before the catalog was gzipped
fn write_new(root: &Path, path: &str, text: &str) {
    let gzipped = fs::read(root.join(path)).is_ok_and(|bytes| bytes.starts_with(&[0x1f, 0x8b]));
    if !gzipped {
        write(root, path, &gzip(text));
    }
}

/// A file as the bucket serves it, `None` for one that isn't there
fn read(root: &Path, path: &str) -> Option<Value> {
    let file = fs::File::open(root.join(path)).ok()?;
    Some(serde_json::from_reader(flate2::read::GzDecoder::new(file)).unwrap())
}

/// Stored as the bucket serves it, since each run starts from a download of the bucket
fn gzip(text: &str) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(text.as_bytes()).unwrap();
    encoder.finish().unwrap()
}

/// Renamed into place, since a later run trusts any file already under a hashed name
fn write(root: &Path, path: &str, bytes: &[u8]) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    let temporary = file.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temporary, bytes).unwrap();
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
    write(
        root,
        "retired.json",
        Value::Object(kept).to_string().as_bytes(),
    );
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
    fn a_market_variant_states_its_markets() {
        let catalog = Catalog::new(BTreeMap::from([(
            "vehicle_models/acme/2025/two.json".to_string(),
            json!({"schema_version": "2.0.0", "id": "m/acme/2025/two", "markets": ["US"],
                "market_variants": {"GB": {"markets": ["GB"]}, "CA": {"model": "Two CA"}}}),
        )]));
        assert_eq!(
            market_variants(&catalog),
            BTreeMap::from([(
                "m/acme/2025/two".to_string(),
                vec!["m/acme/2025/two@GB".to_string()]
            )])
        );
    }

    #[test]
    fn listed_puts_a_market_variant_on_its_base() {
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
        let (listed, listed_comparisons) = listed(&rows, &comparisons, &variants);
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
    fn format_6_puts_the_kit_in_the_vocabulary() {
        let [index, vocabulary, kit, comparison] = [1, 2, 3, 4].map(|each| json!(each));
        let published = |format| files(format, &index, &vocabulary, &kit, &comparison);
        assert_eq!(
            published(5),
            [
                ("index", vec![&index]),
                ("vocabulary", vec![&vocabulary]),
                ("kit", vec![&kit]),
                ("comparison", vec![&comparison]),
            ]
        );
        assert_eq!(
            published(6),
            [
                ("index", vec![&index]),
                ("vocabulary", vec![&vocabulary, &kit]),
                ("comparison", vec![&comparison]),
            ]
        );
    }

    #[test]
    fn write_new_gzips_over_an_uncompressed_file_and_keeps_a_gzipped_one() {
        use std::io::Read;
        let root = std::env::temp_dir().join(format!("write_new_{}", std::process::id()));
        write(&root, "plain.json", b"{}");
        write(&root, "gzipped.json", &gzip("{}"));
        ["plain.json", "gzipped.json"]
            .iter()
            .for_each(|path| write_new(&root, path, "[]"));
        let read = |path: &str| {
            let mut text = String::new();
            flate2::read::GzDecoder::new(fs::File::open(root.join(path)).unwrap())
                .read_to_string(&mut text)
                .unwrap();
            text
        };
        assert_eq!(read("plain.json"), "[]");
        assert_eq!(read("gzipped.json"), "{}");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retire_deletes_only_what_has_been_unreferenced_past_retention() {
        let root = std::env::temp_dir().join(format!("generate_catalog_{}", std::process::id()));
        let long_ago =
            humantime::format_rfc3339_seconds(SystemTime::now() - RETENTION * 2).to_string();
        write(
            &root,
            "retired.json",
            json!({"old.json": long_ago}).to_string().as_bytes(),
        );
        ["old.json", "recent.json"]
            .iter()
            .for_each(|path| write(&root, path, b"{}"));
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
