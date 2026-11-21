//! A row per model, of what the viewer lists, filters and sorts by

use crate::markets::{Classifications, Markets};
use crate::ruby::{blank, truthy};
use bikebook_data_checks::dashed;
use serde_json::{Value, json};

pub const PHOTO_URL: &str = "https://bikebook.bikeindex.org";
const ELECTRIC_PROPULSIONS: [&str; 3] = ["pedal-assist", "throttle", "pedal-assist-and-throttle"];

pub struct Row<'a> {
    pub id: &'a str,
    pub vehicle: &'a Value,
    pub incomplete: bool,
}

impl Row<'_> {
    pub fn manufacturer(&self) -> &str {
        self.id.split('/').nth(1).unwrap_or_default()
    }

    /// Its first year is its file's directory's
    fn first_year(&self) -> Value {
        json!(
            self.id
                .split('/')
                .nth(2)
                .and_then(|year| year.parse::<i64>().ok())
        )
    }

    pub fn to_json(&self, markets: &Markets, classifications: &Classifications) -> Value {
        let vehicle = self.vehicle;
        let price = year_records(vehicle)
            .into_iter()
            .filter(|(_, record)| !blank(record.get("original_msrp")))
            .fold(
                None,
                |latest: Option<(i64, &Value)>, (year, record)| match latest {
                    Some((latest_year, _)) if latest_year >= year => latest,
                    _ => Some((year, record)),
                },
            )
            .map(|(_, record)| record);
        let photo = vehicle["stock_photo"].as_str().map(|url| {
            let prefix = format!(
                "{PHOTO_URL}/{}.",
                self.id.strip_prefix("m/").unwrap_or(self.id)
            );
            url.strip_prefix(&prefix).unwrap_or(url)
        });
        let text = |key: &str| vehicle[key].as_str().map_or(Value::Null, Value::from);
        json!({
            "id": self.id,
            "manufacturer": self.manufacturer(),
            "model": vehicle["model"],
            "display": display(vehicle),
            "first_year": self.first_year(),
            "final_year": vehicle["final_year"],
            "primary_activity": vehicle["primary_activity"],
            "vehicle_type": vehicle["type"],
            "model_configuration": vehicle["model_configuration"],
            "electric": electric(vehicle),
            "e_vehicle_classifications": classifications.of(vehicle),
            "suspension": suspension(vehicle),
            "markets": markets.sold(vehicle, self.manufacturer()),
            "msrp_cents": price.map_or(&Value::Null, |record| &record["original_msrp"]),
            "msrp_currency": price.map_or(&Value::Null, |record| &record["original_msrp_currency"]),
            "manufacturer_name": text("manufacturer"),
            "model_group": text("model_group"),
            "handlebar_type": text("handlebar_type"),
            "target_riders": if blank(vehicle.get("target_riders")) { &Value::Null } else { &vehicle["target_riders"] },
            "photo": photo,
            "incomplete": self.incomplete,
        })
    }
}

/// Each year's record with its year, from a map keyed by year or an array of records naming theirs
pub fn year_records(vehicle: &Value) -> Vec<(i64, &Value)> {
    match &vehicle["years"] {
        Value::Object(years) => years
            .iter()
            .map(|(year, record)| (leading_integer(year), record))
            .collect(),
        Value::Array(records) => records
            .iter()
            .map(|record| (record["year"].as_i64().unwrap_or_default(), record))
            .collect(),
        _ => vec![],
    }
}

/// `String#to_i`
fn leading_integer(string: &str) -> i64 {
    string
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or_default()
}

fn display(vehicle: &Value) -> String {
    let name = ["manufacturer", "model"]
        .iter()
        .filter(|key| !blank(vehicle.get(**key)))
        .map(|key| {
            vehicle[key]
                .as_str()
                .map_or_else(|| vehicle[key].to_string(), str::to_string)
        })
        .collect::<Vec<_>>()
        .join(" ");
    match truthy(vehicle.get("final_year"))
        .or(truthy(vehicle.get("first_year")).map(|_| &Value::Null))
    {
        Some(Value::Null) => format!("{name} (current)"),
        Some(year) => format!("{name} ({year})"),
        None => name,
    }
}

/// null when it states no propulsion
fn electric(vehicle: &Value) -> Value {
    if blank(vehicle.get("propulsion")) {
        return Value::Null;
    }
    json!(
        crate::ruby::array(vehicle.get("propulsion"))
            .into_iter()
            .any(|item| item
                .as_str()
                .is_some_and(|item| ELECTRIC_PROPULSIONS.contains(&dashed(item).as_str())))
    )
}

/// A page stating no travel still names a suspension fork or a rear shock. A frameset has no fork, so its rear shock makes
/// it a full suspension frame
fn suspension(vehicle: &Value) -> &'static str {
    let mut ends: Vec<&str> = vehicle["suspension"]
        .as_object()
        .map_or(vec![], |ends| ends.keys().map(String::as_str).collect());
    if vehicle["model_configuration"] == "frame"
        && ends.contains(&"rear")
        && !ends.contains(&"front")
    {
        ends.push("front");
    }
    ends.sort_unstable();
    match ends.as_slice() {
        ["front", "rear"] => "full",
        ["front"] => "hardtail",
        ["rear"] => "rear",
        _ => "rigid",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suspension_names_exactly_its_ends() {
        let cases = [
            (json!({}), "rigid"),
            (json!({"suspension": {"front": {}}}), "hardtail"),
            (json!({"suspension": {"rear": {}}}), "rear"),
            (json!({"suspension": {"rear": {}, "front": {}}}), "full"),
            (
                json!({"suspension": {"rear": {}}, "model_configuration": "frame"}),
                "full",
            ),
            (
                json!({"suspension": {"front": {}, "seatpost": {}}}),
                "rigid",
            ),
        ];
        for (vehicle, expected) in cases {
            assert_eq!(suspension(&vehicle), expected, "{vehicle}");
        }
    }

    #[test]
    fn display_names_the_final_year_or_current() {
        assert_eq!(
            display(&json!({"manufacturer": "Trek", "model": "Fuel+", "first_year": 2024})),
            "Trek Fuel+ (current)"
        );
        assert_eq!(
            display(&json!({"manufacturer": "Trek", "model": "Fuel+", "final_year": 2025})),
            "Trek Fuel+ (2025)"
        );
        assert_eq!(
            display(&json!({"manufacturer": "Trek", "model": ""})),
            "Trek"
        );
    }

    #[test]
    fn electric_is_null_without_propulsion() {
        assert_eq!(electric(&json!({"propulsion": []})), Value::Null);
        assert_eq!(electric(&json!({"propulsion": ["human"]})), json!(false));
        assert_eq!(
            electric(&json!({"propulsion": ["human", "throttle"]})),
            json!(true)
        );
        for propulsion in ["pedal_assist", "pedal-assist"] {
            assert_eq!(electric(&json!({"propulsion": [propulsion]})), json!(true));
        }
    }

    #[test]
    fn target_riders_is_null_when_blank() {
        let markets = Markets::load(&json!({"$defs": {"market": {"enum": []}}}));
        let classifications = Classifications::new(Default::default());
        let riders = |vehicle: Value| {
            Row {
                id: "m/x/2026/y",
                vehicle: &vehicle,
                incomplete: false,
            }
            .to_json(&markets, &classifications)["target_riders"]
                .clone()
        };
        assert_eq!(riders(json!({"target_riders": ["kid"]})), json!(["kid"]));
        assert_eq!(riders(json!({"target_riders": []})), Value::Null);
    }

    #[test]
    fn year_records_reads_a_map_or_an_array() {
        let keyed = json!({"years": {"2024": {"a": 1}}});
        assert_eq!(year_records(&keyed)[0].0, 2024);
        let listed = json!({"years": [{"year": 2025}]});
        assert_eq!(year_records(&listed)[0].0, 2025);
    }
}
