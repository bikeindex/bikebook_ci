//! What a model is missing to be complete, the messages the viewer shows for it

use crate::ruby::{array, blank};
use bikebook_data_checks::dashed;
use serde_json::Value;

const DRIVETRAINLESS_TYPES: [&str; 12] = [
    "e-scooter",
    "non-e-scooter",
    "non-e-skateboard",
    "personal-mobility",
    "trail-behind",
    "trailer",
    "unicycle",
    "stroller",
    "wheelchair",
    "balance-bike",
    "e-motorcycle",
    "e-atv",
];
const ACTIVITY_OPTIONAL_TYPES: [&str; 9] = [
    "stroller",
    "wheelchair",
    "trailer",
    "trail-behind",
    "non-e-skateboard",
    "non-e-scooter",
    "balance-bike",
    "personal-mobility",
    "e-motorcycle",
];
/// A trailer's page names its wheels by an inch figure that fits more than one rim
const WHEEL_OPTIONAL_TYPES: [&str; 1] = ["trailer"];
const BUILD_KIT: [&str; 5] = [
    "handlebar_type",
    "wheels",
    "drivetrain",
    "brakes",
    "components",
];

enum Required {
    Value,
    OneValue,
    /// An object's field
    Field(&'static str),
    /// An entry with each of these
    EntryWith(&'static [&'static str]),
    OneEntry,
}

/// The vehicle schema's fields a complete model populates, in its order
const FIELDS: [(&str, Required); 13] = [
    ("manufacturer", Required::Value),
    ("model", Required::Value),
    ("type", Required::Value),
    ("propulsion", Required::OneValue),
    ("primary_activity", Required::Value),
    ("handlebar_type", Required::Value),
    ("first_year", Required::Value),
    ("frame", Required::Field("material")),
    ("wheels", Required::EntryWith(&["bsd", "position"])),
    ("drivetrain", Required::OneValue),
    ("brakes", Required::EntryWith(&["type", "position"])),
    ("sizes", Required::OneEntry),
    ("components", Required::OneEntry),
];

pub fn messages(vehicle: &Value) -> Vec<String> {
    let exempt = exempt(vehicle);
    FIELDS
        .iter()
        .filter(|(key, _)| !exempt.contains(key))
        .filter_map(|(key, required)| {
            let value = vehicle.get(*key);
            match required {
                Required::Value => blank(value).then(|| format!("missing {key}")),
                Required::OneValue => array(value)
                    .into_iter()
                    .all(|item| blank(Some(item)))
                    .then(|| format!("{key} needs at least one value")),
                Required::Field(field) => blank(value.and_then(|value| value.get(field)))
                    .then(|| format!("missing {key}.{field}")),
                Required::EntryWith(fields) => (!array(value)
                    .into_iter()
                    .any(|entry| fields.iter().all(|field| !blank(entry.get(field)))))
                .then(|| format!("{key} needs an entry with {}", fields.join(" and "))),
                Required::OneEntry => array(value)
                    .is_empty()
                    .then(|| format!("{key} needs at least one entry")),
            }
        })
        .collect()
}

fn exempt(vehicle: &Value) -> Vec<&'static str> {
    let kind = dashed(vehicle["type"].as_str().unwrap_or_default());
    let kind = kind.as_str();
    let configuration = vehicle["model_configuration"].as_str();
    [
        (
            "drivetrain",
            DRIVETRAINLESS_TYPES.contains(&kind) || throttle(vehicle),
        ),
        ("primary_activity", ACTIVITY_OPTIONAL_TYPES.contains(&kind)),
        ("wheels", WHEEL_OPTIONAL_TYPES.contains(&kind)),
        ("brakes", blank(vehicle.get("brakes"))),
    ]
    .into_iter()
    .filter_map(|(key, exempt)| exempt.then_some(key))
    .chain(
        BUILD_KIT
            .into_iter()
            .filter(|_| matches!(configuration, Some("frame" | "frame_fork"))),
    )
    .collect()
}

/// A throttle vehicle's page often states no gearing
fn throttle(vehicle: &Value) -> bool {
    array(vehicle.get("propulsion")).contains(&&Value::from("throttle"))
        || array(vehicle.get("motors")).into_iter().any(|motor| {
            array(motor.get("operating_modes"))
                .into_iter()
                .any(|mode| mode["mode"] == "throttle")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn complete() -> Value {
        json!({
            "manufacturer": "Trek", "model": "Marlin 5", "type": "bike", "propulsion": ["human"],
            "primary_activity": "mtb", "handlebar_type": "flat", "first_year": 2024, "frame": {"material": "aluminum"},
            "wheels": [{"bsd": 622, "position": ["front"]}], "drivetrain": ["1x9"],
            "sizes": [{"name": "M"}], "components": [{"type": "saddle"}],
        })
    }

    #[test]
    fn a_complete_model_has_none() {
        assert_eq!(messages(&complete()), Vec::<String>::new());
    }

    #[test]
    fn each_missing_field_has_its_message_in_schema_order() {
        let mut vehicle = complete();
        let object = vehicle.as_object_mut().unwrap();
        object.remove("manufacturer");
        object.insert("propulsion".into(), json!([""]));
        object.insert("frame".into(), json!({}));
        object.insert("wheels".into(), json!([{"bsd": 622}]));
        object.insert("brakes".into(), json!([{"type": "disc"}]));
        object.insert("sizes".into(), json!([]));
        assert_eq!(
            messages(&vehicle),
            [
                "missing manufacturer",
                "propulsion needs at least one value",
                "missing frame.material",
                "wheels needs an entry with bsd and position",
                "brakes needs an entry with type and position",
                "sizes needs at least one entry",
            ]
        );
    }

    #[test]
    fn a_type_is_exempt_in_either_spelling() {
        for kind in ["bike_trailer", "trailer"] {
            assert_eq!(
                exempt(&json!({"type": kind})),
                ["drivetrain", "primary_activity", "wheels", "brakes"]
            );
        }
        for kind in ["epamd", "personal-mobility", "scooter", "non-e-scooter"] {
            assert_eq!(
                exempt(&json!({"type": kind})),
                ["drivetrain", "primary_activity", "brakes"]
            );
        }
        assert_eq!(exempt(&json!({"type": "e-atv"})), ["drivetrain", "brakes"]);
    }

    #[test]
    fn a_frameset_and_a_throttle_vehicle_are_exempt() {
        let frame = json!({"model_configuration": "frame", "type": "bike"});
        assert!(
            !messages(&frame)
                .iter()
                .any(|message| message.contains("wheels")
                    || message.contains("drivetrain")
                    || message.contains("components"))
        );
        let throttled = json!({"motors": [{"operating_modes": [{"mode": "throttle"}]}]});
        assert!(
            !messages(&throttled)
                .iter()
                .any(|message| message.contains("drivetrain"))
        );
    }
}
