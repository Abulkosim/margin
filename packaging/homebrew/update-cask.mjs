#!/usr/bin/env node
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const VERSION = /^\d+\.\d+\.\d+$/;

export function dmgSha256(sums, version) {
  const name = `Margin_${version}_universal.dmg`;
  for (const line of sums.split(/\r?\n/)) {
    const m = line.trim().match(/^([0-9a-f]{64})\s+\*?(.+)$/i);
    if (m && m[2] === name) return m[1].toLowerCase();
  }
  throw new Error(`${name} not found in SHA256SUMS.txt`);
}

export function updateCask(source, version, sha256) {
  if (!VERSION.test(version)) throw new Error(`Invalid version: ${version}`);
  if (!/^[0-9a-f]{64}$/.test(sha256)) throw new Error(`Invalid sha256: ${sha256}`);
  const versionLine = /^(\s*version\s+)"[^"]*"$/m;
  const shaLine = /^(\s*sha256\s+)"[^"]*"$/m;
  if (!versionLine.test(source) || !shaLine.test(source)) {
    throw new Error("Cask has no version or sha256 line");
  }
  return source.replace(versionLine, `$1"${version}"`).replace(shaLine, `$1"${sha256}"`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [caskPath, rawVersion, sumsPath] = process.argv.slice(2);
  if (!caskPath || !rawVersion || !sumsPath) {
    console.error("Usage: update-cask.mjs <cask.rb> <version|vX.Y.Z> <SHA256SUMS.txt>");
    process.exit(2);
  }
  const version = rawVersion.replace(/^v/, "");
  const sha = dmgSha256(readFileSync(sumsPath, "utf8"), version);
  writeFileSync(caskPath, updateCask(readFileSync(caskPath, "utf8"), version, sha));
  console.log(`margin ${version} ${sha}`);
}
