// checks a built dataset against its own manifest. a deployment carries the whole dataset and
// serves nothing else, so shipping one that is missing files takes them off the live site

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

// emit records a whole directory as one entry, holding its file count and total size
const DIRECTORY = /^(.+)\/ \((\d+) files\)$/;

const dir = process.argv[2] ?? "build/data";
const manifest = JSON.parse(readFileSync(join(dir, "manifest.json"), "utf8"));
const problems = [];

for (const [entry, size] of Object.entries(manifest.files)) {
  const shards = DIRECTORY.exec(entry);
  try {
    if (shards) {
      const [, name, count] = shards;
      const files = readdirSync(join(dir, name));
      const bytes = files.reduce((sum, file) => sum + statSync(join(dir, name, file)).size, 0);
      if (files.length !== Number(count)) {
        problems.push(`${name}/: ${files.length} files, manifest says ${count}`);
      }
      if (bytes !== size) problems.push(`${name}/: ${bytes} bytes, manifest says ${size}`);
    } else {
      const bytes = statSync(join(dir, entry)).size;
      if (bytes !== size) problems.push(`${entry}: ${bytes} bytes, manifest says ${size}`);
    }
  } catch (e) {
    problems.push(`${entry}: ${e.message}`);
  }
}

if (problems.length > 0) {
  for (const problem of problems) console.error(problem);
  console.error(`${dir} does not match its manifest, refusing to call it a dataset`);
  process.exit(1);
}

const total = Object.values(manifest.files).reduce((sum, size) => sum + size, 0);
const mb = (total / 1e6).toFixed(1);
console.log(`${dir}: ${manifest.nodes} tags, ${mb} MB, generated ${manifest.generated_at}`);
