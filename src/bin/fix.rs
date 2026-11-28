use bikebook_data_checks::*;
use rayon::prelude::*;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let schemas = Schemas::load();
    let exit = |message: String| -> ! {
        eprintln!("{message}");
        std::process::exit(1)
    };
    let canonical = |path: PathBuf| {
        fs::canonicalize(&path).unwrap_or_else(|error| exit(format!("{}: {error}", path.display())))
    };
    let root = canonical(root().to_path_buf());
    let targets: Vec<PathBuf> = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .map(canonical)
        .inspect(|target| {
            if !target.starts_with(&root) {
                exit(format!(
                    "{} isn't in {}, the checkout BIKEBOOK_DATA names",
                    target.display(),
                    root.display()
                ))
            }
        })
        .collect();
    let results: Vec<_> = data_paths()
        .into_par_iter()
        .map(|path| (path, true))
        .chain(
            json_paths(SCHEMA_DIRECTORY)
                .into_par_iter()
                .map(|path| (path, false)),
        )
        .filter(|(path, _)| {
            targets.is_empty()
                || targets
                    .iter()
                    .any(|target| root.join(path).starts_with(target))
        })
        .map(|(path, ordered)| (fix(&schemas, &path, ordered), path))
        .collect();
    for ((changed, error), path) in &results {
        if *changed {
            println!("fixed {path}");
        }
        if let Some(error) = error {
            eprintln!("{path}: {error}");
        }
    }
    let failures = results
        .iter()
        .filter(|((_, error), _)| error.is_some())
        .count();
    let fixed = results.iter().filter(|((changed, _), _)| *changed).count();
    println!("{fixed} fixed, {failures} can't be fixed");
    if failures > 0 {
        std::process::exit(1);
    }
}

/// Whether the file changed, and why it can't be fixed: one whose schema can't be found is still pretty-printed
fn fix(schemas: &Schemas, path: &str, ordered: bool) -> (bool, Option<String>) {
    let file = root().join(path);
    let (text, value) = match read(&file) {
        Ok(read) => read,
        Err(error) => return (false, Some(error)),
    };
    let (value, error) = match ordered.then(|| schemas.document_for(path, &value)) {
        Some(Ok(document)) => (schemas.in_key_order(&document, &value), None),
        Some(Err(error)) => (value, Some(error)),
        None => (value, None),
    };
    let fixed = pretty(&value);
    if fixed == text {
        return (false, error);
    }
    // Renamed over it, so an interrupted run can't leave it half-written
    let temporary = file.with_extension("json.fixing");
    match fs::write(&temporary, fixed).and_then(|()| fs::rename(&temporary, &file)) {
        Ok(()) => (true, error),
        Err(write) => (false, Some(write.to_string())),
    }
}

fn read(file: &Path) -> Result<(String, Value), String> {
    let text = fs::read_to_string(file).map_err(|error| error.to_string())?;
    let value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    Ok((text, value))
}
