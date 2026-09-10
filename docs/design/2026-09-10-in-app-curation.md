# In-app AI curation

**Goal:** press *populate world* in the studio and get a world whose room descriptions, shop
names and wares have been reworked by a language model - generated, curated and watched from
one screen, pausable across days, never trusted blindly.

**Status:** approved in conversation 2026-09-10. Builds on `evennia_roundtrip/curate.py`
(the resumable, law-gated curator already used from the command line).

---

## What happens on *populate world*

1. Generation runs exactly as today, stages on the dial.
2. If **AI curation** is switched on, the same run carries straight on into curation. The
   server spawns `python -m evennia_roundtrip.curate --run runs/<id>` the moment the
   generator exits cleanly.
3. The dial shows a second phase: rooms curated of the total, kept, rejected by the laws,
   rate and time remaining. **Pause**, **Resume** and **Stop** sit under it.
4. When curation finishes, the studio adopts the curated world, so *save world* and the
   room cards carry the new text.

## What the model reworks

| thing | rule |
|---|---|
| room descriptions | every room, areas **and roads** |
| room names | interiors only - a street's name is the street grid (law G1) |
| wares *(stage 4)* | names and one-line descriptions, still recognisably from their place |
| image briefs | one per room, 1536x640, the locked painterly style - for the ComfyUI pass |

Nothing is overwritten: the model's text goes in `key_ai` / `desc_ai` beside the generator's.
Everything is gated by the same prose laws the templates pass, plus the door-word rule; a
failure keeps the template text and is counted.

## The AI settings panel

- Backend: **GX10 cluster** (vLLM, OpenAI protocol), **local model** (LM Studio), or an
  **API key**. One client covers all three: they speak the same protocol.
- **Test connection**, so a dead backend is found before a run rather than four hours in.
- **Curate after generating** on/off - plain fast generation stays one click.
- Rooms in flight at once (default 6, the cluster's `max-num-seqs`).

**The key never reaches the browser after it is saved.** Settings live in
`viewer/curator.local.json` (git-ignored); the page is shown the key masked, and the key is
handed to the curator through its environment, never its command line, where any process
listing would show it. The browser never talks to the model at all: the server does, so the
page's `connect-src 'self'` guarantee is untouched.

## Durability

The job is a server-side process and every room is journalled as it lands (the curator's
existing `done.jsonl`). Closing the browser does not stop it. A laptop restart does - and
the studio then finds the unfinished journal and offers **resume curation**, which continues
from the last room written.

The curator writes `curate/status.json` as it goes; the server only reads it, so the
server holds no state it could lose.

## Built in stages, each verified in the studio

1. **Settings and connection test** - panel, server endpoints, key handling.
2. **Chained curation with progress** - roads added to the curator, `status.json`, the
   server job, the dial's second phase, adoption of the curated world.
3. **Pause / resume / stop**, and *resume curation* after a restart.
4. **Wares** - curated names and descriptions, gated for place and material logic.
5. **Template / AI toggle** on the room card.

## Not in scope

Running the studio itself on the GX10; generating images (a later pass, from the briefs);
exporting to Evennia from the app (the exporter already exists on the command line).
