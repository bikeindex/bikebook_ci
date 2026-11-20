import { slugify } from "./slugify.js";
import { legacyManufacturer, normalize, slugify as manufacturerSlug } from "./slugifyer.js";

const CATALOG = "https://bikebook-catalog.bikeindex.org/catalog/";
const FORMAT = 4;
const PARAMETERS = ["manufacturer", "model", "year", "market"];
const NOT_FOUND =
  "Not found: the API is /v1/models, and /v1/<id> for a vehicle model (m/…), a component (c/…) or an e-vehicle classification (evc/…)";
/** Where each kind of id is published */
const RECORDS = {
  m: {
    kind: "vehicle_model",
    file: (manifest, id) => manifest.blocks[id.slice("m/".length, id.lastIndexOf("/"))],
    record: ({ models }, id) => models[id],
  },
  c: { kind: "component", file: (manifest) => manifest.components, record: ({ components }, id) => components[id] },
  evc: {
    kind: "e_vehicle_classification",
    file: (manifest) => manifest.vocabulary,
    record: ({ e_vehicle_classifications: records }, id) => records[id] && { id, ...records[id] },
  },
};

export function api(fetchCatalog) {
  const json = async (path) => {
    const response = await fetchCatalog(path);
    if (!response.ok) throw new Error(`${path}: ${response.status}`);
    return response.json();
  };
  let loading;
  // Parsed once per isolate, until a publish names a new index
  const catalog = async () => {
    const manifest = await json(`manifest.${FORMAT}.json`);
    if (loading?.index !== manifest.index) {
      const models = json(manifest.index).then(({ models }) => listings(models));
      loading = { index: manifest.index, models };
      models.catch(() => {
        if (loading?.models === models) loading = undefined;
      });
    }
    return { published_at: manifest.published_at, models: await loading.models };
  };

  const search = async (parameters) => {
    const query = parse(parameters);
    if (query.error) return respond(400, query);
    const { published_at, models } = await catalog();
    return respond(200, { format: FORMAT, published_at, query, models: matches(models, query) });
  };

  const lookup = async (id, { kind, file, record }) => {
    const manifest = await json(`manifest.${FORMAT}.json`);
    const path = file(manifest, id);
    const found = path && record(await json(path), id);
    if (!found) return respond(404, { error: `There's no ${kind} ${id}` });
    return respond(200, { format: FORMAT, published_at: manifest.published_at, kind, ...slugs(id, found), record: found });
  };

  return {
    async fetch(request) {
      if (!["GET", "HEAD"].includes(request.method)) return respond(405, { error: "Only GET" });
      const { pathname, searchParams } = new URL(request.url);
      const id = pathname.slice("/v1/".length);
      const records = pathname.startsWith("/v1/") && RECORDS[id.split("/")[0]];
      const route = pathname === "/v1/models" ? search(searchParams) : records && lookup(id, records);
      if (!route) return respond(404, { error: NOT_FOUND });
      return route.catch((error) => respond(502, { error: `The catalog didn't load: ${error.message}` }));
    },
  };
}

export default api((path) => fetch(CATALOG + path));

const lastSegment = (id) => id.slice(id.lastIndexOf("/") + 1);

const slugs = (id, record) =>
  record.manufacturer
    ? {
        manufacturer: { name: record.manufacturer, slug: manufacturerSlug(record.manufacturer) },
        model: { name: record.model, slug: lastSegment(id) },
      }
    : {};

/** A manufacturer's Bike Index slugs, which `Manufacturer.friendly_find` looks up a name by */
const manufacturerSlugs = (name) => [manufacturerSlug(name), legacyManufacturer(name)];

function listings(rows) {
  const makers = new Map(
    [...new Set(rows.map((row) => row.manufacturer_name))].map((name) => [name, manufacturerSlugs(name)]),
  );
  return rows.map((row) => listing(row, makers.get(row.manufacturer_name)));
}

function listing(row, [manufacturer, legacy]) {
  return {
    slug: slugify(row.model),
    manufacturerSlugs: [manufacturer, legacy, row.manufacturer],
    model: {
      id: row.id,
      manufacturer: { name: row.manufacturer_name, slug: manufacturer },
      model: { name: row.model, slug: lastSegment(row.id) },
      first_year: row.first_year,
      final_year: row.final_year,
      markets: row.markets,
      variants: row.variants,
    },
  };
}

function parse(parameters) {
  const unknown = [...parameters.keys()].filter((key) => !PARAMETERS.includes(key));
  if (unknown.length) return { error: `Unknown parameters ${unknown.join(", ")}: it takes ${PARAMETERS.join(", ")}` };
  const [manufacturer, model, year, market] = PARAMETERS.map((key) => normalize(parameters.get(key) ?? "") || null);
  if (!manufacturer && !model) return { error: "Pass a manufacturer, a model or both" };
  const named = {
    manufacturer: manufacturer && { name: manufacturer, slug: manufacturerSlug(manufacturer) },
    model: model && { name: model, slug: slugify(model) },
  };
  const unnamed = Object.keys(named).filter((key) => named[key]?.slug === "");
  if (unnamed.length) return { error: `${unnamed.join(" and ")} has no letters or digits` };
  if (year && !/^\d{4}$/.test(year)) return { error: `year is a four digit year, not ${year}` };
  return { ...named, year: year && Number(year), market: market?.toUpperCase() ?? null };
}

/** Exact model names first, then those it's the first words of */
function matches(models, { manufacturer, model, year, market }) {
  const wanted = manufacturer && manufacturerSlugs(manufacturer.name);
  const found = models
    .filter(
      ({ slug, manufacturerSlugs, model: found }) =>
        (!wanted || wanted.some((each) => manufacturerSlugs.includes(each))) &&
        (!model || slug === model.slug || slug.startsWith(`${model.slug}_`)) &&
        (!year || (found.first_year <= year && year <= (found.final_year ?? Infinity))),
    )
    .map(({ slug, model: found }) => ({ ...inMarket(found, market), exact: !model || slug === model.slug }));
  return [...found.filter(({ exact }) => exact), ...found.filter(({ exact }) => !exact)];
}

function inMarket(model, market) {
  const variant = market && model.variants.find(({ markets }) => markets.includes(market));
  return variant
    ? { ...model, id: variant.id, model: { ...model.model, slug: lastSegment(variant.id) }, markets: variant.markets }
    : model;
}

const respond = (status, body) =>
  new Response(JSON.stringify(body), {
    status,
    headers: {
      "content-type": "application/json",
      "access-control-allow-origin": "*",
      "cache-control": "public, max-age=60",
    },
  });
