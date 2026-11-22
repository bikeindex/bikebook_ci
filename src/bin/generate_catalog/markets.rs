//! The markets a model is sold in, which the index filters by

use crate::ruby::{array, blank};
use bikebook_data_checks::{dashed, dashes};
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

pub struct Markets {
    snapshot: Value,
    /// The vocabulary's
    markets: Vec<String>,
}

static WAYBACK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\Ahttps?://(?:web\.archive\.org/web|index\.commoncrawl\.org/CC-MAIN-[\d-]+)/(\d{4,14})[a-z_]*/(.+)\z").unwrap()
});
static PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\A(?:[a-z]+:)?//[^/]+(/[^?#]*)").unwrap());
static LOCALE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[a-z]{2}[-_][a-z]{2}\z").unwrap());
static HOST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\A(?:[a-z]+:)?//([a-z]{2})\.[^/.]+\.[a-z.]+(?:[:/?#]|\z)").unwrap()
});

impl Markets {
    pub fn load(vocabulary: &Value) -> Markets {
        Markets {
            snapshot: serde_json::from_str(include_str!("../../../catalog_snapshot/markets.json"))
                .unwrap(),
            markets: vocabulary["$defs"]["market"]["enum"]
                .as_array()
                .expect("the vocabulary schema lists no markets")
                .iter()
                .filter_map(|market| Some(market.as_str()?.to_string()))
                .collect(),
        }
    }

    fn lookup(&self, table: &str, key: &str) -> Option<&str> {
        self.snapshot[table][key].as_str()
    }

    fn listed(&self, list: &str, value: &str) -> bool {
        array(self.snapshot.get(list)).contains(&&Value::from(value))
    }

    /// Those it states, else its pages', else its maker's home market
    pub fn sold(&self, vehicle: &Value, manufacturer: &str) -> Value {
        if !blank(vehicle.get("markets")) {
            return vehicle["markets"].clone();
        }
        let pages: Vec<String> = crate::index::year_records(vehicle)
            .into_iter()
            .filter_map(|(_, record)| self.record_market(record))
            .fold(vec![], |markets, market| {
                if markets.contains(&market) {
                    markets
                } else {
                    [markets, vec![market]].concat()
                }
            });
        if pages.is_empty() {
            Value::from(vec![self.home(manufacturer)])
        } else {
            Value::from(pages)
        }
    }

    /// keyed by schema 1.x's slugs
    fn home(&self, manufacturer: &str) -> &str {
        self.lookup("primary_markets", &manufacturer.replace('-', "_"))
            .unwrap_or("US")
    }

    /// The market a model is the model of, by the first it states, when that's not its maker's home market's
    pub fn foreign(&self, vehicle: &Value, manufacturer: &str) -> Option<String> {
        let stated = array(vehicle.get("markets"));
        let home = Value::from(self.home(manufacturer));
        if stated.contains(&&home) {
            return None;
        }
        Some(stated.first()?.as_str()?.to_string())
    }

    /// An unpriced record's store is its page's
    fn record_market(&self, record: &Value) -> Option<String> {
        record["original_msrp_currency"]
            .as_str()
            .and_then(|currency| self.lookup("currency_markets", currency))
            .map(str::to_string)
            .or_else(|| self.url_market(record.get("url")?.as_str()?))
    }

    /// The store a url's first path segments name: a locale ("/en-us/", "/de_en/"), a country beside its language
    /// ("/us/en/") or a bare "/us/"; a language alone is no store, nor is "/ca/" alone, Catalan's. Else the store its
    /// host's first label names ("eu.", "uk."), which a language's ("de.") doesn't
    fn url_market(&self, url: &str) -> Option<String> {
        let original = WAYBACK
            .captures(url)
            .map_or(url, |captures| captures.get(2).unwrap().as_str());
        let path = PATH
            .captures(original)
            .map_or(String::new(), |captures| captures[1].to_lowercase());
        let segments: Vec<&str> = path
            .split('/')
            .filter(|segment| !segment.trim().is_empty())
            .collect();
        let (first, second) = (segments.first().copied(), segments.get(1).copied());
        let language = |code: &str| self.listed("url_languages", code);
        let locales = [first, second]
            .into_iter()
            .flatten()
            .filter(|segment| LOCALE.is_match(segment))
            .flat_map(|locale| {
                let (code, country) = (&locale[..2], &locale[3..]);
                [
                    language(code).then_some(country),
                    language(country).then_some(code),
                ]
            })
            .flatten();
        let bare =
            first.filter(|first| second.is_some_and(language) || ["us", "usa"].contains(first));
        locales
            .chain(bare)
            .find_map(|country| self.market(country))
            .or_else(|| {
                let host = HOST.captures(original)?[1].to_lowercase();
                self.market(&host).filter(|_| !language(&host))
            })
    }

    /// An "en-<country>" locale's market: its country, or the EU for a member
    fn market(&self, country: &str) -> Option<String> {
        let country = match country {
            "uk" => "gb",
            "usa" => "us",
            country => country,
        };
        let locale = format!("en-{country}");
        if country == "la" {
            return None;
        }
        let code = if country.len() == 2
            && country
                .chars()
                .all(|character| character.is_ascii_lowercase())
        {
            country.to_uppercase()
        } else {
            self.lookup("language_countries", &locale)?.to_string()
        };
        let market = if self.listed("eu_members", &code) {
            "EU".to_string()
        } else {
            code
        };
        self.markets.contains(&market).then_some(market)
    }
}

/// The classifications an index row's filter matches
pub struct Classifications {
    records: serde_json::Map<String, Value>,
}

const OUT_OF_CLASS: &str = "evc/out-of-class";
const US_CLASSES: [&str; 3] = ["evc/us/class-1", "evc/us/class-2", "evc/us/class-3"];
/// Types whose motor only throttles
const THROTTLED_TYPES: [&str; 4] = ["e-scooter", "personal-mobility", "e-motorcycle", "e-atv"];

impl Classifications {
    pub fn new(records: serde_json::Map<String, Value>) -> Classifications {
        Classifications { records }
    }

    /// The groups the model's operating modes are classified in, then `evc/out-of-class` (`evc/out_of_class` on schema
    /// 1.x), past every e-bike class and law, or for a class 3 throttle each jurisdiction that bars it
    pub fn of(&self, vehicle: &Value) -> Vec<String> {
        let groups = operating_modes(vehicle)
            .flat_map(|mode| array(mode.get("e_vehicle_classifications")))
            .filter_map(Value::as_str)
            .map(|id| self.carried(id))
            .fold(vec![], |groups: Vec<String>, group| {
                if groups.contains(&group) {
                    groups
                } else {
                    [groups, vec![group]].concat()
                }
            });
        let past_e_bikes = groups.iter().any(|group| {
            !US_CLASSES.contains(&dashed(group).as_str())
                && blank(
                    self.records
                        .get(group)
                        .and_then(|record| record.get("e_bike_law")),
                )
        });
        let kind = dashed(vehicle["type"].as_str().unwrap_or_default());
        let markers: Vec<String> = if kind == "e-motorcycle"
            || (!THROTTLED_TYPES.contains(&kind.as_str()) && past_e_bikes)
        {
            vec![OUT_OF_CLASS.to_string()]
        } else if groups.iter().any(|group| dashed(group) == US_CLASSES[2])
            && modes(vehicle).contains(&"throttle")
        {
            self.class_3_throttle_bars()
        } else {
            vec![]
        };
        let underscored = !dashes(vehicle["schema_version"].as_str().unwrap_or_default());
        let markers = markers.into_iter().map(|marker| {
            if underscored {
                marker.replace('-', "_")
            } else {
                marker
            }
        });
        groups.into_iter().chain(markers).collect()
    }

    /// The one group a classification names, in its place
    fn carried(&self, id: &str) -> String {
        match array(self.records.get(id).and_then(|record| record.get("groups"))).as_slice() {
            [group] => group.as_str().unwrap_or(id).to_string(),
            _ => id.to_string(),
        }
    }

    fn class_3_throttle_bars(&self) -> Vec<String> {
        self.records
            .values()
            .filter(|record| record["class_3_throttle"] == false)
            .map(|record| {
                let jurisdiction = record["jurisdiction"]
                    .as_str()
                    .unwrap_or_default()
                    .to_lowercase()
                    .replace('-', "/");
                format!("{OUT_OF_CLASS}/{jurisdiction}")
            })
            .fold(vec![], |bars, bar| {
                if bars.contains(&bar) {
                    bars
                } else {
                    [bars, vec![bar]].concat()
                }
            })
    }

    /// The slowest limit of those that name one, rounded
    pub fn speed_limit(&self, ids: &[&Value]) -> Option<i64> {
        ids.iter()
            .filter_map(|id| self.records.get(id.as_str()?)?.get("max_speed")?.as_f64())
            .min_by(f64::total_cmp)
            .map(|speed| speed.round() as i64)
    }
}

pub fn operating_modes(vehicle: &Value) -> impl Iterator<Item = &Value> {
    array(vehicle.get("motors"))
        .into_iter()
        .flat_map(|motor| array(motor.get("operating_modes")))
}

pub fn stock(mode: &Value) -> bool {
    match mode.get("availability") {
        None | Some(Value::Null) => true,
        Some(availability) => {
            ["stock", "app_update"].contains(&availability.as_str().unwrap_or_default())
        }
    }
}

/// Its stock propulsions; one with no operating mode for it is a stock one
pub fn modes(vehicle: &Value) -> Vec<&'static str> {
    [("assist", "pedal-assist"), ("throttle", "throttle")]
        .into_iter()
        .filter(|(mode, propulsion)| {
            let named: Vec<&Value> = operating_modes(vehicle)
                .filter(|each| each["mode"] == *mode)
                .collect();
            if named.is_empty() {
                array(vehicle.get("propulsion")).into_iter().any(|item| {
                    item.as_str()
                        .is_some_and(|item| dashed(item) == *propulsion)
                })
            } else {
                named.iter().any(|each| stock(each))
            }
        })
        .map(|(mode, _)| mode)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn markets() -> Markets {
        Markets::load(&json!({"$defs": {"market": {"enum": ["US", "EU", "GB", "CA", "AU"]}}}))
    }

    #[test]
    fn url_market_reads_locales_bare_countries_and_hosts() {
        let markets = markets();
        let cases = [
            ("https://example.com/en-us/bike", Some("US")),
            ("https://example.com/de_en/bike", Some("EU")),
            ("https://example.com/uk/en/bike", Some("GB")),
            ("https://example.com/usa/bike", Some("US")),
            ("https://example.com/en/bike", None),
            ("https://example.com/ca/bike", None),
            ("https://au.example.com/bike", Some("AU")),
            ("https://de.example.com/bike", None),
            (
                "https://web.archive.org/web/2020im_/https://example.com/en-gb/bike",
                Some("GB"),
            ),
        ];
        for (url, expected) in cases {
            assert_eq!(markets.url_market(url).as_deref(), expected, "{url}");
        }
    }

    #[test]
    fn sold_falls_back_to_pages_then_the_home_market() {
        let markets = markets();
        let stated = json!({"markets": ["CA"]});
        assert_eq!(markets.sold(&stated, "trek"), json!(["CA"]));
        let priced = json!({"years": {"2024": {"original_msrp_currency": "GBP"}, "2025": {"url": "https://example.com/en-gb/x"}}});
        assert_eq!(markets.sold(&priced, "trek"), json!(["GB"]));
        assert_eq!(markets.sold(&json!({}), "3t"), json!(["EU"]));
        assert_eq!(markets.sold(&json!({}), "riese-and-muller"), json!(["EU"]));
        assert_eq!(markets.sold(&json!({}), "trek"), json!(["US"]));
    }

    fn classifications() -> Classifications {
        Classifications::new(
            json!({
                "evc/us/class_3": {"max_speed": 45.06},
                "evc/us/ny/class_3": {"groups": ["evc/us/class_3"]},
                "evc/us/ny/nyc": {"jurisdiction": "US-NY", "class_3_throttle": false},
                "evc/l1e_b_moped": {"e_bike_law": false},
            })
            .as_object()
            .unwrap()
            .clone(),
        )
    }

    fn classified(ids: Value, propulsion: Value, kind: &str) -> Value {
        json!({"type": kind, "propulsion": propulsion, "motors": [{"operating_modes": [{"e_vehicle_classifications": ids}]}]})
    }

    #[test]
    fn a_class_3_throttle_is_barred_where_its_jurisdictions_bar_it() {
        let vehicle = classified(json!(["evc/us/ny/class_3"]), json!(["throttle"]), "bike");
        assert_eq!(
            classifications().of(&vehicle),
            ["evc/us/class_3", "evc/out_of_class/us/ny"]
        );
        let assisted = classified(json!(["evc/us/class_3"]), json!(["pedal_assist"]), "bike");
        assert_eq!(classifications().of(&assisted), ["evc/us/class_3"]);
    }

    #[test]
    fn past_e_bike_law_is_out_of_class_unless_throttled() {
        let moped = classified(json!(["evc/l1e_b_moped"]), json!([]), "bike");
        assert_eq!(
            classifications().of(&moped),
            ["evc/l1e_b_moped", "evc/out_of_class"]
        );
        let scooter = classified(json!(["evc/l1e_b_moped"]), json!([]), "e_scooter");
        assert_eq!(classifications().of(&scooter), ["evc/l1e_b_moped"]);
    }

    #[test]
    fn a_2_0_0_model_is_classified_and_marked_dashed() {
        let classifications = Classifications::new(
            json!({
                "evc/us/class-3": {"max_speed": 45.06},
                "evc/us/ny/class-3": {"groups": ["evc/us/class-3"]},
                "evc/us/ny/nyc": {"jurisdiction": "US-NY", "class_3_throttle": false},
                "evc/l1e-b-moped": {"e_bike_law": false},
            })
            .as_object()
            .unwrap()
            .clone(),
        );
        let of = |ids: Value, propulsion: Value, kind: &str| {
            let mut vehicle = classified(ids, propulsion, kind);
            vehicle["schema_version"] = json!("2.0.0");
            classifications.of(&vehicle)
        };
        assert_eq!(
            of(json!(["evc/us/ny/class-3"]), json!(["throttle"]), "bike"),
            ["evc/us/class-3", "evc/out-of-class/us/ny"]
        );
        assert_eq!(
            of(json!(["evc/us/class-3"]), json!(["pedal-assist"]), "bike"),
            ["evc/us/class-3"]
        );
        assert_eq!(
            of(json!(["evc/us/class-3"]), json!([]), "e-motorcycle"),
            ["evc/us/class-3", "evc/out-of-class"]
        );
        assert_eq!(
            of(json!(["evc/l1e-b-moped"]), json!([]), "bike"),
            ["evc/l1e-b-moped", "evc/out-of-class"]
        );
        assert_eq!(
            of(json!(["evc/l1e-b-moped"]), json!([]), "personal-mobility"),
            ["evc/l1e-b-moped"]
        );
    }

    #[test]
    fn modes_counts_a_propulsion_without_operating_modes_as_stock() {
        assert_eq!(
            modes(&json!({"propulsion": ["pedal_assist", "throttle"]})),
            ["assist", "throttle"]
        );
        assert_eq!(modes(&json!({"propulsion": ["pedal-assist"]})), ["assist"]);
        let unlockable = json!({"propulsion": ["throttle"], "motors": [{"operating_modes": [{"mode": "throttle", "availability": "unlockable"}]}]});
        assert_eq!(modes(&unlockable), Vec::<&str>::new());
    }
}
