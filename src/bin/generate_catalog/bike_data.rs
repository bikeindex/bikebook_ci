//! bike_data's latest commit, which the catalog's names, primary activities and wheel sizes come from

use bikebook_data_checks::dashed;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::LazyLock;

const REPOSITORY: &str = "https://github.com/bikeindex/bike_data";

static PARENTHESES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\([^)]*\)").unwrap());

type Row = HashMap<String, String>;

pub struct BikeData {
    pub commit: String,
    directory: PathBuf,
    /// What bike_data doesn't hold
    additions: Value,
    /// Its slugs spelled as schema 2.0.0 does
    dashes: bool,
}

pub struct Activity {
    pub slug: String,
    pub name: String,
    pub family: String,
}

fn git(arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .output()
        .expect("git isn't installed");
    assert!(
        output.status.success(),
        "git {}: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

impl BikeData {
    /// Cloned once per commit
    pub fn fetch(dashes: bool) -> BikeData {
        let cache = std::env::temp_dir().join("bike_data");
        let latest = git(&["ls-remote", REPOSITORY, "HEAD"])
            .split_whitespace()
            .next()
            .expect("bike_data has no HEAD")
            .to_string();
        let directory = if cache.join(&latest).exists() {
            cache.join(&latest)
        } else {
            let partial = cache.join(format!("{latest}.{}", std::process::id()));
            let partial_path = partial.to_str().unwrap();
            git(&["clone", "--quiet", "--depth=1", REPOSITORY, partial_path]);
            // main can move between ls-remote and the clone
            let cloned = cache.join(git(&["-C", partial_path, "rev-parse", "HEAD"]));
            if fs::rename(&partial, &cloned).is_err() {
                fs::remove_dir_all(&partial).unwrap();
            }
            cloned
        };
        BikeData::new(directory, dashes)
    }

    fn new(directory: PathBuf, dashes: bool) -> BikeData {
        BikeData {
            commit: directory.file_name().unwrap().to_string_lossy().to_string(),
            directory,
            additions: serde_json::from_str(include_str!(
                "../../../catalog_snapshot/bike_data_additions.json"
            ))
            .unwrap(),
            dashes,
        }
    }

    fn rows(&self, file: &str) -> Vec<Row> {
        let path = self.directory.join("data").join(file);
        csv::Reader::from_path(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
            .deserialize()
            .map(|row| row.unwrap_or_else(|error| panic!("{file}: {error}")))
            .collect()
    }

    /// Bike Index's slug for a name
    fn slugify(&self, name: &str) -> String {
        let slug = PARENTHESES
            .replace_all(name, "")
            .to_lowercase()
            .replace('&', " and ")
            .replace('+', " plus ")
            .split(|character: char| !character.is_ascii_alphanumeric())
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>()
            .join("_");
        let slug = self.additions["slug_aliases"][&slug]
            .as_str()
            .map_or(slug, str::to_string);
        if self.dashes { dashed(&slug) } else { slug }
    }

    fn named(&self, name: &str) -> (String, String) {
        (self.slugify(name), name.to_string())
    }

    /// Bike Index's ordering: by priority, then as created, a flavor once under each of its families
    pub fn primary_activities(&self) -> Vec<Activity> {
        let rows = self.rows("primary_activities.csv");
        let short_names: HashMap<&str, &str> = rows
            .iter()
            .filter(|row| row["flavor"].trim().is_empty())
            .map(|row| (row["families"].trim(), row["short_name"].as_str()))
            .collect();
        let mut created: Vec<(&str, Option<&str>, i64)> = rows
            .iter()
            .flat_map(|row| {
                let families: Vec<&str> = row["families"]
                    .split('&')
                    .map(str::trim)
                    .filter(|family| !family.is_empty())
                    .collect();
                let flavor = row["flavor"].trim();
                let priority = row
                    .get("priority")
                    .and_then(|priority| priority.parse().ok())
                    .unwrap_or_else(|| {
                        panic!("primary_activities.csv: {flavor:?} {families:?} has no priority")
                    });
                match flavor {
                    "" => families
                        .into_iter()
                        .map(|family| (family, None, priority))
                        .collect(),
                    flavor if families.is_empty() => vec![(flavor, None, priority)],
                    flavor => families
                        .into_iter()
                        .map(|family| (flavor, Some(family), priority))
                        .collect::<Vec<_>>(),
                }
            })
            .chain(
                self.additions["primary_activities"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(name, priority)| (name.as_str(), None, priority.as_i64().unwrap())),
            )
            .collect();
        created.sort_by_key(|(_, _, priority)| std::cmp::Reverse(*priority));
        created
            .into_iter()
            .map(|(name, family, _)| {
                let name = family.map_or(name.to_string(), |family| {
                    format!("{}: {name}", short_names[family])
                });
                Activity {
                    slug: self.slugify(&name),
                    family: self.slugify(family.unwrap_or(&name)),
                    name,
                }
            })
            .collect()
    }

    /// Each name bike_data gives a vehicle model field's slugs, by field
    pub fn names(&self, activities: &[Activity]) -> HashMap<&'static str, HashMap<String, String>> {
        let attributes = self.rows("vehicle_attributes.csv");
        let attribute = |kind: &str| {
            attributes
                .iter()
                .filter(|row| row["attribute"] == kind)
                .map(|row| self.named(&row["name"]))
                .collect()
        };
        let slugged = |file| {
            self.rows(file)
                .into_iter()
                .map(|row| (row["slug"].clone(), row["name"].clone()))
                .collect()
        };
        let named = |file| {
            self.rows(file)
                .iter()
                .map(|row| self.named(&row["name"]))
                .collect()
        };
        HashMap::from([
            ("type", attribute("vehicle_type")),
            ("propulsion", attribute("propulsion_type")),
            ("frame.material", attribute("frame_material")),
            ("motors.kind", attribute("motor_placement")),
            ("handlebar_type", slugged("handlebar_types.csv")),
            ("years.colorways.colors", slugged("colors.csv")),
            ("brakes.type", named("brake_types.csv")),
            ("components.type", named("component_types.csv")),
            ("drivetrain", self.drivetrain()),
            (
                "primary_activity",
                activities
                    .iter()
                    .map(|activity| (activity.slug.clone(), activity.name.clone()))
                    .collect(),
            ),
        ])
    }

    /// A name both with its position and without, since the schema's slug for Fixed or Belt Drive has none
    fn drivetrain(&self) -> HashMap<String, String> {
        self.rows("drivetrain_attributes.csv")
            .iter()
            .flat_map(|row| {
                let name = &row["name"];
                let position = &row["position (front/rear/both)"];
                let qualified = (position != "both")
                    .then(|| format!("{name} {}{}", position[..1].to_uppercase(), &position[1..]));
                [qualified, Some(name.clone())]
                    .into_iter()
                    .flatten()
                    .map(|name| self.named(&name))
            })
            .collect()
    }

    pub fn wheel_sizes(&self) -> Map<String, Value> {
        let sizes: BTreeMap<i64, Value> = self
            .rows("wheel_sizes.csv")
            .iter()
            .map(|row| {
                (
                    row["iso_bsd"].parse().unwrap(),
                    json!({
                        "name": row["name"],
                        "description": (!row["description"].is_empty()).then(|| &row["description"]),
                        "standard": row["priority"] == "standard",
                    }),
                )
            })
            .collect();
        sizes
            .into_iter()
            .map(|(iso_bsd, size)| (iso_bsd.to_string(), size))
            .collect()
    }

    /// Each group, in the kit's order, of the component types in `order`
    pub fn component_groups(&self, groups: &Value, order: &[&str]) -> Map<String, Value> {
        let rows = self.rows("component_types.csv");
        let by_slug: HashMap<String, &Row> = rows
            .iter()
            .map(|row| (self.slugify(&row["name"]), row))
            .collect();
        groups
            .as_array()
            .unwrap()
            .iter()
            .map(|group| {
                let group = group.as_str().unwrap();
                let members = order
                    .iter()
                    .filter_map(|slug| by_slug.get(*slug))
                    .filter(|row| row["group"] == group)
                    .map(|row| json!(row["name"]))
                    .collect();
                (group.to_string(), Value::Array(members))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bike_data(files: &[(&str, &str)]) -> BikeData {
        static TESTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let test = TESTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("bike_data_test_{}_{test}", std::process::id()));
        fs::create_dir_all(directory.join("data")).unwrap();
        files
            .iter()
            .for_each(|(file, body)| fs::write(directory.join("data").join(file), body).unwrap());
        BikeData::new(directory, false)
    }

    #[test]
    fn slugify_drops_parentheses_and_applies_aliases() {
        let data = bike_data(&[]);
        assert_eq!(data.slugify("Cross-Country (XC)"), "cross_country");
        assert_eq!(data.slugify("Bell/Noisemaker"), "bell_noisemaker");
        assert_eq!(data.slugify("Carbon or Composite"), "carbon");
    }

    #[test]
    fn slugify_spells_a_slug_as_schema_2_0_0_does() {
        let data = BikeData {
            dashes: true,
            ..bike_data(&[])
        };
        assert_eq!(data.slugify("Cross-Country (XC)"), "cross-country");
        assert_eq!(data.slugify("Bell/Noisemaker"), "bellnoisemaker");
        assert_eq!(data.slugify("Carbon or Composite"), "composite");
        assert_eq!(
            data.slugify("6 Speed Pinion Gearbox Front"),
            "6-speed-pinion-gearbox"
        );
    }

    #[test]
    fn primary_activities_order_by_priority_and_name_flavors_by_family() {
        let data = bike_data(&[(
            "primary_activities.csv",
            "flavor,families,priority,short_name\n,Road Biking,590,Road\n,MTB (Mountain Biking),595,MTB\nBikepacking,MTB (Mountain Biking) & Road Biking,280,\nBike Polo,,401,Bike Polo\n",
        )]);
        let activities = data.primary_activities();
        let activities: Vec<[&str; 3]> = activities
            .iter()
            .map(|activity| [&*activity.slug, &activity.name, &activity.family])
            .collect();
        assert_eq!(
            activities,
            [
                ["mtb", "MTB (Mountain Biking)", "mtb"],
                ["road_biking", "Road Biking", "road_biking"],
                ["bike_polo", "Bike Polo", "bike_polo"],
                ["mtb_bikepacking", "MTB: Bikepacking", "mtb"],
                ["road_bikepacking", "Road: Bikepacking", "road_biking"],
                ["dirt_travel", "Dirt Travel", "dirt_travel"],
            ]
        );
    }

    #[test]
    fn drivetrain_names_with_and_without_position() {
        let data = bike_data(&[(
            "drivetrain_attributes.csv",
            "position (front/rear/both),name,count,internal\nfront,1,1,false\nrear,Fixed,1,false\nboth,Belt Drive,1,false\nfront,6 Speed Pinion Gearbox,6,true\n",
        )]);
        let names = data.drivetrain();
        assert_eq!(names["1_front"], "1 Front");
        assert_eq!(names["fixed"], "Fixed");
        assert_eq!(names["belt_drive"], "Belt Drive");
        assert_eq!(names["6_speed_pinion"], "6 Speed Pinion Gearbox Front");
    }

    #[test]
    #[should_panic(expected = "has no priority")]
    fn primary_activities_need_a_priority() {
        bike_data(&[(
            "primary_activities.csv",
            "flavor,families,priority,short_name\nBike Polo,,,Bike Polo\n",
        )])
        .primary_activities();
    }

    #[test]
    fn wheel_sizes_order_by_iso_bsd() {
        let data = bike_data(&[(
            "wheel_sizes.csv",
            "name,iso_bsd,priority,description\n700c,622,standard,700c road\n6in,90,uncommon,\n3in,76,rare,3in solid\n",
        )]);
        let sizes = data.wheel_sizes();
        assert_eq!(sizes.keys().collect::<Vec<_>>(), ["76", "90", "622"]);
        assert_eq!(
            sizes["622"],
            json!({"name": "700c", "description": "700c road", "standard": true})
        );
        assert_eq!(sizes["90"]["description"], Value::Null);
    }
}
