#!/usr/bin/env node
import fs from "node:fs";

const lockPath = "package-lock.json";
const lock = JSON.parse(fs.readFileSync(lockPath, "utf8"));
const failures = [];

if (lock.lockfileVersion !== 3) {
  failures.push(`expected lockfileVersion 3, found ${lock.lockfileVersion ?? "missing"}`);
}

for (const [path, entry] of Object.entries(lock.packages || {})) {
  if (path === "") continue;
  if (!path.startsWith("node_modules/")) continue;

  const name = path.slice("node_modules/".length);
  const version = entry.version;
  const tarballName = `${name.includes("/") ? name.split("/").at(-1) : name}-${version}.tgz`;
  const encodedName = name.replace("/", "%2f");
  const expectedResolved = `https://registry.npmjs.org/${encodedName}/-/${tarballName}`;

  if (!version) {
    failures.push(`${path}: missing version`);
  }
  if (!entry.integrity) {
    failures.push(`${path}: missing integrity hash`);
  }
  if (!entry.resolved) {
    failures.push(`${path}: missing resolved tarball URL`);
  } else if (entry.resolved !== expectedResolved) {
    failures.push(`${path}: expected resolved ${expectedResolved}, found ${entry.resolved}`);
  }
}

if (failures.length > 0) {
  console.error(`${lockPath} is not fully pinned:`);
  for (const failure of failures) console.error(`- ${failure}`);
  process.exit(1);
}
