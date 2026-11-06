// Builds public/cities.json (city suggestions in the profile form) from
// all-the-cities, which packages GeoNames data (CC BY 4.0, www.geonames.org).
// Run after changing MIN_POPULATION: node scripts/cities.mjs
import { createRequire } from "node:module";
import { writeFileSync } from "node:fs";

const MIN_POPULATION = 50_000;
const require = createRequire(import.meta.url);
const cities = require("all-the-cities");
const rows = cities
  .filter((c) => c.population >= MIN_POPULATION)
  .sort((a, b) => b.population - a.population)
  .map((c) => [c.name, c.country]);
const out = {
  source: "GeoNames (www.geonames.org), CC BY 4.0, via all-the-cities",
  min_population: MIN_POPULATION,
  cities: rows,
};
writeFileSync(new URL("../public/cities.json", import.meta.url), JSON.stringify(out));
console.log(`${rows.length} cities`);
