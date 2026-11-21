use rayon::prelude::*;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub const DATA_DIRECTORIES: [&str; 3] =
    ["vehicle_models", "components", "e_vehicle_classifications"];
pub const SCHEMA_DIRECTORY: &str = "schemas";
/// The stock photos bucket's public url
pub const IMAGE_PREFIXES: [&str; 1] = ["https://bikebook.bikeindex.org/"];
pub const SCHEMA_URL: &str = "https://bikebook.bikeindex.org/schemas/";

/// Each array an overlay merges item by item, and the fields telling its items apart (none: by position)
const IDENTITIES: [(&str, &[&str]); 8] = [
    ("colorways", &["name"]),
    ("components", &["type", "position", "type_detail"]),
    ("motors", &[]),
    ("operating_modes", &["mode", "availability", "markets"]),
    ("sizes", &["name"]),
    ("mounts", &["type"]),
    ("wheels", &["position", "bsd"]),
    ("brakes", &["position"]),
];

/// The 1.x slugs schema 2.0.0 renames rather than dashes
const RENAMED: [(&str, &str); 33] = [
    ("bike_trailer", "trailer"),
    ("cargo_bike", "cargo"),
    ("cargo_bike_rear", "cargo-rear"),
    ("cargo_tricycle", "cargo-trike"),
    ("cargo_tricycle_rear", "cargo-trike-rear"),
    ("epamd", "personal-mobility"),
    ("scooter", "non-e-scooter"),
    ("skateboard", "non-e-skateboard"),
    ("elliptical_bike", "elliptical"),
    ("pedal", "foot-pedal"),
    ("hand_cycle", "hand-pedal"),
    ("human_powered", "human-not-pedal"),
    ("road_endurance", "road-endurance-sportive"),
    ("carbon", "composite"),
    ("wood", "organic"),
    ("6_speed_pinion", "6-speed-pinion-gearbox"),
    ("9_speed_pinion", "9-speed-pinion-gearbox"),
    ("12_speed_pinion", "12-speed-pinion-gearbox"),
    ("18_speed_pinion", "18-speed-pinion-gearbox"),
    ("chain_guide", "bashguardchain-guide"),
    ("lights", "light"),
    ("bell_noisemaker", "bellnoisemaker"),
    ("cassette", "cogcassettefreewheel"),
    ("grips_tape", "gripstape"),
    ("aero_bars", "aero-barsextensionsbar-ends"),
    ("e_vehicle_battery", "ebike-battery"),
    ("e_vehicle_charger", "ebike-charger"),
    ("e_vehicle_remote", "ebike-displayremote"),
    ("e_vehicle_motor", "ebike-motor"),
    ("e_vehicle_wiring_harness", "e-bike-wiring-harness"),
    ("top_of_rack_bag", "rack-bag"),
    ("electronic_shifting_component", "electronic-shifting-part"),
    ("gps_connectivity_module", "gpsconnectivity-module"),
];

/// An id or slug as schema 2.0.0 spells it, from either spelling. Not for the enums 2.0.0 leaves underscored, like
/// `app_update`
pub fn dashed(value: &str) -> String {
    RENAMED
        .iter()
        .find(|(old, _)| *old == value)
        .map_or_else(|| value.replace('_', "-"), |(_, new)| new.to_string())
}

/// Whether a schema version dashes its ids and slugs
pub fn dashes(schema_version: &str) -> bool {
    schema_version
        .split('.')
        .next()
        .and_then(|major| major.parse::<u64>().ok())
        .is_some_and(|major| major >= 2)
}

/// A missing data directory reads as an empty catalog, which passes every check
static ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    let root = PathBuf::from(
        std::env::var_os("BIKEBOOK_DATA").expect("set BIKEBOOK_DATA to a bikebook checkout"),
    );
    assert!(
        root.join(SCHEMA_DIRECTORY).is_dir(),
        "BIKEBOOK_DATA ({}) has no {SCHEMA_DIRECTORY}/",
        root.display()
    );
    root
});

pub fn root() -> &'static Path {
    &ROOT
}

/// Relative to the repo root
pub fn json_paths(directory: &str) -> Vec<String> {
    fn walk(directory: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                out.push(path);
            }
        }
    }
    let mut paths = vec![];
    walk(&root().join(directory), &mut paths);
    let mut relative: Vec<String> = paths
        .iter()
        .map(|path| {
            path.strip_prefix(root())
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    relative.sort();
    relative
}

pub fn data_paths() -> Vec<String> {
    DATA_DIRECTORIES
        .iter()
        .flat_map(|directory| json_paths(directory))
        .collect()
}

/// A file that doesn't parse is left out, for `formatting` to report
pub fn parsed(paths: &[impl AsRef<str> + Sync]) -> BTreeMap<String, Value> {
    paths
        .par_iter()
        .map(AsRef::as_ref)
        .filter_map(|path| {
            let text = fs::read_to_string(root().join(path)).ok()?;
            serde_json::from_str(&text)
                .ok()
                .map(|value| (path.to_string(), value))
        })
        .collect()
}

/// What Ruby's `JSON.pretty_generate` writes, with the trailing newline the catalog adds
pub fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap() + "\n"
}

pub fn assert_none(check: &str, problems: Vec<String>) {
    assert!(
        problems.is_empty(),
        "{check}: {} problems\n{}",
        problems.len(),
        problems.join("\n")
    );
}

pub struct Catalog {
    pub files: BTreeMap<String, Value>,
    /// Each file's `market_variants`, as the overlays over it they stand for, by the path their id names
    pub variants: BTreeMap<String, Value>,
}

impl Catalog {
    pub fn load() -> Catalog {
        Catalog::new(parsed(&data_paths()))
    }

    pub fn new(files: BTreeMap<String, Value>) -> Catalog {
        let variants = files
            .values()
            .flat_map(|data| {
                let id = data.get("id").and_then(Value::as_str).unwrap_or_default();
                let separator = if dashes(data["schema_version"].as_str().unwrap_or_default()) {
                    '-'
                } else {
                    '_'
                };
                data.get("market_variants")
                    .and_then(Value::as_object)
                    .into_iter()
                    .flatten()
                    .filter_map(move |(market, variant)| {
                        let variant_id = format!("{id}{separator}{}", market.to_lowercase());
                        let mut overlay = Map::new();
                        overlay.insert("schema_version".into(), data["schema_version"].clone());
                        overlay.insert("id".into(), variant_id.clone().into());
                        overlay.insert("base".into(), id.into());
                        overlay.extend(variant.as_object()?.clone());
                        Some((Catalog::path(&variant_id)?, Value::Object(overlay)))
                    })
            })
            .collect();
        Catalog { files, variants }
    }

    /// The files, then the market variants
    pub fn models(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.files.iter().chain(&self.variants)
    }

    pub fn kind(path: &str) -> &'static str {
        match path.split('/').next() {
            Some("vehicle_models") => "vehicle_model",
            Some("components") => "component",
            _ => "e_vehicle_classification",
        }
    }

    pub fn expected_id(path: &str) -> String {
        let (directory, rest) = path.split_once('/').unwrap();
        let prefix = match directory {
            "vehicle_models" => "m/",
            "components" => "c/",
            _ => "evc/",
        };
        format!("{prefix}{}", rest.trim_end_matches(".json"))
    }

    pub fn path(id: &str) -> Option<String> {
        let (prefix, rest) = id.split_once('/')?;
        let directory = match prefix {
            "m" => "vehicle_models",
            "c" => "components",
            "evc" => "e_vehicle_classifications",
            _ => return None,
        };
        Some(format!("{directory}/{rest}.json"))
    }

    pub fn by_id(&self, id: &str) -> Option<&Value> {
        let path = Catalog::path(id)?;
        self.files.get(&path).or_else(|| self.variants.get(&path))
    }

    /// An overlay applied over its base; any other file as it is. Neither keeps the market variants, each a model of
    /// its own
    pub fn resolve(&self, data: &Value) -> Result<Value, String> {
        let Some(base_id) = data.get("base").and_then(Value::as_str) else {
            let mut resolved = data.clone();
            if let Some(map) = resolved.as_object_mut() {
                map.shift_remove("market_variants");
            }
            return Ok(resolved);
        };
        let base = self
            .by_id(base_id)
            .ok_or_else(|| format!("its base {base_id} isn't in this repo"))?;
        let mut merged = vehicle(base);
        merge(
            &mut merged,
            &vehicle(data)
                .into_iter()
                .filter(|(key, _)| key != "base")
                .collect(),
        )?;
        let mut resolved = Map::new();
        for key in ["schema_version", "id"] {
            if let Some(value) = data.get(key) {
                resolved.insert(key.to_string(), value.clone());
            }
        }
        resolved.extend(merged);
        Ok(Value::Object(resolved))
    }
}

fn vehicle(data: &Value) -> Map<String, Value> {
    data.as_object()
        .into_iter()
        .flatten()
        .filter(|(key, _)| !["schema_version", "id", "market_variants"].contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// false removes an object the base has, null any other value; an object over an array merges into its keyed items
fn merge(out: &mut Map<String, Value>, overlay: &Map<String, Value>) -> Result<(), String> {
    for (key, value) in overlay {
        let existing = out.get_mut(key);
        match (value, existing) {
            (Value::Null, _) => {
                out.shift_remove(key);
            }
            (Value::Bool(false), Some(Value::Object(_))) => {
                out.shift_remove(key);
            }
            (Value::Object(changes), Some(Value::Array(items))) => {
                let mut keyed = keyed(key, items)
                    .ok_or_else(|| format!("the base's {key} can't be told apart to merge into"))?;
                merge(&mut keyed, changes)?;
                *items = keyed.into_iter().map(|(_, item)| item).collect();
            }
            (Value::Object(changes), Some(Value::Object(object))) => merge(object, changes)?,
            _ => {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    Ok(())
}

fn keyed(key: &str, items: &[Value]) -> Option<Map<String, Value>> {
    let fields = IDENTITIES.iter().find(|(name, _)| *name == key)?.1;
    if !items.iter().all(Value::is_object) {
        return None;
    }
    let ids: Vec<String> = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            if fields.is_empty() {
                return index.to_string();
            }
            fields
                .iter()
                .map(|field| match item.get(*field) {
                    None | Some(Value::Null) => String::new(),
                    Some(Value::Array(values)) => {
                        values.iter().map(ruby_string).collect::<Vec<_>>().join("+")
                    }
                    Some(value) => ruby_string(value),
                })
                .filter(|id| !id.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    let unique: std::collections::HashSet<&String> = ids.iter().collect();
    (unique.len() == ids.len()).then(|| ids.into_iter().zip(items.iter().cloned()).collect())
}

fn ruby_string(value: &Value) -> String {
    match value {
        Value::String(string) => string.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// A vehicle model's years, as a map keyed by year or an array of records naming theirs
pub fn years(vehicle: &Value) -> Vec<String> {
    match vehicle.get("years") {
        Some(Value::Object(map)) => map.keys().cloned().collect(),
        Some(Value::Array(records)) => records
            .iter()
            .filter_map(|record| record.get("year"))
            .map(ruby_string)
            .collect(),
        _ => vec![],
    }
}

/// Every string in the value under a key, with its JSON pointer
pub fn strings<'a>(value: &'a Value, path: String, out: &mut Vec<(String, String, &'a str)>) {
    match value {
        Value::Object(map) => map.iter().for_each(|(key, child)| {
            if let Value::String(string) = child {
                out.push((format!("{path}/{key}"), key.clone(), string));
            }
            strings(child, format!("{path}/{key}"), out)
        }),
        Value::Array(items) => items.iter().enumerate().for_each(|(index, child)| {
            if let Value::String(string) = child {
                out.push((format!("{path}/{index}"), String::new(), string));
            }
            strings(child, format!("{path}/{index}"), out)
        }),
        _ => {}
    }
}

pub fn image_url(key: &str, string: &str) -> bool {
    let named = ["photo", "image", "logo", "icon"]
        .iter()
        .any(|word| key.contains(word));
    let path = string
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let extension = path
        .rsplit_once('.')
        .map(|(_, extension)| extension)
        .unwrap_or_default();
    let image = string.starts_with("http")
        && [
            "jpg", "jpeg", "png", "webp", "avif", "gif", "svg", "tif", "tiff", "heic",
        ]
        .contains(&extension);
    named || image
}

/// The published schemas, by `$id`: each current one at `schemas/<name>.json`, and each older version a file still
/// names at `schemas/<name>/v<version>.json`
pub struct Schemas {
    pub documents: HashMap<String, Value>,
    current: Vec<String>,
}

impl Schemas {
    pub fn load() -> Schemas {
        let files = parsed(&json_paths(SCHEMA_DIRECTORY));
        let id = |document: &Value| Some(document.get("$id")?.as_str()?.to_string());
        let current = files
            .iter()
            .filter(|(path, _)| path.matches('/').count() == 1)
            .filter_map(|(_, document)| id(document))
            .collect();
        let documents = files
            .into_values()
            .filter_map(|document| Some((id(&document)?, document)))
            .collect();
        Schemas { documents, current }
    }

    pub fn id(name: &str, version: &str) -> String {
        format!("{SCHEMA_URL}{name}/v{version}.json")
    }

    pub fn current_version(&self, name: &str) -> Option<String> {
        let prefix = format!("{SCHEMA_URL}{name}/v");
        self.current
            .iter()
            .find_map(|id| Some(id.strip_prefix(&prefix)?.strip_suffix(".json")?.to_string()))
    }

    pub fn validator(&self, id: &str) -> Result<jsonschema::Validator, String> {
        let document = self
            .documents
            .get(id)
            .ok_or_else(|| format!("no schema {id}"))?;
        jsonschema::options()
            .with_retriever(Retriever(self.documents.clone()))
            .should_validate_formats(true)
            .build(document)
            .map_err(|error| format!("{id} doesn't build: {error}"))
    }

    fn dereference<'a>(&'a self, document: &'a str, node: &'a Value) -> (&'a str, &'a Value) {
        let Some(reference) = node.get("$ref").and_then(Value::as_str) else {
            return (document, node);
        };
        let (url, pointer) = reference.split_once('#').unwrap_or((reference, ""));
        let (document, root) = if url.is_empty() {
            (document, &self.documents[document])
        } else {
            match self.documents.get_key_value(url) {
                Some((id, root)) => (id.as_str(), root),
                None => return (document, node),
            }
        };
        match root.pointer(pointer) {
            Some(target) => self.dereference(document, target),
            None => (document, node),
        }
    }

    fn admits(&self, document: &str, node: &Value, value: &Value) -> bool {
        let (document, node) = self.dereference(document, node);
        if let Some(constant) = node.get("const") {
            return constant == value || !(constant.is_null() || value.is_null());
        }
        if let Some(kind) = node.get("type") {
            let kinds: Vec<&str> = match kind {
                Value::Array(kinds) => kinds.iter().filter_map(Value::as_str).collect(),
                other => other.as_str().into_iter().collect(),
            };
            return kinds.iter().any(|kind| match *kind {
                "null" => value.is_null(),
                "boolean" => value.is_boolean(),
                "object" => value.is_object(),
                "array" => value.is_array(),
                "string" => value.is_string(),
                "integer" => value.is_i64() || value.is_u64(),
                "number" => value.is_number(),
                _ => true,
            });
        }
        for keyword in ["anyOf", "oneOf"] {
            if let Some(Value::Array(branches)) = node.get(keyword) {
                return branches
                    .iter()
                    .any(|branch| self.admits(document, branch, value));
            }
        }
        true
    }

    /// The node and every subschema that applies to the value: its allOf, the anyOf/oneOf branches of the value's
    /// type, and the if/then/else branch its required keys pick
    fn branches<'a>(
        &'a self,
        document: &'a str,
        node: &'a Value,
        value: &Value,
        out: &mut Vec<(&'a str, &'a Value)>,
    ) {
        let (document, node) = self.dereference(document, node);
        out.push((document, node));
        if let Some(Value::Array(all)) = node.get("allOf") {
            all.iter()
                .for_each(|branch| self.branches(document, branch, value, out));
        }
        for keyword in ["anyOf", "oneOf"] {
            if let Some(Value::Array(branches)) = node.get(keyword) {
                branches
                    .iter()
                    .filter(|branch| self.admits(document, branch, value))
                    .for_each(|branch| self.branches(document, branch, value, out));
            }
        }
        if let Some(condition) = node.get("if") {
            let required = condition
                .get("required")
                .and_then(Value::as_array)
                .into_iter()
                .flatten();
            let holds = required
                .filter_map(Value::as_str)
                .all(|key| value.get(key).is_some());
            if let Some(branch) = node.get(if holds { "then" } else { "else" }) {
                self.branches(document, branch, value, out);
            }
        }
    }

    /// Each object whose keys aren't in the order the catalog writes them: its schema's property order, any key it
    /// doesn't name after them sorted, and a year-keyed object newest first. An overlay's removals (null, or false
    /// for an object), a market variant's among them, are left out, since they follow what it changes
    pub fn key_order_errors(&self, document: &str, value: &Value) -> Vec<String> {
        let mut errors = vec![];
        let overlay = value.get("base").is_some();
        let Some(map) = value.as_object() else {
            return errors;
        };
        if overlay {
            let leading: Vec<&str> = map.keys().take(3).map(String::as_str).collect();
            if leading != ["schema_version", "id", "base"] {
                errors.push(format!(
                    "(root): an overlay starts schema_version, id, base, not {}",
                    leading.join(", ")
                ));
            }
        }
        let root = &self.documents[document];
        let mut body = map.clone();
        if overlay {
            body.shift_remove("base");
        }
        self.order_errors(
            document,
            root,
            &Value::Object(body),
            String::new(),
            overlay,
            &mut errors,
        );
        errors
    }

    fn order_errors(
        &self,
        document: &str,
        node: &Value,
        value: &Value,
        path: String,
        overlay: bool,
        errors: &mut Vec<String>,
    ) {
        let mut candidates = vec![];
        self.branches(document, node, value, &mut candidates);
        match value {
            Value::Object(map) => {
                let mut properties: Vec<(&str, (&str, &Value))> = vec![];
                let mut additional = None;
                let mut by_year = false;
                for (document, candidate) in &candidates {
                    if let Some(Value::Object(named)) = candidate.get("properties") {
                        for (key, child) in named {
                            if !properties.iter().any(|(name, _)| name == key) {
                                properties.push((key, (document, child)));
                            }
                        }
                    }
                    if let Some(child @ Value::Object(_)) = candidate.get("additionalProperties") {
                        additional.get_or_insert((*document, child));
                    }
                    by_year |= candidate
                        .pointer("/propertyNames/pattern")
                        .and_then(Value::as_str)
                        == Some("^\\d{4}$");
                }
                let removal =
                    |child: &Value| overlay && (child.is_null() || *child == Value::Bool(false));
                let keys: Vec<&str> = map
                    .iter()
                    .filter(|(_, child)| !removal(child))
                    .map(|(key, _)| key.as_str())
                    .collect();
                let expected: Vec<&str> = if !properties.is_empty() {
                    let mut named: Vec<&str> = properties
                        .iter()
                        .map(|(key, _)| *key)
                        .filter(|key| keys.contains(key))
                        .collect();
                    let mut unnamed: Vec<&str> = keys
                        .iter()
                        .copied()
                        .filter(|key| !named.contains(key))
                        .collect();
                    unnamed.sort();
                    named.extend(unnamed);
                    named
                } else if by_year {
                    let mut sorted = keys.clone();
                    sorted.sort_by_key(|key| std::cmp::Reverse(key.parse::<i64>().unwrap_or(0)));
                    sorted
                } else {
                    keys.clone()
                };
                if keys != expected {
                    let at = if path.is_empty() { "(root)" } else { &path };
                    errors.push(format!(
                        "{at}: keys must be ordered {}",
                        expected.join(", ")
                    ));
                }
                for (key, child) in map {
                    let schema = properties
                        .iter()
                        .find(|(name, _)| name == key)
                        .map(|(_, schema)| *schema)
                        .or(additional);
                    if let Some((document, schema)) = schema {
                        self.order_errors(
                            document,
                            schema,
                            child,
                            format!("{path}/{key}"),
                            overlay || path == "/market_variants",
                            errors,
                        );
                    }
                }
            }
            Value::Array(items) => {
                if let Some((document, schema)) = candidates
                    .iter()
                    .find_map(|(document, candidate)| Some((*document, candidate.get("items")?)))
                {
                    for (index, item) in items.iter().enumerate() {
                        self.order_errors(
                            document,
                            schema,
                            item,
                            format!("{path}/{index}"),
                            overlay,
                            errors,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

/// Resolves a `$ref` to another published schema from the repo's own files, never the network
struct Retriever(HashMap<String, Value>);

impl jsonschema::Retrieve for Retriever {
    fn retrieve(
        &self,
        uri: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let url = uri.as_str().split('#').next().unwrap_or_default();
        self.0
            .get(url)
            .cloned()
            .ok_or_else(|| format!("{url} isn't a schema in this repository").into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merges_an_overlay_item_by_item() {
        let base = json!({"schema_version": "1.0.0", "id": "m/acme/2025/one", "markets": ["US"], "frame": {"material": "aluminum"},
            "sizes": [{"name": "S", "reach": 1}, {"name": "M", "reach": 2}]});
        let overlay = json!({"schema_version": "1.0.0", "id": "m/acme/2025/one_ca", "base": "m/acme/2025/one", "markets": ["CA"],
            "frame": false, "sizes": {"M": {"reach": 3}, "L": {"name": "L", "reach": 4}}});
        let catalog = Catalog::new(BTreeMap::from([(
            "vehicle_models/acme/2025/one.json".to_string(),
            base,
        )]));
        let resolved = catalog.resolve(&overlay).unwrap();
        assert_eq!(resolved["id"], "m/acme/2025/one_ca");
        assert_eq!(resolved["markets"], json!(["CA"]));
        assert!(resolved.get("frame").is_none());
        assert_eq!(
            resolved["sizes"],
            json!([{"name": "S", "reach": 1}, {"name": "M", "reach": 3}, {"name": "L", "reach": 4}])
        );
    }

    #[test]
    fn a_market_variant_resolves_as_an_overlay_and_its_base_without_it() {
        let base = json!({"schema_version": "1.1.0", "id": "m/acme/2025/one", "markets": ["EU"], "model": "One",
            "market_variants": {"US": {"markets": ["US"], "model": "One US"}}});
        let catalog = Catalog::new(BTreeMap::from([(
            "vehicle_models/acme/2025/one.json".to_string(),
            base.clone(),
        )]));
        let variant = catalog.by_id("m/acme/2025/one_us").unwrap();
        assert_eq!(
            catalog.resolve(variant).unwrap(),
            json!({"schema_version": "1.1.0", "id": "m/acme/2025/one_us", "markets": ["US"], "model": "One US"})
        );
        assert!(
            catalog
                .resolve(&base)
                .unwrap()
                .get("market_variants")
                .is_none()
        );
    }

    #[test]
    fn a_2_0_0_market_variant_is_suffixed_with_a_dash() {
        let base = json!({"schema_version": "2.0.0", "id": "m/acme/2025/one-two", "markets": ["EU"],
            "market_variants": {"US": {"markets": ["US"]}}});
        let catalog = Catalog::new(BTreeMap::from([(
            "vehicle_models/acme/2025/one-two.json".to_string(),
            base,
        )]));
        assert_eq!(
            catalog.variants.keys().collect::<Vec<_>>(),
            ["vehicle_models/acme/2025/one-two-us.json"]
        );
    }

    #[test]
    fn flags_an_image_by_its_key_or_its_extension() {
        assert!(image_url("stock_photo", "https://example.com/photo"));
        assert!(image_url("url", "https://example.com/a/b.JPG?w=200"));
        assert!(!image_url("url", "https://example.com/law.pdf"));
    }
}
