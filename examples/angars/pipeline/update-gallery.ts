/** Copies the current demo renders into docs/gallery/ (the committed copies). */
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
const SRC = path.join(ROOT, "examples/angars/out/renders");
const DST = path.join(ROOT, "docs/gallery");
fs.mkdirSync(DST, { recursive: true });
const pngs = fs.readdirSync(SRC).filter((f) => f.endsWith(".png"));
if (pngs.length === 0) {
  console.error("no PNGs — run pnpm demo first");
  process.exit(1);
}
for (const f of pngs) fs.copyFileSync(path.join(SRC, f), path.join(DST, f));
console.log(`gallery updated: ${pngs.length} images`);
