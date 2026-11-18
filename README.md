# Bike Book CI

The checks for [Bike Book](https://github.com/bikeindex/bikebook)'s data. Bike Book's CI runs [`.github/workflows/ci.yml`](.github/workflows/ci.yml) from this repository's `main`, so a check merged here applies to Bike Book's next run.

This repository holds no data. It's a Rust crate whose tests read a Bike Book checkout and fail on any file that breaks what the catalog promises:

- **Formatting:** every file is pretty-printed with two spaces and a trailing newline.
- **Key order:** keys are in the property order of the file's schema.
- **Schemas:** each file is valid against the schema its `schema_version` names. An overlay must be valid both as stored and once resolved over its base.
- **Catalog:** each file's `id` matches its path, every overlay's base and every model's motors exist, and photos are keyed by the file's id.
- **Images:** every image is served from `https://bikebook.bikeindex.org/`.

## Running the checks

```sh
BIKEBOOK_DATA=<path to a bikebook checkout> cargo test --locked --no-fail-fast
```

A data problem gets fixed in Bike Book, not here.

## Generating the catalog

```sh
BIKEBOOK_DATA=<path to a bikebook checkout> cargo run --release --bin generate_catalog -- <output directory>
```

This writes the catalog the viewer at [bikeindex.org/bikebook](https://bikeindex.org/bikebook) reads. `manifest.json` names an `index`, a `vocabulary`, a `kit`, a `comparison` and a block of models for each maker and first year. Each of those files is named by a hash of its content. A rerun into the same directory writes only what changed, and deletes a file a day after the manifest stops naming it.

Names, primary activities, vehicle types, wheel sizes and component groups come from [bike_data](https://github.com/bikeindex/bike_data)'s latest commit, fetched each run and cached by commit under the temp directory, so a bike_data change is published without a change here.

Some of the catalog's hand-written values aren't in bikebook or bike_data yet: the viewer's kit, the names of enums bike_data doesn't list, currencies, filter labels, market rules, and bikebook's slug aliases, family short names and own Dirt Travel activity (`bike_data_additions.json`). Until they are ([#3](https://github.com/bikeindex/bikebook_ci/issues/3)), they're copied from `catalog_snapshot/`.
