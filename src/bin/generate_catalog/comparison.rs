//! The figures the viewer compares models by, a column per field

use crate::markets::{Classifications, modes, operating_modes, stock};
use crate::ruby::{self, array, float, median, truthy};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

const FIT: [&str; 6] = [
    "stack",
    "reach",
    "rider_height_min",
    "rider_height_max",
    "rider_age_min",
    "rider_age_max",
];
pub const FIELDS: [&str; 31] = [
    "modes",
    "assist_speed",
    "throttle_speed",
    "battery_size",
    "motor_kind",
    "head_angle",
    "seat_angle",
    "stack_reach",
    "bb_drop",
    "chainstay_gap",
    "travel_front",
    "travel_rear",
    "tire_clearance",
    "front_bsd",
    "rear_bsd",
    "total_weight",
    "mounts",
    "bottle_mounts",
    "groupset_series",
    "frame_material",
    "target_riders",
    "size_names",
    "rider_height_min",
    "rider_height_max",
    "rider_height_estimated",
    "size_stack",
    "size_reach",
    "size_rider_height_min",
    "size_rider_height_max",
    "size_rider_age_min",
    "size_rider_age_max",
];
/// A family needs this many models stating their sizes' rider heights to estimate the others'
const MINIMUM_MODELS: usize = 100;

pub fn call(vehicle: &Value, classifications: &Classifications) -> Map<String, Value> {
    let motors = array(vehicle.get("motors"));
    let sizes = array(vehicle.get("sizes"));
    let stock_modes: Vec<&Value> = operating_modes(vehicle)
        .filter(|mode| stock(mode))
        .collect();
    let empty = json!({});
    let per_size: Vec<Map<String, Value>> = if sizes.is_empty() {
        vec![&empty]
    } else {
        sizes.clone()
    }
    .into_iter()
    .map(|size| size_values(vehicle, size))
    .collect();
    let fields = per_size.iter().flat_map(|values| values.keys()).fold(
        vec![],
        |fields: Vec<&String>, field| {
            if fields.contains(&field) {
                fields
            } else {
                [fields, vec![field]].concat()
            }
        },
    );
    let medians = fields.into_iter().map(|field| {
        let values: Vec<&Value> = per_size
            .iter()
            .filter_map(|values| truthy(values.get(field)))
            .collect();
        (field.clone(), json!(median(values)))
    });
    // a size stating one end of its riders' heights is open at the other, so the model is too
    let heights: Vec<[&Value; 2]> = sizes
        .iter()
        .map(|size| ["rider_height_min", "rider_height_max"].map(|bound| &size["geometry"][bound]))
        .filter(|bounds| bounds.iter().any(|bound| truthy(Some(bound)).is_some()))
        .collect();
    let height = |index: usize, pick: fn(Vec<&Value>) -> Option<&Value>| {
        let column: Vec<&Value> = heights.iter().map(|bounds| bounds[index]).collect();
        (!column.is_empty() && column.iter().all(|bound| truthy(Some(bound)).is_some()))
            .then(|| pick(column))
            .flatten()
            .cloned()
    };
    let mounts = vehicle["frame"]["mounts"].as_array().map(|mounts| {
        let mut types: Vec<&Value> = mounts.iter().map(|mount| &mount["type"]).collect();
        types.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
        types.dedup();
        json!(types)
    });
    let modes = modes(vehicle);
    let stated = [
        ("modes", (!modes.is_empty()).then(|| json!(modes))),
        (
            "assist_speed",
            speed(&stock_modes, "assist", classifications),
        ),
        (
            "throttle_speed",
            speed(&stock_modes, "throttle", classifications),
        ),
        (
            "battery_size",
            ruby::max(
                motors
                    .iter()
                    .filter_map(|motor| truthy(motor["battery"].get("capacity"))),
            )
            .cloned(),
        ),
        (
            "motor_kind",
            motors
                .iter()
                .find_map(|motor| truthy(motor.get("kind")))
                .cloned(),
        ),
        (
            "travel_front",
            truthy(vehicle["suspension"]["front"].get("travel")).cloned(),
        ),
        (
            "travel_rear",
            truthy(vehicle["suspension"]["rear"].get("travel")).cloned(),
        ),
        (
            "groupset_series",
            truthy(vehicle["groupset"].get("series")).cloned(),
        ),
        (
            "frame_material",
            truthy(vehicle["frame"].get("material")).cloned(),
        ),
        ("mounts", mounts),
        (
            "target_riders",
            (!ruby::blank(vehicle.get("target_riders"))).then(|| vehicle["target_riders"].clone()),
        ),
        ("rider_height_min", height(0, |column| ruby::min(column))),
        ("rider_height_max", height(1, |column| ruby::max(column))),
    ]
    .into_iter()
    .filter_map(|(field, value)| Some((field.to_string(), value?)));
    medians.chain(stated).chain(fit(&sizes)).collect()
}

fn size_values(vehicle: &Value, size: &Value) -> Map<String, Value> {
    let geometry = &size["geometry"];
    let name = &size["name"];
    let wheels: BTreeMap<&str, Option<&Value>> = ["front", "rear"]
        .into_iter()
        .map(|position| {
            (
                position,
                array(vehicle.get("wheels"))
                    .into_iter()
                    .find(|wheel| wheel_at(wheel, position, name)),
            )
        })
        .collect();
    let radii: BTreeMap<&str, Option<f64>> = wheels
        .iter()
        .map(|(position, wheel)| (*position, wheel.and_then(outer_radius)))
        .collect();
    let radius_values: Vec<Value> = ["front", "rear"]
        .iter()
        .filter_map(|position| radii[position])
        .map(Value::from)
        .collect();
    let axle_height = median(&radius_values);
    let field = |key: &str| truthy(geometry.get(key)).cloned();
    let stack_reach = match (
        truthy(geometry.get("stack")),
        geometry.get("reach").filter(|reach| float(reach) > 0.0),
    ) {
        (Some(stack), Some(reach)) => Some(json!(float(stack) / float(reach))),
        _ => None,
    };
    let bb_drop = field("bb_drop").or_else(|| {
        Some(json!(
            axle_height? - float(truthy(geometry.get("bb_height"))?)
        ))
    });
    let chainstay_gap = truthy(geometry.get("chainstay"))
        .and_then(|chainstay| Some(json!(float(chainstay) - radii["rear"]?)));
    let tire_widths: Vec<&Value> = ["front", "rear"]
        .iter()
        .filter_map(|position| wheels[position])
        .filter_map(|wheel| truthy(wheel.get("tire_width_max")).or(truthy(wheel.get("tire_width"))))
        .collect();
    let bottle_mounts = vehicle["frame"]["mounts"].as_array().map(|mounts| {
        json!(
            mounts
                .iter()
                .filter(|mount| mount["type"] == "bottle" && sized(mount, name))
                .count()
        )
    });
    [
        ("head_angle", field("head_angle")),
        ("seat_angle", field("seat_angle")),
        ("stack_reach", stack_reach),
        ("bb_drop", bb_drop),
        ("chainstay_gap", chainstay_gap),
        ("travel_front", field("travel_front")),
        ("travel_rear", field("travel_rear")),
        ("tire_clearance", ruby::min(tire_widths).cloned()),
        (
            "front_bsd",
            wheels["front"]
                .and_then(|wheel| truthy(wheel.get("bsd")))
                .cloned(),
        ),
        (
            "rear_bsd",
            wheels["rear"]
                .and_then(|wheel| truthy(wheel.get("bsd")))
                .cloned(),
        ),
        ("total_weight", field("total_weight")),
        ("bottle_mounts", bottle_mounts),
    ]
    .into_iter()
    .filter_map(|(key, value)| Some((key.to_string(), value?)))
    .collect()
}

fn fit(sizes: &[&Value]) -> Map<String, Value> {
    let fields: Vec<(String, Value)> = FIT
        .iter()
        .map(|key| {
            (
                format!("size_{key}"),
                sizes
                    .iter()
                    .map(|size| size["geometry"][key].clone())
                    .collect::<Vec<_>>(),
            )
        })
        .filter(|(_, values)| values.iter().any(|value| truthy(Some(value)).is_some()))
        .map(|(key, values)| (key, json!(values)))
        .collect();
    if fields.is_empty() {
        return Map::new();
    }
    let names: Vec<&Value> = sizes.iter().map(|size| &size["name"]).collect();
    [("size_names".to_string(), json!(names))]
        .into_iter()
        .chain(fields)
        .collect()
}

fn wheel_at(wheel: &Value, position: &str, size_name: &Value) -> bool {
    wheel["configured"] != false
        && array(wheel.get("position")).contains(&&json!(position))
        && sized(wheel, size_name)
}

fn sized(item: &Value, size_name: &Value) -> bool {
    match item.get("sizes") {
        None | Some(Value::Null) => true,
        Some(sizes) => array(Some(sizes)).contains(&size_name),
    }
}

fn outer_radius(wheel: &Value) -> Option<f64> {
    Some(float(truthy(wheel.get("bsd"))?) / 2.0 + float(truthy(wheel.get("tire_width"))?))
}

fn speed(modes: &[&Value], mode: &str, classifications: &Classifications) -> Option<Value> {
    let speeds: Vec<Value> = modes
        .iter()
        .filter(|each| each["mode"] == mode)
        .filter_map(|each| {
            truthy(each.get("max_speed")).cloned().or_else(|| {
                classifications
                    .speed_limit(&array(each.get("e_vehicle_classifications")))
                    .map(Value::from)
            })
        })
        .collect();
    ruby::max(&speeds).cloned()
}

/// Each size as [top tube, seat tube, minimum height, maximum height]: of the geometry figures, those two predicted stated
/// heights best, ~5cm out across brands
pub fn rider_height_sizes(vehicle: &Value) -> Vec<[Value; 4]> {
    array(vehicle.get("sizes"))
        .into_iter()
        .map(|size| {
            let geometry = &size["geometry"];
            let seat_tube = geometry
                .get("seat_tube_ct")
                .unwrap_or(&geometry["seat_tube_ctc"]);
            [
                &geometry["top_tube_effective"],
                seat_tube,
                &geometry["rider_height_min"],
                &geometry["rider_height_max"],
            ]
            .map(Value::clone)
        })
        .collect()
}

/// A model's primary activity family, its comparison and its `rider_height_sizes`
pub type Fitted = (Option<String>, Map<String, Value>, Vec<[Value; 4]>);

/// Each model's comparison, with rider heights estimated from its sizes' geometry where it states none, by a fit over its
/// primary activity family's models that do
pub fn with_rider_heights(rows: Vec<Fitted>) -> Vec<Map<String, Value>> {
    let families: BTreeMap<&String, Vec<&Vec<[Value; 4]>>> =
        rows.iter()
            .fold(BTreeMap::new(), |mut families, (family, _, sizes)| {
                if let Some(family) = family {
                    families.entry(family).or_insert_with(Vec::new).push(sizes);
                }
                families
            });
    let bounds: BTreeMap<&String, [[f64; 3]; 2]> = families
        .into_iter()
        .filter_map(|(family, sizes)| Some((family, fit_bounds(&sizes)?)))
        .collect();
    rows.iter()
        .map(|(family, comparison, sizes)| {
            match family.as_ref().and_then(|family| bounds.get(family)) {
                Some(bounds) if estimable(sizes) => {
                    let [minimums, maximums] = bounds.map(|[intercept, top_tube, seat_tube]| {
                        sizes
                            .iter()
                            .map(|size| {
                                intercept + top_tube * float(&size[0]) + seat_tube * float(&size[1])
                            })
                            .collect::<Vec<f64>>()
                    });
                    let minimum = minimums.into_iter().min_by(f64::total_cmp).unwrap();
                    let maximum = maximums.into_iter().max_by(f64::total_cmp).unwrap();
                    let mut estimated = comparison.clone();
                    estimated.insert(
                        "rider_height_min".into(),
                        json!(ruby::round_to_tens(minimum)),
                    );
                    estimated.insert(
                        "rider_height_max".into(),
                        json!(ruby::round_to_tens(maximum)),
                    );
                    estimated.insert("rider_height_estimated".into(), json!(true));
                    estimated
                }
                _ => comparison.clone(),
            }
        })
        .collect()
}

fn stating(size: &[Value; 4]) -> bool {
    size.iter().all(|value| truthy(Some(value)).is_some())
}

fn estimable(sizes: &[[Value; 4]]) -> bool {
    sizes.len() > 1
        && sizes.iter().all(|size| {
            truthy(Some(&size[0])).is_some()
                && truthy(Some(&size[1])).is_some()
                && size[2..].iter().all(|bound| truthy(Some(bound)).is_none())
        })
}

fn fit_bounds(models: &[&Vec<[Value; 4]>]) -> Option<[[f64; 3]; 2]> {
    let stating: Vec<Vec<&[Value; 4]>> = models
        .iter()
        .map(|sizes| {
            sizes
                .iter()
                .filter(|size| stating(size))
                .collect::<Vec<_>>()
        })
        .filter(|sizes| !sizes.is_empty())
        .collect();
    if stating.len() < MINIMUM_MODELS {
        return None;
    }
    let points: Vec<&[Value; 4]> = stating.into_iter().flatten().collect();
    Some([2, 3].map(|bound| {
        regression(
            &points
                .iter()
                .map(|size| [&size[0], &size[1], &size[bound]])
                .collect::<Vec<_>>(),
        )
    }))
}

/// Least squares of [x1, x2, y] points, as [intercept, x1 slope, x2 slope]
fn regression(points: &[[&Value; 3]]) -> [f64; 3] {
    let size = points.len() as f64;
    let means: [f64; 3] =
        [0, 1, 2].map(|column| float(&ruby::sum(points.iter().map(|point| point[column]))) / size);
    let centered: Vec<[f64; 3]> = points
        .iter()
        .map(|point| [0, 1, 2].map(|column| float(point[column]) - means[column]))
        .collect();
    let sum =
        |i: usize, j: usize| ruby::float_sum(0.0, centered.iter().map(|point| point[i] * point[j]));
    let determinant = sum(0, 0) * sum(1, 1) - sum(0, 1).powi(2);
    let slopes = [
        (sum(1, 1) * sum(0, 2) - sum(0, 1) * sum(1, 2)) / determinant,
        (sum(0, 0) * sum(1, 2) - sum(0, 1) * sum(0, 2)) / determinant,
    ];
    [
        means[2] - slopes[0] * means[0] - slopes[1] * means[1],
        slopes[0],
        slopes[1],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classifications() -> Classifications {
        Classifications::new(
            json!({"evc/us/class-1": {"max_speed": 32.19}})
                .as_object()
                .unwrap()
                .clone(),
        )
    }

    #[test]
    fn medians_over_sizes_and_stated_values() {
        let vehicle = json!({
            "propulsion": ["pedal-assist"],
            "motors": [{"kind": "mid-drive", "battery": {"capacity": 500}, "operating_modes": [{"mode": "assist", "e_vehicle_classifications": ["evc/us/class-1"]}]}],
            "frame": {"material": "aluminum", "mounts": [{"type": "rack"}, {"type": "bottle", "sizes": ["L"]}, {"type": "bottle"}]},
            "wheels": [{"position": "front", "bsd": 622, "tire_width": 40}, {"position": "rear", "bsd": 622, "tire_width": 40}],
            "sizes": [
                {"name": "M", "geometry": {"head_angle": 70, "stack": 600, "reach": 400, "chainstay": 450, "bb_height": 300, "rider_height_min": 1600}},
                {"name": "L", "geometry": {"head_angle": 71, "stack": 620, "reach": 410, "chainstay": 450, "bb_height": 300, "rider_height_min": 1700}},
            ],
        });
        let comparison = call(&vehicle, &classifications());
        assert_eq!(comparison["head_angle"], json!(70.5));
        assert_eq!(comparison["stack_reach"], json!(1.506));
        assert_eq!(comparison["bb_drop"], json!(51.0));
        assert_eq!(comparison["chainstay_gap"], json!(99.0));
        assert_eq!(comparison["bottle_mounts"], json!(1.5));
        assert_eq!(comparison["assist_speed"], json!(32));
        assert_eq!(comparison["mounts"], json!(["bottle", "rack"]));
        assert_eq!(comparison["rider_height_min"], json!(1600));
        assert!(comparison.get("rider_height_max").is_none());
        assert!(comparison.get("size_rider_height_max").is_none());
        assert_eq!(comparison["size_names"], json!(["M", "L"]));
        assert_eq!(comparison["modes"], json!(["assist"]));
    }

    #[test]
    fn rider_heights_are_estimated_only_from_a_family_with_enough_stated() {
        let stated = |offset: i64| {
            vec![
                json!([500 + offset, 400, 1500 + 2 * offset, 1700 + 2 * offset]),
                json!([550 + offset, 420, 1600 + 2 * offset, 1800 + 2 * offset]),
            ]
            .into_iter()
            .map(|size| [0, 1, 2, 3].map(|index| size[index].clone()))
            .collect::<Vec<_>>()
        };
        let unstated = vec![
            [json!(500), json!(400), Value::Null, Value::Null],
            [json!(560), json!(430), Value::Null, Value::Null],
        ];
        let rows = (0..MINIMUM_MODELS as i64)
            .map(|offset| (Some("road".to_string()), Map::new(), stated(offset)))
            .chain([
                (Some("road".to_string()), Map::new(), unstated.clone()),
                (Some("mtb".to_string()), Map::new(), unstated),
            ])
            .collect();
        let estimated = with_rider_heights(rows);
        let road = &estimated[MINIMUM_MODELS];
        assert_eq!(road["rider_height_estimated"], json!(true));
        assert_eq!(
            (&road["rider_height_min"], &road["rider_height_max"]),
            (&json!(1500), &json!(1820))
        );
        assert!(estimated[MINIMUM_MODELS + 1].is_empty());
    }
}
