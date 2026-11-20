import assert from "node:assert/strict";
import { test } from "node:test";
import { api } from "../src/worker.js";

const row = (id, model, first_year, final_year, extra = {}) => ({
  id,
  manufacturer: id.split("/")[1],
  manufacturer_name: { riese_and_muller: "Riese & Müller", surly: "Surly Bikes", trek: "Trek" }[id.split("/")[1]],
  model,
  first_year,
  final_year,
  markets: ["US"],
  variants: [],
  ...extra,
});

const MODELS = [
  row("m/riese_and_muller/2024/load4_75", "Load4 75", 2024, null),
  row("m/surly/2020/karate_monkey", "Karate Monkey", 2020, 2022),
  row("m/trek/2021/fuel_ex_8", "Fuel EX 8", 2021, 2021, {
    variants: [{ id: "m/trek/2021/fuel_ex_8_gb", markets: ["GB"] }],
  }),
  row("m/trek/2022/fuel_ex_8_gen_5", "Fuel EX 8 Gen 5", 2022, null),
  row("m/trek/2022/fuel_ex_9_8", "Fuel EX 9.8", 2022, null),
];

const serving = (files) => {
  const fetched = [];
  const fetchCatalog = async (path) => {
    fetched.push(path);
    return path in files
      ? new Response(JSON.stringify(files[path]))
      : new Response("", { status: 404 });
  };
  return { fetched, fetchCatalog };
};

const catalog = (models = MODELS, index = "index.a.json") => ({
  "manifest.4.json": { index, published_at: "2026-11-19T00:00:00Z" },
  [index]: { models },
});

const get = async (worker, search, path = "/v1/models") => {
  const response = await worker.fetch(new Request(`https://catalog.bikeindex.org${path}?${search}`));
  return { status: response.status, body: await response.json() };
};

const ids = ({ body }) => body.models.map(({ id }) => id);

test("a manufacturer alone lists its models, named by their Bike Index slug", async () => {
  const worker = api(serving(catalog()).fetchCatalog);
  const response = await get(worker, "manufacturer=Surly%20%20Bicycles%20");
  assert.deepEqual(response.body.query.manufacturer, { name: "Surly Bicycles", slug: "surly-bicycles" });
  assert.deepEqual(ids(response), ["m/surly/2020/karate_monkey"]);
  assert.deepEqual(response.body.models[0].manufacturer, { name: "Surly Bikes", slug: "surly-bikes" });
});

test("a manufacturer is found by its slug, its legacy slug or bikebook's", async () => {
  const worker = api(serving(catalog()).fetchCatalog);
  const found = async (manufacturer) =>
    ids(await get(worker, `manufacturer=${encodeURIComponent(manufacturer)}`)).length;
  const counts = await Promise.all(
    ["riese-amp-muller", "Riese and Muller", "riese_and_muller", "surly-bikes", "Trek Bicycle Corporation"].map(found),
  );
  assert.deepEqual(counts, [1, 1, 1, 1, 0]);
});

test("a model and year match across manufacturers, exact names first", async () => {
  const worker = api(serving(catalog()).fetchCatalog);
  const response = await get(worker, "model=Fuel+EX+8+(2022)&year=2022");
  assert.deepEqual(response.body.query.model, { name: "Fuel EX 8 (2022)", slug: "fuel_ex_8" });
  assert.deepEqual(ids(response), ["m/trek/2022/fuel_ex_8_gen_5"]);
  assert.equal(response.body.models[0].exact, false);
  const both = await get(worker, "model=fuel%20ex%208");
  assert.deepEqual(ids(both), ["m/trek/2021/fuel_ex_8", "m/trek/2022/fuel_ex_8_gen_5"]);
  assert.deepEqual(
    both.body.models.map(({ exact }) => exact),
    [true, false],
  );
});

test("a year matches from a model's first year through its final year, or on while it's current", async () => {
  const worker = api(serving(catalog()).fetchCatalog);
  const years = async (year) => ids(await get(worker, `manufacturer=surly&year=${year}`)).length;
  assert.deepEqual(await Promise.all([2019, 2020, 2022, 2023].map(years)), [0, 1, 1, 0]);
  assert.deepEqual(ids(await get(worker, "manufacturer=trek&model=fuel_ex&year=2030")), [
    "m/trek/2022/fuel_ex_8_gen_5",
    "m/trek/2022/fuel_ex_9_8",
  ]);
});

test("a market picks the variant sold there, and the model itself where none is", async () => {
  const worker = api(serving(catalog()).fetchCatalog);
  const [gb] = (await get(worker, "manufacturer=trek&model=Fuel%20EX%208&year=2021&market=gb")).body.models;
  assert.equal(gb.id, "m/trek/2021/fuel_ex_8_gb");
  assert.equal(gb.model.slug, "fuel_ex_8_gb");
  assert.deepEqual(gb.markets, ["GB"]);
  const [us] = (await get(worker, "manufacturer=trek&model=Fuel%20EX%208&year=2021&market=US")).body.models;
  assert.equal(us.id, "m/trek/2021/fuel_ex_8");
});

test("a query it can't answer is a 400 saying why", async () => {
  const worker = api(serving(catalog()).fetchCatalog);
  const errors = await Promise.all(
    ["year=2022", "make=Trek", "model=Fuel&year=22", "manufacturer=()"].map((search) => get(worker, search)),
  );
  assert.deepEqual(
    errors.map(({ status }) => status),
    [400, 400, 400, 400],
  );
  assert.match(errors[1].body.error, /make/);
  assert.equal((await get(worker, "model=Fuel", "/models")).status, 404);
});

test("a publish naming a new index is read, and the same index is read once", async () => {
  const files = { ...catalog(MODELS.slice(0, 1), "index.b.json"), ...catalog() };
  const { fetched, fetchCatalog } = serving(files);
  const worker = api(fetchCatalog);
  await get(worker, "manufacturer=trek");
  await get(worker, "manufacturer=trek");
  assert.deepEqual(fetched, ["manifest.4.json", "index.a.json", "manifest.4.json"]);
  files["manifest.4.json"] = { index: "index.b.json" };
  assert.deepEqual(ids(await get(worker, "manufacturer=trek")), []);
});

test("a catalog that fails to load is a 502, and is fetched again on the next request", async () => {
  const files = catalog();
  const { fetchCatalog } = serving(files);
  const worker = api(fetchCatalog);
  const index = files["index.a.json"];
  delete files["index.a.json"];
  assert.equal((await get(worker, "manufacturer=trek")).status, 502);
  files["index.a.json"] = index;
  assert.equal(ids(await get(worker, "manufacturer=trek")).length, 3);
});
