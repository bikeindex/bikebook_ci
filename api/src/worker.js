import { manufacturerSlug, slugify } from "./slugify.js";

const CATALOG = "https://bikebook-catalog.bikeindex.org/catalog/";
const FORMAT = 4;
const PARAMETERS = ["manufacturer", "model", "year", "market"];

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
      const models = json(manifest.index).then(({ models }) => models.map(listing));
      loading = { index: manifest.index, models };
      models.catch(() => {
        if (loading?.models === models) loading = undefined;
      });
    }
    return { published_at: manifest.published_at, models: await loading.models };
  };

  return {
    async fetch(request) {
      const url = new URL(request.url);
      if (url.pathname !== "/v1/models") return respond(404, { error: "Not found: the API is /v1/models" });
      if (!["GET", "HEAD"].includes(request.method)) return respond(405, { error: "Only GET" });
      const query = parse(url.searchParams);
      if (query.error) return respond(400, query);
      const loaded = await catalog().catch((error) => ({ error }));
      if (loaded.error) return respond(502, { error: `The catalog didn't load: ${loaded.error.message}` });
      return respond(200, {
        format: FORMAT,
        published_at: loaded.published_at,
        query,
        models: matches(loaded.models, query),
      });
    },
  };
}

export default api((path) => fetch(CATALOG + path));

const lastSegment = (id) => id.slice(id.lastIndexOf("/") + 1);

function listing(row) {
  return {
    slug: slugify(row.model),
    model: {
      id: row.id,
      manufacturer: { name: row.manufacturer_name, slug: row.manufacturer },
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
  const [manufacturer, model, year, market] = PARAMETERS.map((key) => parameters.get(key)?.trim() || null);
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
  const found = models
    .filter(
      ({ slug, model: found }) =>
        (!manufacturer || found.manufacturer.slug === manufacturer.slug) &&
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
