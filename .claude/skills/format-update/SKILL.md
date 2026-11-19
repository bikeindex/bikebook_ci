---
name: format-update
description: Change the layout of the published catalog (a field removed, renamed or reshaped in the index, a model block, the vocabulary, the kit or the comparison, rows dropped or merged, or a file split or added) by publishing a new format beside the old one, then dropping the old one once its readers have moved. Use whenever a change to generate_catalog would change what a reader of the published files sees, or when asked to bump, add or drop a catalog format.
---

# Updating the catalog's format

Readers check the `format` of what they fetch. Changing a format in place breaks every reader still expecting it, from the moment the publish lands until each reader ships. A new field that readers can ignore isn't a layout change, and is published in the current format. Any other layout change takes two PRs:

1. **Publish both.** The new format is published beside the old one, each under its own manifest.
2. **Drop the old one.** This happens only once every reader has shipped on the new format and works there. Open this PR only after the user says so, not when the first PR merges.

## Where formats live

- `FORMATS` in `src/bin/generate_catalog/main.rs` lists the published formats, oldest first. Every file is written once per format and carries its own `format`.
- `manifest_paths` names each format's manifest: `manifest.<format>.json`. Format 3 is also at `manifest.json`, where readers from before per-format manifests fetch it.
- A file that no format's manifest names, a dropped format's manifests included, is deleted `RETENTION` after it stops being named. `publish.yml` uploads every `manifest*.json` and deletes what the generator retired.
- Readers include the viewer (in bike_index: `bikebook/catalog_worker.js`, `similarity.js`) and bike_index's `Integrations::Bikebook::Catalog`, whose `FORMAT` and manifest path move with it. Before the second PR, ask the user whether there are others.

## PR 1: publish both formats

1. Append the new number to `FORMATS`.
2. Put the change behind the format in the generator. The old format's files must stay byte for byte as they were, so their hashed names don't change. Branch on the format in one place (as `listed` does), not at every use, so the second PR deletes one branch.
3. Test the new shape, and that the old format still gets the old one: one test that calls the function with both formats.
4. Describe the new layout in the README, under "Publishing the catalog", for whoever updates the readers.
5. Run `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and the tests against a bikebook checkout.
6. In the PR body, say that this is the first of two PRs, state the new format's layout and manifest path for the readers, and say that the old format is unchanged.

After it merges, the next publish should upload no new file for the old format: its names are unchanged. The readers then move to `manifest.<new>.json` and the new `FORMAT`.

## PR 2: drop the old format

Open this only once the user confirms that every reader has shipped on the new format and works there.

1. Remove the old number from `FORMATS`.
2. Delete its branches in the generator: the `format < N` early return and anything only it used. If it was format 3, also delete `manifest_paths`' `manifest.json` case. After that a format has only its own manifest.
3. Delete the old format's assertions from the tests, and parameters that no longer vary.
4. Remove the old format's description from the README.
5. In the PR body, say that the old format's manifests and files are deleted a day after this publishes, and that any reader still on them breaks then.
