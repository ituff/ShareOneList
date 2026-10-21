#!/usr/bin/env node
// Bake a tagged (prerelease) version into the build metadata files.
//
// Repo convention keeps the version files at the plain base ("2.3.0") — the
// beta/rc suffix lives only in the git tag. The release CI runs this before
// building a prerelease tag so the binary, the About page, and the updater's
// CURRENT_VERSION all report e.g. "2.3.0-beta.4"; without it the app shows
// the bare base and the beta channel re-offers the already-installed beta.
//
// WiX requires a purely numeric ProductVersion, so the suffix is additionally
// folded into bundle.windows.wix.version ("2.3.0-beta.4" -> "2.3.0.4");
// NSIS only warns and macOS accepts the full string.
//
// Usage: node scripts/bake-release-version.mjs 2.3.0-beta.4   (run in tauri-app/)
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+-[A-Za-z0-9.]+$/.test(version ?? "")) {
  console.error(`usage: node bake-release-version.mjs <x.y.z-prerelease> (got: ${version})`);
  process.exit(1);
}

// Fold "beta.4"-style suffixes into a numeric 4th segment for WiX.
const wixMatch = version.match(/^(\d+)\.(\d+)\.(\d+)-[A-Za-z]+\.(\d+)$/);
const wixVersion = wixMatch && `${wixMatch[1]}.${wixMatch[2]}.${wixMatch[3]}.${wixMatch[4]}`;

for (const file of ["package.json", "src-tauri/tauri.conf.json"]) {
  const json = JSON.parse(readFileSync(file, "utf8"));
  json.version = version;
  if (file.endsWith("tauri.conf.json") && wixVersion) {
    json.bundle.windows.wix = { ...json.bundle.windows.wix, version: wixVersion };
  }
  writeFileSync(file, JSON.stringify(json, null, 2) + "\n");
  console.log(`${file}: version -> ${version}`);
}
if (wixVersion) {
  console.log(`src-tauri/tauri.conf.json: bundle.windows.wix.version -> ${wixVersion}`);
}

// Cargo.toml: patch the [package] version — the first line-start `version =`
// match. Dependency versions live inline (`tauri = { version = "2" }`) and
// never start a line, so the anchor cannot hit them.
const tomlPath = "src-tauri/Cargo.toml";
const toml = readFileSync(tomlPath, "utf8");
const patched = toml.replace(/^version = ".*$/m, `version = "${version}"`);
if (patched === toml) {
  console.error("Cargo.toml: no package version line found");
  process.exit(1);
}
writeFileSync(tomlPath, patched);
console.log(`${tomlPath}: version -> ${version}`);
