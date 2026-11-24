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

pub fn representative_vehicle_ids() -> Vec<String> {
    let mut reader = csv::Reader::from_path(root().join("representative_vehicles.csv")).unwrap();
    assert_eq!(reader.headers().unwrap(), vec!["vehicle_id", "notes"]);
    reader
        .records()
        .map(|record| record.unwrap()[0].to_string())
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
    /// Each file's `market_variants`, as the overlays over it they stand for, by their citation key: `<id>@<market>`
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
                data.get("market_variants")
                    .and_then(Value::as_object)
                    .into_iter()
                    .flatten()
                    .filter_map(move |(market, variant)| {
                        let mut overlay = Map::new();
                        overlay.insert("schema_version".into(), data["schema_version"].clone());
                        overlay.insert("id".into(), id.into());
                        overlay.insert("base".into(), id.into());
                        overlay.extend(variant.as_object()?.clone());
                        Some((format!("{id}@{market}"), Value::Object(overlay)))
                    })
            })
            .collect();
        Catalog { files, variants }
    }

    /// The files by path, then the market variants by citation key
    pub fn models(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.files.iter().chain(&self.variants)
    }

    /// Of a file's path or a market variant's citation key
    pub fn kind(path: &str) -> &'static str {
        match path.split('/').next() {
            Some("vehicle_models" | "m") => "vehicle_model",
            Some("components" | "c") => "component",
            _ => "e_vehicle_classification",
        }
    }

    /// A market variant's is its base's
    pub fn expected_id(path: &str) -> String {
        if let Some((id, _)) = path.split_once('@') {
            return id.to_string();
        }
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

    /// The key `models` gives a file's path or a market variant's citation key: the file's id, or the citation key
    pub fn citation_key(path: &str) -> String {
        if path.contains('@') {
            path.to_string()
        } else {
            Catalog::expected_id(path)
        }
    }

    /// What a market's model's photos are keyed by, as its id was before every market's model of a name and year
    /// shared one: the id suffixed with the market, unless it already ends in it
    pub fn photo_stem(id: &str, market: Option<&str>) -> String {
        match market.map(|market| format!("-{}", market.to_lowercase())) {
            Some(suffix) if !id.ends_with(&suffix) => format!("{id}{suffix}"),
            _ => id.to_string(),
        }
    }

    /// By id, a market variant by its citation key or, as ids named one before markets shared them, its photo stem
    pub fn by_id(&self, id: &str) -> Option<&Value> {
        Catalog::path(id)
            .and_then(|path| self.files.get(&path))
            .or_else(|| self.variants.get(id))
            .or_else(|| {
                self.variants
                    .iter()
                    .find(|(key, _)| {
                        let (base, market) = key.split_once('@').unwrap();
                        Catalog::photo_stem(base, Some(market)) == id
                    })
                    .map(|(_, data)| data)
            })
    }

    /// An overlay applied over its base; any other file as it is. Neither keeps the market variants, each a model of
    /// its own. An overlay that can't carry a drawing, a bike not suspended at both ends, drops its base's
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
        let suspended = ["front", "rear"].iter().all(|end| {
            merged
                .get("suspension")
                .and_then(|suspension| suspension.get(end))
                .is_some()
        });
        if merged.get("type").and_then(Value::as_str) == Some("bike") && !suspended {
            merged.shift_remove("geometry_photo");
        }
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
        let base = json!({"schema_version": "2.0.0", "id": "m/acme/2025/one", "markets": ["US"], "frame": {"material": "aluminum"},
            "sizes": [{"name": "S", "reach": 1}, {"name": "M", "reach": 2}]});
        let overlay = json!({"schema_version": "2.0.0", "id": "m/acme/2025/one-ca", "base": "m/acme/2025/one", "markets": ["CA"],
            "frame": false, "sizes": {"M": {"reach": 3}, "L": {"name": "L", "reach": 4}}});
        let catalog = Catalog::new(BTreeMap::from([(
            "vehicle_models/acme/2025/one.json".to_string(),
            base,
        )]));
        let resolved = catalog.resolve(&overlay).unwrap();
        assert_eq!(resolved["id"], "m/acme/2025/one-ca");
        assert_eq!(resolved["markets"], json!(["CA"]));
        assert!(resolved.get("frame").is_none());
        assert_eq!(
            resolved["sizes"],
            json!([{"name": "S", "reach": 1}, {"name": "M", "reach": 3}, {"name": "L", "reach": 4}])
        );
    }

    #[test]
    fn a_market_variant_shares_its_bases_id_and_resolves_as_an_overlay_over_it() {
        let base = json!({"schema_version": "2.0.0", "id": "m/acme/2025/one", "markets": ["EU"], "model": "One",
            "market_variants": {"US": {"markets": ["US"], "model": "One US"}}});
        let catalog = Catalog::new(BTreeMap::from([(
            "vehicle_models/acme/2025/one.json".to_string(),
            base.clone(),
        )]));
        assert_eq!(
            catalog.variants.keys().collect::<Vec<_>>(),
            ["m/acme/2025/one@US"]
        );
        let variant = catalog.by_id("m/acme/2025/one@US").unwrap();
        assert_eq!(catalog.by_id("m/acme/2025/one-us"), Some(variant));
        assert_eq!(catalog.by_id("m/acme/2025/one"), Some(&base));
        assert_eq!(
            catalog.resolve(variant).unwrap(),
            json!({"schema_version": "2.0.0", "id": "m/acme/2025/one", "markets": ["US"], "model": "One US"})
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
    fn a_photo_stem_suffixes_the_market_unless_the_id_ends_in_it() {
        assert_eq!(
            Catalog::photo_stem("m/acme/2025/one-two", Some("GB")),
            "m/acme/2025/one-two-gb"
        );
        assert_eq!(
            Catalog::photo_stem("m/acme/2025/one-us", Some("US")),
            "m/acme/2025/one-us"
        );
        assert_eq!(
            Catalog::photo_stem("m/acme/2025/one", None),
            "m/acme/2025/one"
        );
    }

    #[test]
    fn a_market_variant_that_cant_carry_a_drawing_drops_its_bases() {
        let base = json!({"schema_version": "2.0.0", "id": "m/acme/2025/one", "type": "bike",
            "suspension": {"front": {"travel": 100}, "rear": {"travel": 90}},
            "geometry_photo": "https://bikebook.bikeindex.org/acme/2025/one-geometry.png",
            "market_variants": {"GB": {"suspension": false}, "CA": {"markets": ["CA"]}}});
        let catalog = Catalog::new(BTreeMap::from([(
            "vehicle_models/acme/2025/one.json".to_string(),
            base,
        )]));
        let geometry = |key: &str| {
            catalog
                .resolve(&catalog.variants[key])
                .unwrap()
                .get("geometry_photo")
                .cloned()
        };
        assert_eq!(geometry("m/acme/2025/one@GB"), None);
        assert!(geometry("m/acme/2025/one@CA").is_some());
    }

    #[test]
    fn flags_an_image_by_its_key_or_its_extension() {
        assert!(image_url("stock_photo", "https://example.com/photo"));
        assert!(image_url("url", "https://example.com/a/b.JPG?w=200"));
        assert!(!image_url("url", "https://example.com/law.pdf"));
    }
}
