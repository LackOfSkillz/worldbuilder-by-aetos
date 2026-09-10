//! The AI curation settings: which model reworks a world's prose, reached how.
//
// **The page never talks to the model.** It shows and edits settings the server keeps; the
// server tests the connection and runs the curator. So the key typed here goes one way - to
// the server, once - and comes back only as "a key is set", and the page's
// `connect-src 'self'` guarantee is untouched.
//
// **A backend is a list of addresses, tried in order.** The GX10 cluster answers on the
// house LAN at home and on its Tailscale address away from it, so it is saved as both and
// whichever answers is used. The test says which one did, and what the other did instead.

function el(tag, cls, text) {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
}

/// What each backend choice fills in. Somebody switching to "local model" should not have to
/// know LM Studio's port; somebody who has already typed their own addresses keeps them.
export const PRESETS = {
  cluster: { label: "GX10 cluster",
             base_urls: ["http://192.168.1.200:8888/v1", "http://100.92.130.112:8888/v1"],
             model: "deepseek-v4-flash-0731" },
  local: { label: "local model (LM Studio)", base_urls: ["http://localhost:1234/v1"],
           model: "" },
  api: { label: "API key", base_urls: ["https://api.openai.com/v1"], model: "gpt-4o-mini" },
};

async function call(path, body) {
  const response = await fetch(path, body === undefined ? { cache: "no-store" } : {
    method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const value = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(value.error || `server said ${response.status}`);
  return value;
}

/// Say what a connection test found, in words a person can act on.
export function describeTest(result) {
  const where = (url) => {
    try {
      const host = new URL(url).hostname;
      if (host.startsWith("192.168.") || host.startsWith("10.") || host === "localhost") {
        return `LAN (${host})`;
      }
      if (host.startsWith("100.")) return `Tailscale (${host})`;
      return host;
    } catch {
      return url;
    }
  };
  if (result.ok) {
    const others = (result.tried || []).filter((t) => !t.ok)
      .map((t) => `${where(t.url)} ${t.error}`);
    return `✓ ${where(result.url)} answered in ${result.ms} ms`
      + (result.models && result.models.length ? ` · ${result.models.slice(0, 3).join(", ")}` : "")
      + (others.length ? ` (${others.join("; ")})` : "")
      + (result.warning ? ` · ⚠ ${result.warning}` : "");
  }
  return "✗ nothing answered: "
    + (result.tried || []).map((t) => `${where(t.url)} ${t.error}`).join("; ");
}

/// Build the AI curation section into a parent element.
export function buildAiPanel(parent) {
  const wrap = el("div", "wb-section wb-ai");
  wrap.append(el("div", "wb-section-title", "ai curation"));

  const onRow = el("label", "wb-row wb-brush-field");
  const enabled = document.createElement("input");
  enabled.type = "checkbox";
  onRow.append(enabled, document.createTextNode(" curate after generating"));

  const backendRow = el("div", "wb-row");
  const backend = document.createElement("select");
  backend.className = "wb-text";
  for (const [value, preset] of Object.entries(PRESETS)) {
    const option = el("option", null, preset.label);
    option.value = value;
    backend.append(option);
  }
  backendRow.append(backend);

  const field = (label, input) => {
    const row = el("div", "wb-row");
    const caption = el("label", "wb-brush-field", label);
    input.className = "wb-text";
    row.append(caption, input);
    return row;
  };
  const addresses = document.createElement("input");
  addresses.type = "text";
  addresses.placeholder = "http://host:port/v1, another";
  addresses.title = "Tried in order; the first that answers is used.";
  const model = document.createElement("input");
  model.type = "text";
  const models = document.createElement("datalist");
  models.id = "wb-ai-models";
  model.setAttribute("list", models.id);
  const key = document.createElement("input");
  key.type = "password";
  key.autocomplete = "off";
  const atOnce = document.createElement("input");
  atOnce.type = "number";
  atOnce.min = "1";
  atOnce.max = "32";

  const buttons = el("div", "wb-row");
  const test = el("button", "wb-mini", "test connection");
  test.type = "button";
  const save = el("button", "wb-mini wb-mini-go", "save");
  save.type = "button";
  const clearKey = el("button", "wb-mini", "clear key");
  clearKey.type = "button";
  buttons.append(test, save, clearKey);

  const note = el("div", "wb-note wb-ai-note", "loading settings…");

  wrap.append(onRow, backendRow, field("addresses", addresses), field("model", model),
              models, field("key", key), field("at once", atOnce), buttons, note);
  parent.append(wrap);

  let keySet = false;

  const show = (settings) => {
    enabled.checked = Boolean(settings.enabled);
    backend.value = settings.backend || "cluster";
    addresses.value = (settings.base_urls || []).join(", ");
    model.value = settings.model || "";
    atOnce.value = String(settings.at_once || 6);
    keySet = Boolean(settings.key_set);
    key.value = "";
    key.placeholder = keySet ? `key set ${settings.key_hint || ""}` : "no key (not needed for the cluster)";
    clearKey.disabled = !keySet;
  };

  const edits = () => {
    const body = { enabled: enabled.checked, backend: backend.value,
                   base_urls: addresses.value, model: model.value.trim(),
                   at_once: Number(atOnce.value) };
    // Write-only: an empty box keeps whatever key is saved.
    if (key.value.trim()) body.key = key.value.trim();
    return body;
  };

  backend.addEventListener("change", () => {
    const preset = PRESETS[backend.value];
    const current = addresses.value.split(/[\s,]+/).filter(Boolean);
    const wasAPreset = Object.values(PRESETS).some((p) =>
      p.base_urls.join(",") === current.join(","));
    // A preset replaces a preset, never somebody's own addresses.
    if (!current.length || wasAPreset) addresses.value = preset.base_urls.join(", ");
    if (!model.value || Object.values(PRESETS).some((p) => p.model === model.value)) {
      model.value = preset.model;
    }
  });

  test.addEventListener("click", async () => {
    test.disabled = true;
    note.textContent = "asking each address in turn…";
    try {
      const result = await call("/curator/test", edits());
      note.textContent = describeTest(result);
      models.replaceChildren(...(result.models || []).map((id) => {
        const option = document.createElement("option");
        option.value = id;
        return option;
      }));
      if (result.ok && !model.value && result.models && result.models.length) {
        model.value = result.models[0];
      }
    } catch (error) {
      note.textContent = `✗ ${error.message}`;
    } finally {
      test.disabled = false;
    }
  });

  save.addEventListener("click", async () => {
    try {
      show(await call("/curator/settings", edits()));
      note.textContent = enabled.checked
        ? "saved · the next populate run will be curated"
        : "saved · curation is off, populate runs as before";
    } catch (error) {
      note.textContent = `✗ not saved: ${error.message}`;
    }
  });

  clearKey.addEventListener("click", async () => {
    try {
      show(await call("/curator/settings", { clear_key: true }));
      note.textContent = "key cleared";
    } catch (error) {
      note.textContent = `✗ ${error.message}`;
    }
  });

  call("/curator/settings").then((settings) => {
    show(settings);
    note.textContent = settings.enabled
      ? "curation is on · test the connection before a long run"
      : "curation is off";
  }).catch((error) => {
    note.textContent = `✗ could not load settings: ${error.message}`;
  });

  return { refresh: () => call("/curator/settings").then(show) };
}
