// The room card shows the curator's text or the generator's, and says which.

import { test } from "node:test";
import assert from "node:assert/strict";

// area-markers.js reads `window` only inside functions, so a bare stub is enough to load it.
globalThis.window = globalThis.window || {};
const { roomText } = await import("../public/app/area-markers.js");

const curated = { key: "the alchemist", key_ai: "Wound-Salve Stall", desc: "A template room.",
                  desc_ai: "The curated room.", shop: "the inn", shop_ai: "the Quiethall Inn" };

test("the AI side shows the curator's name, text and shop", () => {
  const shown = roomText(curated, "ai");
  assert.equal(shown.ai, true);
  assert.equal(shown.key, "Wound-Salve Stall");
  assert.equal(shown.desc, "The curated room.");
  assert.equal(shown.shop, "the Quiethall Inn");
});

test("the template side shows exactly what the generator wrote", () => {
  const shown = roomText(curated, "template");
  assert.equal(shown.ai, false);
  assert.equal(shown.key, "the alchemist");
  assert.equal(shown.desc, "A template room.");
  assert.equal(shown.shop, "the inn");
});

test("a room never curated shows its own text whichever side is chosen", () => {
  const plain = { key: "Mill Lane", desc: "Only a template." };
  assert.equal(roomText(plain, "ai").desc, "Only a template.");
  assert.equal(roomText(plain, "ai").ai, false);
});

test("a street keeps its name on the AI side when the curator left it alone", () => {
  const street = { key: "Mill Lane, East End", key_ai: null, desc: "t", desc_ai: "a" };
  assert.equal(roomText(street, "ai").key, "Mill Lane, East End");
});

test("the AI side lists the curator's goods with what each looks like", () => {
  const shop = { key: "Hill Road", desc: "t", stock: ["a whetstone from Greystair"],
                 stock_ai: [{ name: "a fine-grit whetstone from Greystair",
                              desc: "A grey stone worn hollow in the middle by long use." }] };
  const ai = roomText(shop, "ai");
  assert.equal(ai.ai, true, "a curated shelf alone is enough to offer the AI side");
  assert.deepEqual(ai.wares.map((w) => w.name), ["a fine-grit whetstone from Greystair"]);
  assert.match(ai.wares[0].desc, /worn hollow/);
  assert.equal(ai.desc, "t", "no curated room text falls back to the template's");
  const plain = roomText(shop, "template");
  assert.deepEqual(plain.wares, [{ name: "a whetstone from Greystair", desc: "" }]);
});
