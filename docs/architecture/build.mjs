#!/usr/bin/env node
// Builds docs/architecture/index.html from template.html by inlining the ANGARS
// diagram PNGs as base64 data URIs — so the doc ships as one self-contained file.
//
//   node docs/architecture/build.mjs
//
// Each %%IMG:<name>%% token in template.html is replaced with a data: URI for
// examples/angars/diagrams/<name>.png. Missing images become an inline notice
// (the page still renders) and are reported on stderr.

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..", "..");
const diagrams = resolve(repoRoot, "examples", "angars", "diagrams");

const templatePath = resolve(here, "template.html");
const outPath = resolve(here, "index.html");

let html = readFileSync(templatePath, "utf8");

const TOKEN = /%%IMG:([a-z0-9_-]+)%%/gi;
let missing = 0;
let inlined = 0;

html = html.replace(TOKEN, (_match, name) => {
  const png = resolve(diagrams, `${name}.png`);
  if (!existsSync(png)) {
    missing++;
    console.error(`  ! missing diagram: ${name}.png`);
    // 1x1 transparent gif keeps <img> valid if a render is absent
    return "data:image/gif;base64,R0lGODlhAQABAAAAACH5BAEKAAEALAAAAAABAAEAAAICTAEAOw==";
  }
  const b64 = readFileSync(png).toString("base64");
  inlined++;
  return `data:image/png;base64,${b64}`;
});

writeFileSync(outPath, html);

const kb = (Buffer.byteLength(html, "utf8") / 1024).toFixed(0);
console.log(`built index.html — ${inlined} image(s) inlined${missing ? `, ${missing} missing` : ""}, ${kb} KB`);
if (missing) process.exitCode = 1;
