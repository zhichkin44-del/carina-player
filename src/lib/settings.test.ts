// Run with `npm test` (Node's built-in test runner, no extra dependencies).
import assert from "node:assert/strict";
import { test } from "node:test";
import { DEFAULT_LANG, parseSettings } from "./settings.ts";

test("a clean install starts in English", () => {
  assert.equal(DEFAULT_LANG, "en");
  assert.equal(parseSettings(null).lang, "en");
});

test("settings saved before the language switch keep the Russian interface", () => {
  assert.equal(parseSettings(JSON.stringify({ opacity: 40, volume: 0.8 })).lang, "ru");
});

test("a saved language is kept", () => {
  assert.equal(parseSettings(JSON.stringify({ opacity: 10, volume: 0.5, lang: "en" })).lang, "en");
  assert.equal(parseSettings(JSON.stringify({ opacity: 10, volume: 0.5, lang: "ru" })).lang, "ru");
});

test("broken settings fall back to the defaults", () => {
  assert.deepEqual(parseSettings("{not json"), { opacity: 40, volume: 0.8, lang: "en" });
});
