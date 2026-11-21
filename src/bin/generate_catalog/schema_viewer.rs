//! The schema viewer Bike Index shows at /bikebook/schema_viewer: each document's fields in its schema's order, with how
//! many vehicle models hold each value and a few that do.
//!
//! What goes where. Each rule's specifics are in the constant it names:
//! - Documents, as tabs in this order: `DOCUMENTS`. Top-level fields left out: `HIDDEN`.
//! - `type`: the schema's, renamed by `TYPES`. An object has its fields under `fields`, and an object of records under
//!   their ids names the id as `keyed_by` (`KEYED_BY`) and has the record's fields under `each`. Any other object is a
//!   `hash`. A list of records is an `array` with the record's fields under `each`, a list of values one with their
//!   type as `of`.
//! - `unit`: the schema's `x-unit`.
//! - `description`: the schema's, less the unit it names ("Reach, in millimeters" is `reach`), its first letter
//!   lowercased unless it starts an acronym or a name in title case. One naming values one by one, "a: …; b and c: …",
//!   gives each value its part, and is left off the field when it names nothing but the field's values.
//! - `values`: an enum's in its order, or a bool's true then false. A value's `description` is what the field's says
//!   of it, else its display name in the published vocabulary where that says more than the value. Another document's
//!   field takes the names of the vehicle_model field `NAMED_LIKE` gives it.
//! - `models_count` and `example_ids`, on vehicle_model's fields: on a value, the models holding it; on a string, a
//!   number or a list of them, the models filling it. The examples are up to `REPRESENTATIVE_EXAMPLES` of bikebook's
//!   representative vehicles, then up to `OTHER_EXAMPLES` others, each the best by a maker none picked has, then a
//!   type none has, the newest year (its `years`, else its final or first year) and the most fields filled.

use crate::index::year_records;
use crate::ruby::{array, blank};
use bikebook_data_checks::Schemas;
use rayon::prelude::*;
use serde_json::{Map, Value, json};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap};

const DOCUMENTS: [&str; 3] = ["vehicle_model", "component", "e_vehicle_classification"];
/// Each file's bookkeeping, and its market variants, which restate its own fields
const HIDDEN: [&str; 4] = ["$schema", "schema_version", "id", "market_variants"];
/// `<document>.<path>` and the vehicle_model definition the vocabulary names its values under
const NAMED_LIKE: [(&str, &str); 5] = [
    ("component.type", "components.type"),
    ("component.markets", "markets"),
    ("component.motor.kind", "motors.kind"),
    (
        "component.motor.operating_modes.e_vehicle_classifications",
        "motors.operating_modes.e_vehicle_classifications",
    ),
    (
        "component.motor.operating_modes.markets",
        "motors.operating_modes.markets",
    ),
];
const TYPES: [(&str, &str); 2] = [("number", "float"), ("boolean", "bool")];
const KEYED_BY: [(&str, &str); 1] = [("years", "year")];
const REPRESENTATIVE_EXAMPLES: usize = 3;
const OTHER_EXAMPLES: usize = 5;

/// A field's path and a value it holds, `None` for a string or number it fills
type Key = (String, Option<String>);

/// A vehicle model, as `examples` picks among them
struct Model<'a> {
    id: &'a str,
    maker: &'a str,
    kind: Option<&'a str>,
    newest_year: Option<i64>,
    keys: usize,
    representative: bool,
}

/// `names`: the vocabulary's display names, by the vehicle_model definition they name the values of
pub fn call(
    schemas: &Schemas,
    names: &Map<String, Value>,
    models: &BTreeMap<String, Value>,
    representatives: &[String],
) -> Map<String, Value> {
    let schema = |name: &str| {
        let version = schemas.current_version(name).unwrap();
        &schemas.documents[&Schemas::id(name, &version)]
    };
    let documents: Map<String, Value> = DOCUMENTS
        .iter()
        .map(|&name| {
            let document = schema(name);
            let reader = Reader {
                documents: &schemas.documents,
                name,
                document,
                names,
            };
            let fields = reader.fields(&document["properties"], "");
            let fields = if name == "vehicle_model" {
                counted(&fields, "", &stats(&fields, models, representatives))
            } else {
                fields
            };
            (
                name.to_string(),
                json!({"url": document["$id"], "fields": fields}),
            )
        })
        .collect();
    json!({
        "schema_version": schemas.current_version("vehicle_model"),
        "versioning": schema("vehicle_model")["description"],
        "representative_ids": representatives,
        "documents": documents,
    })
    .as_object()
    .unwrap()
    .clone()
}

struct Reader<'a> {
    /// The schemas by `$id`
    documents: &'a HashMap<String, Value>,
    name: &'a str,
    document: &'a Value,
    names: &'a Map<String, Value>,
}

impl Reader<'_> {
    fn fields(&self, properties: &Value, prefix: &str) -> Map<String, Value> {
        properties
            .as_object()
            .unwrap()
            .iter()
            .filter(|(name, _)| !prefix.is_empty() || !HIDDEN.contains(&name.as_str()))
            .map(|(name, property)| {
                let path = joined(prefix, name);
                (name.clone(), self.field(property, &path))
            })
            .collect()
    }

    fn field(&self, property: &Value, path: &str) -> Value {
        let schema = self.resolved(self.document, property);
        let items = self.resolved(self.document, &schema["items"]);
        let records = &schema["additionalProperties"];
        let unit = schema["x-unit"].as_str();
        let (kind, of, keyed_by, children) = match type_of(&schema) {
            "object" if schema.get("properties").is_some() => (
                "object",
                None,
                None,
                Some(("fields", &schema["properties"])),
            ),
            "object" if records.get("properties").is_some() => (
                "object",
                None,
                Some(
                    lookup(&KEYED_BY, path)
                        .unwrap_or_else(|| panic!("KEYED_BY names no key for {path}")),
                ),
                Some(("each", &records["properties"])),
            ),
            "object" => ("hash", None, None, None),
            "array" if items.get("properties").is_some() => {
                ("array", None, None, Some(("each", &items["properties"])))
            }
            "array" => ("array", Some(name_of(type_of(&items))), None, None),
            other => (name_of(other), None, None, None),
        };
        let enumerated = if kind == "array" { &items } else { &schema };
        let definition = |schema: &Value| {
            schema["$ref"]
                .as_str()?
                .strip_prefix("#/$defs/")
                .map(String::from)
        };
        let named_like = match self.name {
            "vehicle_model" => definition(&schema["items"]).or_else(|| definition(property)),
            document => lookup(&NAMED_LIKE, &format!("{document}.{path}")).map(String::from),
        };
        let names = named_like.and_then(|name| self.names.get(&name));
        let description = schema["description"]
            .as_str()
            .map(|description| described(description, unit))
            .filter(|description| !description.is_empty());
        let values = values(enumerated, description.as_deref(), names);
        let description = description.filter(|description| {
            !values
                .as_ref()
                .is_some_and(|values| explained(description, values))
        });
        [
            ("type", Some(json!(kind))),
            ("of", of.map(Value::from)),
            ("keyed_by", keyed_by.map(Value::from)),
            ("unit", unit.map(Value::from)),
            ("description", description.map(Value::from)),
            ("values", values.map(Value::from)),
        ]
        .into_iter()
        .chain(children.map(|(key, properties)| (key, Some(json!(self.fields(properties, path))))))
        .filter_map(|(key, value)| Some((key.to_string(), value?)))
        .collect::<Map<_, _>>()
        .into()
    }

    /// The schema with its `$ref`s followed, the referring schema's own keys over the definition's
    fn resolved(&self, document: &Value, schema: &Value) -> Value {
        let Some((url, pointer)) = schema["$ref"]
            .as_str()
            .and_then(|reference| reference.split_once('#'))
        else {
            return schema.clone();
        };
        let document = if url.is_empty() {
            document
        } else {
            &self.documents[url]
        };
        let definition = self.resolved(document, document.pointer(pointer).unwrap());
        with(&definition, schema.as_object().unwrap().clone())
            .as_object()
            .unwrap()
            .clone()
            .into_iter()
            .filter(|(key, _)| key != "$ref")
            .collect()
    }
}

/// The object with `added` over its own
fn with(object: &Value, added: impl IntoIterator<Item = (String, Value)>) -> Value {
    object
        .as_object()
        .unwrap()
        .clone()
        .into_iter()
        .chain(added)
        .collect()
}

fn joined(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}.{name}")
    }
}

fn lookup(table: &[(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    table
        .iter()
        .find(|(each, _)| *each == key)
        .map(|(_, value)| *value)
}

fn type_of(schema: &Value) -> &str {
    match &schema["type"] {
        Value::Array(types) => types
            .iter()
            .filter_map(Value::as_str)
            .find(|kind| *kind != "null")
            .unwrap(),
        kind => kind.as_str().unwrap_or("string"),
    }
}

fn name_of(kind: &str) -> &str {
    lookup(&TYPES, kind).unwrap_or(kind)
}

/// On a field with a unit, less a closing ", in <unit>" or a whole "In <unit>"
fn described(description: &str, unit: Option<&str>) -> String {
    let unitless = |phrase: &str| !phrase.contains(',');
    let description = match description.rsplit_once(", in ") {
        _ if unit.is_none() => description,
        Some((rest, phrase)) if unitless(phrase) => rest,
        _ if description.starts_with("In ") && unitless(description) => "",
        _ => description,
    };
    let mut words = description.split(' ');
    let first = words.next().unwrap_or_default();
    let capitalized = |word: &str| word.chars().next().is_some_and(char::is_uppercase);
    if first.chars().nth(1).is_some_and(char::is_uppercase) || words.next().is_some_and(capitalized)
    {
        return description.to_string();
    }
    let mut characters = description.chars();
    characters
        .next()
        .map(|initial| initial.to_lowercase().chain(characters).collect())
        .unwrap_or_default()
}

/// A description describing values one by one, "a: …; b and c: …", as each name and what it says of it. `None` for
/// one that doesn't start by naming values
fn value_descriptions(description: &str) -> Option<Vec<(String, String)>> {
    let named = |segment: &str| {
        let (names, text) = segment.split_once(": ")?;
        let names: Vec<String> = names
            .split(", ")
            .flat_map(|names| names.split(" and "))
            .map(String::from)
            .collect();
        names
            .iter()
            .all(|name| {
                !name.is_empty()
                    && name
                        .chars()
                        .all(|character| character.is_alphanumeric() || "_-".contains(character))
            })
            .then(|| (names, text.to_string()))
    };
    let segments: Vec<&str> = description.split("; ").collect();
    let described = segments
        .chunk_by(|_, next| named(next).is_none())
        .map(|group| named(&group.join("; ")))
        .collect::<Option<Vec<_>>>()?;
    Some(
        described
            .into_iter()
            .flat_map(|(names, text)| names.into_iter().map(move |name| (name, text.clone())))
            .collect(),
    )
}

/// Whether the description only describes the values, so theirs say all of it
fn explained(description: &str, values: &[Value]) -> bool {
    value_descriptions(description).is_some_and(|described| {
        described.iter().all(|(name, _)| {
            values
                .iter()
                .any(|value| value["value"].as_str() == Some(name))
        })
    })
}

fn values(schema: &Value, description: Option<&str>, names: Option<&Value>) -> Option<Vec<Value>> {
    let values: Vec<Value> = match &schema["enum"] {
        Value::Array(values) => values
            .iter()
            .filter(|value| !value.is_null())
            .cloned()
            .collect(),
        _ if type_of(schema) == "boolean" => vec![json!(true), json!(false)],
        _ => return None,
    };
    let described: HashMap<String, String> = description
        .and_then(value_descriptions)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let letters = |text: &str| -> String {
        text.chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|character| character.to_ascii_lowercase())
            .collect()
    };
    Some(
        values
            .into_iter()
            .map(|value| {
                let slug = value.as_str().unwrap_or_default();
                let name = names
                    .and_then(|names| names[slug].as_str())
                    .filter(|name| letters(name) != letters(slug));
                match described.get(slug).map(String::as_str).or(name) {
                    Some(description) => json!({"value": value, "description": description}),
                    None => json!({"value": value}),
                }
            })
            .collect(),
    )
}

/// Each key the models hold, with how many hold it and examples of them
fn stats(
    fields: &Map<String, Value>,
    models: &BTreeMap<String, Value>,
    representatives: &[String],
) -> HashMap<Key, (usize, Vec<String>)> {
    // Ordered by a hash of the id, so examples tied on everything `examples` weighs come from across the catalog
    // rather than its first makers
    let mut ordered: Vec<(&String, &Value)> = models.iter().collect();
    ordered.sort_by_cached_key(|(id, _)| crate::digest(id));
    let (models, keys): (Vec<Model>, Vec<BTreeSet<Key>>) = ordered
        .par_iter()
        .map(|(id, vehicle)| {
            let keys = keys(fields, vehicle, "");
            let model = Model {
                id,
                maker: id.split('/').nth(1).unwrap_or_default(),
                kind: vehicle["type"].as_str(),
                newest_year: newest_year(vehicle),
                keys: keys.len(),
                representative: representatives.iter().any(|each| each == *id),
            };
            (model, keys)
        })
        .unzip();
    let holders = keys.iter().enumerate().fold(
        HashMap::<&Key, Vec<usize>>::new(),
        |mut holders, (index, keys)| {
            keys.iter()
                .for_each(|key| holders.entry(key).or_default().push(index));
            holders
        },
    );
    holders
        .into_par_iter()
        .map(|(key, holding)| (key.clone(), (holding.len(), examples(&models, &holding))))
        .collect()
}

/// The keys a record holds: each enum value or bool, and each string or number (or list of them) it fills
fn keys(fields: &Map<String, Value>, record: &Value, prefix: &str) -> BTreeSet<Key> {
    fields
        .iter()
        .flat_map(|(name, field)| -> BTreeSet<Key> {
            let path = joined(prefix, name);
            let value = &record[name.as_str()];
            match (&field["each"], &field["fields"]) {
                (Value::Object(each), _) => match value {
                    Value::Object(keyed) => keyed.values().collect(),
                    _ => array(Some(value)),
                }
                .into_iter()
                .flat_map(|record| keys(each, record, &path))
                .collect(),
                (_, Value::Object(fields)) => keys(fields, value, &path),
                _ if field.get("values").is_some() => array(Some(value))
                    .into_iter()
                    .filter_map(text)
                    .map(|text| (path.clone(), Some(text)))
                    .collect(),
                _ if filled(field) && !blank(Some(value)) => BTreeSet::from([(path, None)]),
                _ => BTreeSet::new(),
            }
        })
        .collect()
}

fn filled(field: &Value) -> bool {
    ["string", "integer", "float"].contains(
        &field["of"]
            .as_str()
            .or(field["type"].as_str())
            .unwrap_or_default(),
    )
}

fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Bool(_) | Value::Number(_) => Some(value.to_string()),
        _ => None,
    }
}

fn newest_year(vehicle: &Value) -> Option<i64> {
    year_records(vehicle)
        .into_iter()
        .map(|(year, _)| year)
        .max()
        .or_else(|| {
            ["final_year", "first_year"]
                .iter()
                .find_map(|key| vehicle[key].as_i64())
        })
}

/// `fields` with each count on the field or value it counts
fn counted(
    fields: &Map<String, Value>,
    prefix: &str,
    stats: &HashMap<Key, (usize, Vec<String>)>,
) -> Map<String, Value> {
    let with_count = |entry: &Value, key: Key| {
        let count = stats.get(&key).map(|(count, examples)| {
            vec![
                ("models_count".to_string(), json!(count)),
                ("example_ids".to_string(), json!(examples)),
            ]
        });
        with(entry, count.unwrap_or_default())
    };
    fields
        .iter()
        .map(|(name, field)| {
            let path = joined(prefix, name);
            let children = ["each", "fields"]
                .into_iter()
                .find(|key| field.get(*key).is_some());
            let field = match (children, &field["values"]) {
                (Some(children), _) => {
                    let counted = counted(field[children].as_object().unwrap(), &path, stats);
                    with(field, vec![(children.to_string(), json!(counted))])
                }
                (None, Value::Array(values)) => {
                    let values: Vec<Value> = values
                        .iter()
                        .map(|value| with_count(value, (path.clone(), text(&value["value"]))))
                        .collect();
                    with(field, vec![("values".to_string(), json!(values))])
                }
                _ if filled(field) => with_count(field, (path, None)),
                _ => field.clone(),
            };
            (name.clone(), field)
        })
        .collect()
}

fn examples(models: &[Model], holding: &[usize]) -> Vec<String> {
    let (representatives, others): (Vec<usize>, Vec<usize>) = holding
        .iter()
        .partition(|&&index| models[index].representative);
    let picked = pick(models, &representatives, REPRESENTATIVE_EXAMPLES, vec![]);
    let count = picked.len() + OTHER_EXAMPLES;
    pick(models, &others, count, picked)
        .into_iter()
        .map(|index| models[index].id.to_string())
        .collect()
}

fn pick(models: &[Model], pool: &[usize], up_to: usize, picked: Vec<usize>) -> Vec<usize> {
    (picked.len()..up_to).fold(picked, |picked, _| {
        let score = |index: usize| {
            let model = &models[index];
            (
                picked.iter().all(|&each| models[each].maker != model.maker),
                picked.iter().all(|&each| models[each].kind != model.kind),
                model.newest_year,
                model.keys,
            )
        };
        let best = pool
            .iter()
            .filter(|index| !picked.contains(index))
            .max_by_key(|&&index| (score(index), Reverse(index)));
        match best {
            Some(&index) => [picked, vec![index]].concat(),
            None => picked,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reader_fields(document: &Value, names: &Map<String, Value>) -> Value {
        let vocabulary = json!({"$defs": {"market": {"type": "string", "enum": ["US", "EU"]}}});
        let documents = HashMap::from([(
            "https://example.com/vocabulary.json".to_string(),
            vocabulary,
        )]);
        let reader = Reader {
            documents: &documents,
            name: "vehicle_model",
            document,
            names,
        };
        json!(reader.fields(&document["properties"], ""))
    }

    #[test]
    fn fields_take_the_viewers_shape_with_their_values_described() {
        let document = json!({
            "properties": {
                "id": {"type": "string"},
                "frame": {"type": "object", "properties": {
                    "top_tube": {"$ref": "#/$defs/frame.top_tube"},
                    "reach": {"type": ["number", "null"], "description": "Reach, in millimeters", "x-unit": "mm"},
                }},
                "markets": {"type": "array", "items": {"$ref": "#/$defs/markets"}, "description": "ISO codes"},
                "years": {"type": "object", "propertyNames": {"pattern": "^\\d{4}$"}, "additionalProperties": {
                    "type": "object", "properties": {"url_live": {"type": ["boolean", "null"], "description": "Whether it answers"}}
                }},
                "motors": {"type": "array", "items": {"type": "object", "properties": {"id": {"type": "string"}}}},
                "extra_measurements": {"type": ["object", "null"], "description": "In millimeters", "x-unit": "mm"},
            },
            "$defs": {
                "frame.top_tube": {
                    "type": ["string", "null"],
                    "enum": ["horizontal", "sloping", "step_through", null],
                    "description": "Sloping: drops toward the seat tube; step_through: low or none",
                },
                "markets": {"$ref": "https://example.com/vocabulary.json#/$defs/market"},
            },
        });
        let names = json!({"frame.top_tube": {"horizontal": "Horizontal", "sloping": "Compact"}, "markets": {"US": "United States"}});
        assert_eq!(
            reader_fields(&document, names.as_object().unwrap()),
            json!({
                "frame": {"type": "object", "fields": {
                    "top_tube": {"type": "string", "values": [
                        {"value": "horizontal"},
                        {"value": "sloping", "description": "drops toward the seat tube"},
                        {"value": "step_through", "description": "low or none"},
                    ]},
                    "reach": {"type": "float", "unit": "mm", "description": "reach"},
                }},
                "markets": {"type": "array", "of": "string", "description": "ISO codes", "values": [
                    {"value": "US", "description": "United States"},
                    {"value": "EU"},
                ]},
                "years": {"type": "object", "keyed_by": "year", "each": {
                    "url_live": {"type": "bool", "description": "whether it answers", "values": [{"value": true}, {"value": false}]},
                }},
                "motors": {"type": "array", "each": {"id": {"type": "string"}}},
                "extra_measurements": {"type": "hash", "unit": "mm"},
            })
        );
    }

    #[test]
    fn a_description_keeps_its_case_for_an_acronym_or_a_title() {
        assert_eq!(
            described("Model family grouping", None),
            "model family grouping"
        );
        assert_eq!(
            described("ISO 4217 currency code", None),
            "ISO 4217 currency code"
        );
        assert_eq!(
            described("Top Tube (effective), in degrees", Some("degrees")),
            "Top Tube (effective)"
        );
    }

    #[test]
    fn a_description_naming_a_value_it_lacks_stays_on_the_field() {
        let values = [json!({"value": "sloping"})];
        assert!(explained("sloping: drops toward the seat tube", &values));
        assert!(!explained("sloping: drops; compact: low", &values));
        assert!(!explained("e.g. BSA, T47 or threaded", &values));
    }

    #[test]
    fn keys_count_values_bools_and_filled_fields_through_records() {
        let fields = json!({
            "model": {"type": "string"},
            "description": {"type": "string"},
            "first_year": {"type": "integer"},
            "propulsion": {"type": "array", "of": "string", "values": []},
            "years": {"type": "object", "keyed_by": "year", "each": {"url_live": {"type": "bool", "values": []}}},
            "frame": {"type": "object", "fields": {"material": {"type": "string", "values": []}}},
            "sizes": {"type": "array", "each": {"name": {"type": "string"}}},
            "extra": {"type": "hash"},
        });
        let model = json!({
            "model": "One", "description": " ", "first_year": 2025, "propulsion": ["foot-pedal", "throttle"],
            "years": {"2025": {"url_live": true}, "2024": {"url_live": false}}, "frame": {"material": "steel"},
            "sizes": [{"name": "M"}], "extra": {"bb_drop": 70},
        });
        let keys = keys(fields.as_object().unwrap(), &model, "");
        let key = |path: &str, value: Option<&str>| (path.to_string(), value.map(String::from));
        assert_eq!(
            keys,
            BTreeSet::from([
                key("first_year", None),
                key("frame.material", Some("steel")),
                key("model", None),
                key("propulsion", Some("foot-pedal")),
                key("propulsion", Some("throttle")),
                key("sizes.name", None),
                key("years.url_live", Some("false")),
                key("years.url_live", Some("true")),
            ])
        );
    }

    #[test]
    fn examples_take_representatives_then_others_by_new_maker_new_type_and_newest_year() {
        let model = |id: &'static str, kind, newest_year, representative| Model {
            id,
            maker: id.split('/').nth(1).unwrap(),
            kind: Some(kind),
            newest_year: Some(newest_year),
            keys: 10,
            representative,
        };
        let models = [
            model("m/a/2025/one", "bike", 2025, false),
            model("m/a/2025/two", "tandem", 2025, true),
            model("m/b/2025/three", "bike", 2025, false),
            model("m/c/2020/old", "tricycle", 2020, false),
            model("m/d/2025/four", "tricycle", 2025, false),
        ];
        assert_eq!(
            examples(&models, &[0, 1, 2, 3, 4]),
            [
                "m/a/2025/two",
                "m/b/2025/three",
                "m/d/2025/four",
                "m/c/2020/old",
                "m/a/2025/one"
            ]
        );
    }
}
