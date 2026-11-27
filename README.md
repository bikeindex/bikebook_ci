# Bike Book CI

The checks for [Bike Book](https://github.com/bikeindex/bikebook)'s data, and the publisher of its catalog. Bike Book's CI runs [`ci.yml`](.github/workflows/ci.yml) from this repository's `main`, so a check merged here applies to Bike Book's next run.

This repository holds no data. Its tests read a Bike Book checkout and fail on any file that breaks what the catalog promises:

- **Formatting:** pretty-printed with two spaces and a trailing newline.
- **Key order:** keys in the order of the file's schema.
- **Schemas:** valid against the schema its `schema_version` names (`schemas/<name>/v<version>.json` for an older one), with each market variant resolved over it.
- **Catalog:** each `id` matches its path, every model's motors exist, and photos are keyed by the file's id. A market variant is checked as an overlay named by its citation key, `<id>@<market>`, and its photos are keyed by that id, alone or suffixed `-<market lower-cased>`. A reference can name a variant by its citation key, another market it states, or the suffixed id. A variant that can't carry a geometry drawing (a bike not suspended at both ends) doesn't take its file's `geometry_photo`.
- **Images:** served from `https://bikebook.bikeindex.org/`.
- **Representative vehicles:** `representative_vehicles.csv` lists catalog models, alphabetically.
- **Schema changes:** a branch that changes a schema edits no data file but the vehicles its base's `representative_vehicles.csv` lists and the components they link. On a Bike Book pull request, any change to `representative_vehicles.csv` fails too, so the list only changes by a hand merge. CI runs it as the `schema_changes` job, with [`bin/check_schema_change`](bin/check_schema_change).

## Running the checks

Clone this into a Bike Book checkout (Bike Book's `.gitignore` ignores `bikebook_ci/`), install [Rust](https://rustup.rs), and from the checkout:

```sh
bikebook_ci/bin/test # run tests

# To check another checkout, such as a worktree, name it in BIKEBOOK_DATA
BIKEBOOK_DATA=../bikebook-worktree bikebook_ci/bin/test
```

A data problem is fixed in Bike Book, not here. `--fix` rewrites what the formatting and key order checks report, in every file or only those given. A duplicate key keeps its last value, so read the diff.

```sh
bikebook_ci/bin/test --fix
bikebook_ci/bin/test --fix vehicle_models/trek
```

To run the schema change check on a branch:

```sh
git fetch origin main && bikebook_ci/bin/check_schema_change . origin/main HEAD
```

## Publishing the catalog

The catalog is what [bikeindex.org/bikebook](https://bikeindex.org/bikebook) reads. Only GitHub Actions publishes it.

Each format has a manifest, `manifest.<format>.json`, naming an `index`, a `vocabulary` (which holds the viewer's `kit`), a `comparison`, a `schema_viewer`, and a block of models per maker and first year. Every file is named by a hash of its content, so a publish uploads only what changed, and a file is deleted a day after no manifest names it. A change to the layout is a new format, published beside the old one until every reader has moved: see `.claude/skills/format-update`.

The `schema_viewer` is what Bike Index's schema viewer shows: each document's current schema as fields, with how many models hold each value and a few examples. Its rules are at the top of [`schema_viewer.rs`](src/bin/generate_catalog/schema_viewer.rs). It's only rewritten when a schema or those rules change, so its counts are as of that publish. What Bike Index reads of it is [`schema_viewer_contract.json`](src/bin/generate_catalog/schema_viewer_contract.json): a publish stops on a viewer that breaks it, and the checks test it on every commit.

### Market variants

The index lists a model's `market_variants` that state their own `markets` as its `variants` (each an `id` and its `markets`), not as rows of their own. A lookup picks the variant sold in the registrant's country, else the model itself.

Format 6 keys a variant by its citation key. Ebycco's GB EB7Pro 24" is `m/ebycco/2025/eb7pro-24@GB` in its block's `models` and `incomplete` and in its index row's `variants`, while the model's own `id` stays `m/ebycco/2025/eb7pro-24`. A variant stating other markets is also published under each of their keys, listed after the variants in `variants` and keyed to the same model in `models` and `incomplete`: Cannondale's JP SuperSix EVO Hi-MOD Disc Ultegra, sold in Japan and Switzerland, is both `m/cannondale/2020/supersix-evo-hi-mod-disc-ultegra@JP` and `…@CH`. Where two variants state a market, the first takes its key.

### Where the values come from

Display names, primary activities, vehicle types, wheel sizes and component groups come from [bike_data](https://github.com/bikeindex/bike_data)'s latest commit on every run, so a bike_data change publishes without a change here. A value missing there stops the publish.

Hand-written values bikebook and bike_data don't hold yet (the kit, enum names bike_data doesn't list, currencies, filter labels, market rules, slug aliases, the Dirt Travel activity) are copied from `catalog_snapshot/` until they do ([#3](https://github.com/bikeindex/bikebook_ci/issues/3)).

### How a publish runs

Each push to Bike Book's `main` calls [`start_publish.yml`](.github/workflows/start_publish.yml), which starts [`publish.yml`](.github/workflows/publish.yml) here and waits, so Bike Book's publish fails if the catalog doesn't. To publish by hand, run "Publish the catalog" from this repository's Actions tab.

`publish.yml` takes nothing from whatever starts it, and always publishes Bike Book's latest `main`:

1. Runs the checks.
2. Downloads the published catalog from R2 and runs `generate_catalog` over it.
3. Writes the schema viewer, if a schema changed.
4. Uploads each schema to the stock photos bucket at its `$id`, unless that version is already there.
5. Uploads the new files, then the manifests, then deletes expired files. Nothing uploads unless every earlier step passed.
6. Purges the manifests from Cloudflare's cache, and fails if the edge still serves an old one a minute later. Content-named files are cached for good and manifests for an hour, and a file outlives its last manifest by a day, so a stale manifest never names a missing file.

**The R2 keys stay here.** A workflow Bike Book calls runs with Bike Book's secrets, so the keys live in this repository's `catalog` environment, which only `main` can use, and only the download and upload steps get them. Bike Book holds a token that can start the publish, since a run's `GITHUB_TOKEN` only works on its own repository.

### Setup

- **Here:** an environment `catalog`, deployable only from `main`, with:
  - Secrets `R2_ACCESS_KEY_ID` and `R2_SECRET_ACCESS_KEY`: an R2 token with Object Read & Write on the catalog bucket only.
  - Secrets `R2_STOCK_PHOTOS_ACCESS_KEY_ID` and `R2_STOCK_PHOTOS_SECRET_ACCESS_KEY`: an R2 token with Object Read & Write on the stock photos bucket only, used only by the schema upload.
  - Variable `R2_ENDPOINT` (`https://<account id>.r2.cloudflarestorage.com`). The buckets are `bikebook-catalog` and `bikebook` unless `R2_CATALOG_BUCKET` or `R2_STOCK_PHOTOS_BUCKET` name others.
  - Secret `CLOUDFLARE_PURGE_TOKEN`: a Cloudflare token with Zone > Cache Purge on `bikeindex.org` only.
  - Variable `CLOUDFLARE_ZONE_ID`: that zone's ID.
- **In Cloudflare:** a Cache Rule making the catalog eligible for cache, with edge and browser TTL from the origin's `Cache-Control`, and no-store for 404s, so a deleted file's cached 404 can't outlast a publish that restores it:

  ```
  (http.host eq "bikebook-catalog.bikeindex.org" and any(http.request.headers["accept-encoding"][*] contains "gzip"))
  ```

  Only a request accepting gzip can fill the cache. Otherwise a decoded copy (`no-transform` stops Cloudflare recompressing it) would be what every browser gets. A purge by URL accepts no gzip and would miss the cached copy, so the publish purges by prefix.
- **In Bike Book:** a secret `CATALOG_PUBLISH_TOKEN`: a fine-grained token for this repository only, with "Actions: Read and write" and nothing else.
