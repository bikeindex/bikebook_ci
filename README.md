# Bike Book CI

The checks for [Bike Book](https://github.com/bikeindex/bikebook)'s data, and the publisher of its catalog. Bike Book's CI runs [`.github/workflows/ci.yml`](.github/workflows/ci.yml) from this repository's `main`, so a check merged here applies to Bike Book's next run.

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

Display names, primary activities (with their families, priorities and short names), vehicle types, wheel sizes and component groups come from [bike_data](https://github.com/bikeindex/bike_data)'s latest commit. It's fetched on every run and cached by commit under the temp directory, so a bike_data change is published without a change here. A value bike_data is missing stops the run.

Some of the catalog's hand-written values aren't in bikebook or bike_data yet: the viewer's kit, the names of enums bike_data doesn't list, currencies, filter labels, market rules, bikebook's slug aliases and its own Dirt Travel activity. Until they are ([#3](https://github.com/bikeindex/bikebook_ci/issues/3)), they're copied from `catalog_snapshot/`.

## Publishing the catalog

Bike Book's own publish workflow runs on each push to its `main`, separately from its checks. It calls [`start_publish.yml`](.github/workflows/start_publish.yml), which starts [`publish.yml`](.github/workflows/publish.yml) here and waits for it. If the catalog doesn't publish, Bike Book's publish workflow fails. To publish by hand, run "Publish the catalog" from this repository's Actions tab.

`publish.yml` takes no data from whatever starts it. It always publishes Bike Book's latest `main`:

1. It runs the checks against that commit.
2. It downloads the published catalog from R2 and generates over it.
3. It uploads the new files, then the manifest that names them, then deletes the expired files. Nothing is uploaded unless every step before it succeeded.

**The R2 keys are only ever available here.** A workflow that Bike Book calls runs as Bike Book's run, with Bike Book's secrets. So the keys live in this repository's `catalog` environment, which only `main` can use, and only the download and upload steps get them.

Bike Book holds a token that can start the publish. It needs that token because a run's own `GITHUB_TOKEN` only works on its own repository.

Setup:

- **Here:** an environment named `catalog`, deployable only from `main`, holding:
  - Secrets `R2_ACCESS_KEY_ID` and `R2_SECRET_ACCESS_KEY`, from an R2 API token with Object Read & Write on the catalog bucket only.
  - Variables `R2_CATALOG_BUCKET` (`bikebook-catalog`) and `R2_ENDPOINT` (`https://<account id>.r2.cloudflarestorage.com`).
- **In Bike Book:** a secret `CATALOG_PUBLISH_TOKEN`, from a fine-grained token for this repository only, with "Actions: Read and write" and nothing else.
