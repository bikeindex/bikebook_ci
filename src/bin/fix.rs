use bikebook_data_checks::*;
use rayon::prelude::*;
use std::fs;

fn main() {
    let schemas = Schemas::load();
    let results: Vec<_> = data_paths()
        .into_par_iter()
        .map(|path| (path, true))
        .chain(
            json_paths(SCHEMA_DIRECTORY)
                .into_par_iter()
                .map(|path| (path, false)),
        )
        .map(|(path, ordered)| (fix(&schemas, &path, ordered), path))
        .collect();
    for (result, path) in &results {
        match result {
            Ok(true) => println!("fixed {path}"),
            Err(error) => eprintln!("{path}: {error}"),
            Ok(false) => {}
        }
    }
    let failures = results.iter().filter(|(result, _)| result.is_err()).count();
    let fixed = results
        .iter()
        .filter(|(result, _)| *result == Ok(true))
        .count();
    println!("{fixed} fixed, {failures} can't be fixed");
    if failures > 0 {
        std::process::exit(1);
    }
}

fn fix(schemas: &Schemas, path: &str, ordered: bool) -> Result<bool, String> {
    let file = root().join(path);
    let text = fs::read_to_string(&file).map_err(|error| error.to_string())?;
    let value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let value = match ordered {
        true => {
            schemas
                .in_key_order(&schemas.document_for(path, &value)?, &value)
                .0
        }
        false => value,
    };
    let fixed = pretty(&value);
    let changed = fixed != text;
    if changed {
        fs::write(&file, fixed).map_err(|error| error.to_string())?;
    }
    Ok(changed)
}
