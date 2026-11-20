# Bike Book CI

The checks for [Bike Book](https://github.com/bikeindex/bikebook)'s data, and the publisher of its catalog. Bike Book's CI runs [`.github/workflows/ci.yml`](.github/workflows/ci.yml) from this repository's `main`, so a check merged here applies to Bike Book's next run.

This repository holds no data. It's a Rust crate whose tests read a Bike Book checkout and fail on any file that breaks what the catalog promises:

- **Formatting:** every file is pretty-printed with two spaces and a trailing newline.
- **Key order:** keys are in the property order of the file's schema.
- **Schemas:** each file is valid against the schema its `schema_version` names. An overlay must be valid both as stored and once resolved over its base.
- **Catalog:** each file's `id` matches its path, every overlay's base and every model's motors exist, and photos are keyed by the file's id.
- **Images:** every image is served from `https://bikebook.bikeindex.org/`.
- **Representative vehicles:** Bike Book's `representative_vehicles.csv` lists vehicle models in the catalog, in alphabetical order.
- **Schema changes:** a branch that changes a schema edits no data file but the vehicles its base's `representative_vehicles.csv` lists, the components they link and those components' bases. This is a check of its own, the `schema_changes` job, which runs [`bin/check_schema_change`](bin/check_schema_change) on the commit under test. On a Bike Book pull request, it also fails any change to `representative_vehicles.csv`, so a change to the list only merges by hand. To run the schema check on a branch, from a Bike Book checkout: `git fetch origin main && <bikebook_ci>/bin/check_schema_change . origin/main HEAD`.

## Running the checks

```sh
BIKEBOOK_DATA=<path to a bikebook checkout> cargo test --locked --no-fail-fast
```

A data problem gets fixed in Bike Book, not here.

## Publishing the catalog

The catalog is the one the viewer at [bikeindex.org/bikebook](https://bikeindex.org/bikebook) reads. It's published only by GitHub Actions, never from a local run.

Each format of the catalog has a manifest, `manifest.<format>.json`, naming an `index`, a `vocabulary`, a `kit`, a `comparison`, the `components` (each by its id, an overlay resolved over its base) and a block of models for each maker and first year. Format 3's is also at `manifest.json`, where its readers fetch it. Each of those files is named by a hash of its content, so a publish uploads only what changed. A file is deleted a day after no manifest names it.

A change to the published layout is a new format, published beside the one before it until every reader has moved to it. `.claude/skills/format-update` has the steps.

Format 4 lists a model's market variants, the overlays over it that state their own `markets`, as its `variants` (each an `id` and its `markets`) rather than as index rows of their own. A lookup picks the variant sold in the registrant's country, and the model itself when none is.

Display names, primary activities (with their families, priorities and short names), vehicle types, wheel sizes and component groups come from [bike_data](https://github.com/bikeindex/bike_data)'s latest commit, fetched on every run. A bike_data change is therefore published without a change here. If bike_data is missing a value, the publish stops.

Some of the catalog's hand-written values aren't in bikebook or bike_data yet: the viewer's kit, the names of enums bike_data doesn't list, currencies, filter labels, market rules, bikebook's slug aliases and its own Dirt Travel activity. Until they are ([#3](https://github.com/bikeindex/bikebook_ci/issues/3)), they're copied from `catalog_snapshot/`.

Bike Book's own publish workflow runs on each push to its `main`, separately from its checks. It calls [`start_publish.yml`](.github/workflows/start_publish.yml), which starts [`publish.yml`](.github/workflows/publish.yml) here and waits for it. If the catalog doesn't publish, Bike Book's publish workflow fails. To publish by hand, run "Publish the catalog" from this repository's Actions tab.

`publish.yml` takes no data from whatever starts it. It always publishes Bike Book's latest `main`:

1. It runs the checks against that commit.
2. It downloads the published catalog from R2 and runs `generate_catalog` over it.
3. It uploads the new files, then the manifests that name them, then deletes the expired files. Nothing is uploaded unless every step before it succeeded.
4. It purges the manifests from Cloudflare's cache. The files are gzipped, and Cloudflare caches them. A file named by its content is cached for good, and a manifest for an hour unless a publish purges it. Since a file outlives the last manifest naming it by a day, a stale manifest never names a missing file, even if the purge fails.

**The R2 keys are only ever available here.** A workflow that Bike Book calls runs as Bike Book's run, with Bike Book's secrets. So the keys live in this repository's `catalog` environment, which only `main` can use, and only the download and upload steps get them.

Bike Book holds a token that can start the publish. It needs that token because a run's own `GITHUB_TOKEN` only works on its own repository.

Setup:

- **Here:** an environment named `catalog`, deployable only from `main`, holding:
  - Secrets `R2_ACCESS_KEY_ID` and `R2_SECRET_ACCESS_KEY`, from an R2 API token with Object Read & Write on the catalog bucket only.
  - Variables `R2_CATALOG_BUCKET` (`bikebook-catalog`) and `R2_ENDPOINT` (`https://<account id>.r2.cloudflarestorage.com`).
  - Secret `CLOUDFLARE_PURGE_TOKEN`, from a Cloudflare API token with Zone > Cache Purge on the `bikeindex.org` zone only.
  - Variable `CLOUDFLARE_ZONE_ID`, that zone's ID.
- **In Cloudflare:** a Cache Rule that makes the catalog eligible for cache, with edge and browser TTL taken from the origin's `Cache-Control`, and a status code TTL of no-store for 404. Otherwise a 404 for a deleted file, from a tab open for over a day, would be cached, and outlast a publish that puts a file with the same contents back. Its expression:

  ```
  (http.host eq "bikebook-catalog.bikeindex.org" and any(http.request.headers["accept-encoding"][*] contains "gzip"))
  ```

  A request that doesn't accept gzip gets the file decoded, and if it filled the cache, the decoded copy would be what every browser gets from there on: `no-transform` stops Cloudflare compressing it. So only a request that accepts gzip can fill it, and any other goes to R2.
- **In Bike Book:** a secret `CATALOG_PUBLISH_TOKEN`, from a fine-grained token for this repository only, with "Actions: Read and write" and nothing else.

## The API

`https://catalog.bikeindex.org/v1/models` finds the catalog's models by manufacturer, model, year and market, and names each one's slugs, so a registration can be linked to its Bike Book model. It's a Cloudflare Worker, [`api/`](api/), that reads format 4's manifest and index as published, so it answers from the latest publish without a deploy of its own.

It takes a `manufacturer`, a `model` or both, and optionally a `year` and a `market`:

- **`manufacturer`** finds a maker as Bike Index's `Manufacturer.friendly_find` does, by its Bike Index slug (`Surly Bikes` is `surly-bikes`) or its legacy one (`Surly Bicycles` is `surly`), or by Bike Book's (its directory). A manufacturer's `slug` is its Bike Index one.
- **`model`** matches a model named the same, by Bike Book's slug, or one whose name starts with its words: `Fuel EX 8` finds `Fuel EX 8 Gen 5` too. The exact names come first, each with `exact`.
- **`year`** matches a model from its first year through its final year, or on, while it's current.
- **`market`** picks a model's variant sold in it, in its place, as format 4's index describes. It filters nothing out.

The response repeats the query with the slugs it made, then lists each model with its `id`, its manufacturer's and model's `name` and `slug`, its `first_year`, `final_year`, `markets` and `variants`:

```sh
curl 'https://catalog.bikeindex.org/v1/models?manufacturer=Trek&model=Fuel%20EX%208&year=2022'
```

`https://catalog.bikeindex.org/v1/<id>` returns the record with that id: a vehicle model's (`m/…`) from its block, a component's (`c/…`) from the `components`, or an e-vehicle classification's (`evc/…`) from the vocabulary. It names the `kind`, and for a model or component its manufacturer's and model's `name` and `slug`, as the search does:

```sh
curl 'https://catalog.bikeindex.org/v1/evc/us/class_3'
```

Its tests run with `npm test` in `api/`. [`api.yml`](.github/workflows/api.yml) runs them on a change to `api/`, and deploys the Worker from `main`.

Setup:

- **Here:** an environment named `api`, deployable only from `main`, holding secret `CLOUDFLARE_WORKERS_TOKEN`, from a Cloudflare API token with Account > Workers Scripts > Edit, and Zone > Workers Routes > Edit and Zone > DNS > Edit on the `bikeindex.org` zone, and variable `CLOUDFLARE_ACCOUNT_ID`. It holds no R2 key: the Worker reads the catalog as any reader does.
- **In Cloudflare:** the Workers Paid plan, since the first request to each instance parses the 24 MB index, well past the free plan's 10 ms of CPU. The deploy adds `catalog.bikeindex.org` as the Worker's custom domain.
