import assert from "node:assert/strict";
import { test } from "node:test";
import { legacyManufacturer, slugify } from "../src/slugifyer.js";

// Bike Index's Slugifyer.slugify and .legacy_manufacturer of each, run in Ruby
const RUBY = [
  ["Riese & Müller", "riese-amp-muller", "riese_amp_muller"],
  ["S & M", "s-amp-m", "s_amp_m"],
  ["Surly Bicycles", "surly-bicycles", "surly"],
  ["Trek Bicycle Corporation", "trek-bicycle-corporation", "trek_corporation"],
  ["Brüdeli & Co.", "brudeli-amp-co", "brudeli_amp"],
  ["Ritchey (Ritchey Design)", "ritchey", "ritchey"],
  ["Velo+Bike", "velobike", "veloplusbike"],
  ["Haibike (Accell)", "haibike", "haibike"],
  ["Øyvind  Cycles", "oyvind--cycles", "oyvind"],
  ["Łódź Works", "lodz-works", "lodz"],
  ["E-Bike Elec. Bicycles", "e-bike-elec-bicycles", "e_bike_elec"],
  ["Tern%20Bikes", "tern-bikes", "tern"],
  ["Black Market Frameworks", "black-market-frameworks", "black_market"],
  ["Ŋorth ×Bikes", "ngorth-xbikes", "ngorth_xbikes"],
  ["Café Racer™", "cafe-racer", "cafe_racer"],
  [" Fit\tBike Co. ", "fit-bike-co", "fit"],
  ["İzmir", "izmir", "i_zmir"],
  ["Straße E Bikes", "strasse-e-bikes", "strasse"],
  ["123 Company", "123-company", "123"],
];

test("slugs a manufacturer as Bike Index does", () => {
  assert.deepEqual(
    RUBY.map(([name]) => [name, slugify(name), legacyManufacturer(name)]),
    RUBY,
  );
});
