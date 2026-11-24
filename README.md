# Bike Book CI

The checks for [Bike Book](https://github.com/bikeindex/bikebook)'s data, and the publisher of its catalog. Bike Book's CI runs [`.github/workflows/ci.yml`](.github/workflows/ci.yml) from this repository's `main`, so a check merged here applies to Bike Book's next run.

This repository holds no data. It's a Rust crate whose tests read a Bike Book checkout and fail on any file that breaks what the catalog promises:

- **Formatting:** every file is pretty-printed with two spaces and a trailing newline.
- **Key order:** keys are in the property order of the file's schema.
- **Schemas:** each file is valid against the schema its `schema_version` names. An overlay must be valid both as stored and once resolved over its base, and a market variant once resolved. A file on an older version validates against it at `schemas/<name>/v<version>.json`.
- **Catalog:** each file's `id` matches its path, every overlay's base and every model's motors exist, and photos are keyed by the file's id. A file's `market_variants` are each checked as an overlay over it that shares its id, named by its citation key, `<its id>@<market>`. A market's model's photos are keyed by its id, or its base's, either suffixed with `-<market lower-cased>`, and a reference names a market variant by its citation key or by that suffixed id, as ids did before every market's model of a name and year shared one. A market variant that can't carry a geometry drawing, a bike not suspended at both ends, doesn't take its base's `geometry_photo`.
- **Images:** every image is served from `https://bikebook.bikeindex.org/`.
- **Representative vehicles:** Bike Book's `representative_vehicles.csv` lists vehicle models in the catalog, in alphabetical order.
- **Schema changes:** a branch that changes a schema edits no data file but the vehicles its base's `representative_vehicles.csv` lists, the components they link, those components' bases and their market overlays, which move into them as `market_variants`. This is a check of its own, the `schema_changes` job, which runs [`bin/check_schema_change`](bin/check_schema_change) on the commit under test. On a Bike Book pull request, it also fails any change to `representative_vehicles.csv`, so a change to the list only merges by hand. To run the schema check on a branch, from a Bike Book checkout: `git fetch origin main && <bikebook_ci>/bin/check_schema_change . origin/main HEAD`.

## Running the checks

```sh
BIKEBOOK_DATA=<path to a bikebook checkout> cargo test --locked --no-fail-fast
```

A data problem gets fixed in Bike Book, not here.

## Publishing the catalog

The catalog is the one the viewer at [bikeindex.org/bikebook](https://bikeindex.org/bikebook) reads. It's published only by GitHub Actions, never from a local run.

Each format of the catalog has a manifest, `manifest.<format>.json`, naming an `index`, a `vocabulary`, a `kit`, a `comparison`, a `schema_viewer` and a block of models for each maker and first year. Each of those files is named by a hash of its content, so a publish uploads only what changed. A file is deleted a day after no manifest names it.

A change to the published layout is a new format, published beside the one before it until every reader has moved to it. `.claude/skills/format-update` has the steps.

The `schema_viewer` is what Bike Index's schema viewer shows: the current schema of each document as its fields, with how many models hold each value of a vehicle model's and a few examples of them. What goes where in it is set by the rules at the top of [`src/bin/generate_catalog/schema_viewer.rs`](src/bin/generate_catalog/schema_viewer.rs). It's only rewritten when a schema changes, or those rules do, so its counts are as of that publish. What Bike Index's viewer reads of it is [`schema_viewer_contract.json`](src/bin/generate_catalog/schema_viewer_contract.json): a publish stops on a viewer that breaks it, and a test checks the viewer of every commit the checks run on.

The index lists a model's market variants, its `market_variants` and the overlays over it that state their own `markets`, as its `variants` (each an `id` and its `markets`) rather than as rows of their own. A lookup picks the variant sold in the registrant's country, and the model itself when none is.

Format 5 keys a market variant by its citation key: Ebycco's GB EB7Pro 24" is `m/ebycco/2025/eb7pro-24@GB` in its block's `models` and `incomplete` and in its file's index row's `variants`, as `{"id": "m/ebycco/2025/eb7pro-24@GB", "markets": ["GB"]}`, while the model's own `id` is its file's, `m/ebycco/2025/eb7pro-24`, which also keys the file's own model.

Display names, primary activities (with their families, priorities and short names), vehicle types, wheel sizes and component groups come from [bike_data](https://github.com/bikeindex/bike_data)'s latest commit, fetched on every run. A bike_data change is therefore published without a change here. If bike_data is missing a value, the publish stops.

Some of the catalog's hand-written values aren't in bikebook or bike_data yet: the viewer's kit, the names of enums bike_data doesn't list, currencies, filter labels, market rules, bikebook's slug aliases and its own Dirt Travel activity. Until they are ([#3](https://github.com/bikeindex/bikebook_ci/issues/3)), they're copied from `catalog_snapshot/`.

Bike Book's own publish workflow runs on each push to its `main`, separately from its checks. It calls [`start_publish.yml`](.github/workflows/start_publish.yml), which starts [`publish.yml`](.github/workflows/publish.yml) here and waits for it. If the catalog doesn't publish, Bike Book's publish workflow fails. To publish by hand, run "Publish the catalog" from this repository's Actions tab.

`publish.yml` takes no data from whatever starts it. It always publishes Bike Book's latest `main`:

1. It runs the checks against that commit.
2. It downloads the published catalog from R2 and runs `generate_catalog` over it.
3. It writes the schema viewer, in a step of its own, if a schema changed.
4. It uploads each of Bike Book's schemas to the stock photos bucket, at the URL its `$id` names, unless that version is already there, since a published version never changes.
5. It uploads the new files, then the manifests that name them, then deletes the expired files. Nothing is uploaded unless every step before it succeeded.
6. It purges the manifests from Cloudflare's cache, and fails if a minute later the edge still serves an old one. The files are gzipped, and Cloudflare caches them. A file named by its content is cached for good, and a manifest for an hour unless a publish purges it. Since a file outlives the last manifest naming it by a day, a stale manifest never names a missing file, even if the purge fails.

**The R2 keys are only ever available here.** A workflow that Bike Book calls runs as Bike Book's run, with Bike Book's secrets. So the keys live in this repository's `catalog` environment, which only `main` can use, and only the download and upload steps get them.

Bike Book holds a token that can start the publish. It needs that token because a run's own `GITHUB_TOKEN` only works on its own repository.

Setup:

- **Here:** an environment named `catalog`, deployable only from `main`, holding:
  - Secrets `R2_ACCESS_KEY_ID` and `R2_SECRET_ACCESS_KEY`, from an R2 API token with Object Read & Write on the catalog bucket only.
  - Secrets `R2_STOCK_PHOTOS_ACCESS_KEY_ID` and `R2_STOCK_PHOTOS_SECRET_ACCESS_KEY`, from an R2 API token with Object Read & Write on the stock photos bucket only, which only the schema upload gets.
  - Variable `R2_ENDPOINT` (`https://<account id>.r2.cloudflarestorage.com`). The catalog goes to the `bikebook-catalog` bucket and the schemas to `bikebook`, the one `https://bikebook.bikeindex.org/` serves, unless variables `R2_CATALOG_BUCKET` or `R2_STOCK_PHOTOS_BUCKET` name others.
  - Secret `CLOUDFLARE_PURGE_TOKEN`, from a Cloudflare API token with Zone > Cache Purge on the `bikeindex.org` zone only.
  - Variable `CLOUDFLARE_ZONE_ID`, that zone's ID.
- **In Cloudflare:** a Cache Rule that makes the catalog eligible for cache, with edge and browser TTL taken from the origin's `Cache-Control`, and a status code TTL of no-store for 404. Otherwise a 404 for a deleted file, from a tab open for over a day, would be cached, and outlast a publish that puts a file with the same contents back. Its expression:

  ```
  (http.host eq "bikebook-catalog.bikeindex.org" and any(http.request.headers["accept-encoding"][*] contains "gzip"))
  ```

  A request that doesn't accept gzip gets the file decoded, and if it filled the cache, the decoded copy would be what every browser gets from there on: `no-transform` stops Cloudflare compressing it. So only a request that accepts gzip can fill it, and any other goes to R2. A purge by URL is matched against this rule too, and as it accepts no gzip it misses the copy every browser gets, so the publish purges the manifests by prefix.
- **In Bike Book:** a secret `CATALOG_PUBLISH_TOKEN`, from a fine-grained token for this repository only, with "Actions: Read and write" and nothing else.
