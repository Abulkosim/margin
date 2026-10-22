import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { dmgSha256, updateCask } from "./update-cask.mjs";

const cask = readFileSync(new URL("./Casks/margin.rb", import.meta.url), "utf8");
const a = "a".repeat(64);
const b = "b".repeat(64);
const sums = [
  `${a}  Margin_0.5.0_x64-setup.exe`,
  `${b}  Margin_0.5.0_universal.dmg`,
  `${a}  Margin_0.5.0_universal.dmg.sig`,
].join("\n");

test("picks the universal DMG checksum", () => {
  assert.equal(dmgSha256(sums, "0.5.0"), b);
});

test("fails when the DMG is missing", () => {
  assert.throws(() => dmgSha256(sums, "0.6.0"), /not found/);
});

test("rewrites only version and sha256", () => {
  const out = updateCask(cask, "0.5.0", b);
  assert.match(out, /^ {2}version "0\.5\.0"$/m);
  assert.match(out, new RegExp(`^ {2}sha256 "${b}"$`, "m"));
  assert.match(out, /#\{version\}_universal\.dmg/);
  assert.equal(out.split("\n").length, cask.split("\n").length);
});

test("rejects bad input", () => {
  assert.throws(() => updateCask(cask, "v0.5.0", b), /Invalid version/);
  assert.throws(() => updateCask(cask, "0.5.0", "xyz"), /Invalid sha256/);
  assert.throws(() => updateCask("cask \"x\" do\nend\n", "0.5.0", b), /no version/);
});
