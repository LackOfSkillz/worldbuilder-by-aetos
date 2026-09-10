// The line under the dial says what the curator is working through, and how it reaches it.

import { test } from "node:test";
import assert from "node:assert/strict";

globalThis.window = globalThis.window || {};
const { curationNote } = await import("../public/app/tally.js");

test("the rate is of rooms, then of shelves once the rooms are done", () => {
  assert.match(curationNote({ state: "running", rate_per_min: 12, doing: "rooms" }), /12 rooms\/min/);
  assert.match(curationNote({ state: "running", rate_per_min: 9, doing: "shelves" }),
               /9 shelves\/min/);
  assert.match(curationNote({ state: "running", rate_per_min: 9 }), /9 rooms\/min/,
               "an older curator's status has no `doing` and meant rooms");
});

test("the road to the model is named", () => {
  assert.match(curationNote({ url: "http://100.92.130.112:8888/v1" }), /via Tailscale/);
  assert.match(curationNote({ url: "http://192.168.1.200:8888/v1" }), /via the house LAN/);
});
