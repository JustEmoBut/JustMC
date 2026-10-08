import assert from "node:assert/strict";
import { test } from "node:test";
import { count, label, noun } from "./format.ts";

test("count abbreviates thousands and millions", () => {
  assert.equal(count(999), "999");
  assert.equal(count(12_400), "12k");
  assert.equal(count(3_250_000), "3.3M");
});

test("label turns a slug into words", () => {
  assert.equal(label("game-mechanics"), "Game mechanics");
});

test("noun follows the folder and the number", () => {
  assert.equal(noun("mods", 1), "mod");
  assert.equal(noun("resourcepacks", 2), "resource packs");
});
