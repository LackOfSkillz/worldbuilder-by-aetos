# Mark 2 — the World Studio

**Status:** engineering baseline. Supersedes the previous Mark 1 + Mark 2 master spec.

Mark 1's engine work stands as built and is referenced rather than restated — see
`mark-1-scope.md`, `2026-08-31-generator-spec.md` and `anchoring-architecture.md`.

Mark 1 built a planet engine. Mark 2 turns it into a product, and the product is not the
generator.

## 1. What Worldbuilder is

A standalone visual 3D worldbuilding studio for Evennia. A builder opens or generates a
planet, places existing or new areas on it, declares how those areas connect to generated
space and to each other, reviews the result as a file, and reconciles that file into a
game.

The procedural planet is a subsystem behind that. It was the whole of Mark 1 and it is one
of four subsystems in Mark 2.

Two users, and the second sets the constraints:

- **Greenfield** — an empty Evennia. Worldbuilder creates areas.
- **Brownfield** — years of authored rooms, exits and zones. Worldbuilder puts a planet
  around them and changes nothing.

The brownfield case defines the safety model, so it is written first everywhere below.

## 2. The product principle

> Take the Evennia world somebody has already built, and put a planet around it.

Worldbuilder must never require an existing game to rebuild rooms, change exits, replace
its zone system, convert rooms to latitude and longitude, adopt a new coordinate system,
or migrate data into Worldbuilder-owned objects.

Integration is not ownership. Global is not complete.

## 3. Ownership rules

**Brownfield — load-bearing.** Worldbuilder never writes Worldbuilder metadata onto
objects the game already owned. Rooms, Exits, Characters, zone attributes, tags,
descriptions and custom area objects are untouched. All brownfield Worldbuilder state —
anchors, seams, placement, hierarchy — lives in Worldbuilder-owned tables.

One exception exists, and it is host-initiated: a game may opt in to a stable Worldbuilder
identity tag on its own rooms (§9.2). Never the default, never applied unasked.

**Greenfield.** Worldbuilder may create game objects only where a worldfile explicitly
requests an area that does not exist. Every created object is recorded, identified and
distinguishable from host content; creation is idempotent, and reversible within the
deletion-safety rules of §11.

## 4. The shared world engine

The generator core moves to Rust. One canonical implementation, two compiled targets:

    Rust world engine
      |
      +-- native library  -> Python bindings -> Evennia, maritime
      +-- WASM            -> browser studio

The studio and the game evaluate the *same compiled algorithm*, not two implementations
that tests hope agree. That is the reason for the port. Performance is a welcome side
effect, not the motive.

Rust owns generation. Python owns Evennia integration, database state, commands, adapters,
inventory, apply and reconciliation. WASM exposes only what the studio needs to draw and
query a planet — world spec, parameters, positions, samples — and knows nothing of Django,
Rooms or typeclasses.

**End users install a wheel.** No Rust toolchain, no Cargo, no compiler configuration.
Prebuilt native artifacts ship for supported platforms; a platform with no wheel falls back
to a source build, which is a documented footnote rather than a supported path.

**Contributors changing algorithms need Rust. Builders changing parameters do not.** That
line is deliberate, and §5 is what keeps the second category large.

### 4.1 Bit-equality is a precondition, not an assumption

The architecture rests on native and WASM builds returning identical results. That is a
claim to be *measured before the studio is built on it*, across sphere points, noise, plate
placement, continentality, terrain sampling, shelf shaping, detail and explicit features.
If it does not hold, the architecture is wrong, and we need to know in a day rather than a
month.

### 4.2 DETERMINISM-001 — two guards, not a rule in a document

Platform trigonometry differs between native and WASM in the last bits, and coastlines live
exactly where last bits decide. Generator-critical code therefore never calls `f64::sin`
and its relatives from std; it calls a central `detmath` module backed by one shared
pure-Rust implementation (`libm`).

"Don't use std trig" is the kind of rule that holds for a year and then quietly stops. So it
is mechanised, twice:

    static guard       CI rejects banned math APIs in generator-critical crates
    behavioural guard  CI runs a native-vs-WASM equality corpus

Generator code never chooses its own math provider.

## 5. The declared parameter surface

Mark 2 requires one declared, versioned, serialisable generator parameter schema: key,
type, valid range, default, display metadata, generator compatibility. The studio edits
that surface; the generator consumes it; neither invents parameters the other cannot see.

This is not UI convenience. It is what makes a world identifiable as **seed + parameters +
generator version**, which is the triple provenance needs. It is also why a builder who
wants a wetter world does not need a compiler.

Algorithms are code. Parameters are data. Maximise the second without turning the generator
into a bag of public constants.

## 6. VERSION-001 — versions fail closed

Mark 1 worlds were disposable, so generator identity did not matter. Mark 2 worldfiles are
persistent and reviewable, so it does.

The invariant is *not* that every generator version lives forever. It is:

> Worldbuilder never silently evaluates a worldfile with a generator other than the one it
> declares.

    worldfile declares v4, engine supports v4    evaluate
    worldfile declares v4, engine dropped v4     refuse, clearly
    under no circumstances                       silently evaluate as v7

Permanent retention of every experimental generator is not required. Explicit migration
tooling is the realistic path — regenerate, compare anchored placements, report land and
water conflicts, assist repositioning, emit a new worldfile. A version field that refuses is
honest in a way one that guesses is not.

## 7. The studio

A static web application with the WASM engine embedded. It needs no live Evennia to
generate a planet, inspect it, edit parameters, arrange an anchor tree, position areas,
define seams or write a worldfile.

Its visual surface is a 3D globe: rotate, zoom, inspect terrain, distinguish land from
water, select positions, see anchored areas and their hierarchy, see seams, plant new areas.
It is an instrument, not a renderer — no photorealism, no atmospherics, no vegetation. It
draws from the WASM generator, never from a separately baked approximation that could
disagree with the game.

**No live connection.** No websocket, no browser access to the ORM, no credentials to the
game database. The studio and the game exchange files. Authority stays on the Evennia side,
and every change to a world is reviewable, diffable, committable and replayable on staging
before it touches anything.

## 8. Three artifacts

    INVENTORY   game   -> studio    what the game already has
    WORLDFILE   studio -> game      what Worldbuilder should manage
    APPLY       worldfile -> game   reconciliation

Deliberately asymmetric. The inventory is a snapshot; the worldfile is desired state; the
game never exposes its whole database to a browser.

### 8.1 Inventory

`worldbuilder export` writes a schema-versioned JSON snapshot: export metadata, zones with
ids and room counts, zone relationships where discoverable, the exit graph between zones,
boundary rooms as seam candidates, unzoned rooms, and discovery provenance.

Export is **read-only and safe against a live game**. It may not mutate rooms, exits, zone
attributes, tags, descriptions or any host metadata.

Every fact carries its provenance, because the studio must know which facts are guesses:

    zone_source: explicit_adapter
    zone_source: graph_inference

### 8.2 Zone discovery

Evennia has no zone concept, so Worldbuilder must not assume one convention. Discovery goes
through a narrow adapter protocol — list zones, list a zone's rooms, list zone edges, list
boundary rooms, report known relationships — with built-in adapters for common conventions
and a game free to supply its own.

Where no convention exists, exit-graph clustering proposes likely areas. **Inference is
advisory.** It produces inventory candidates for studio review; it never classifies a game's
rooms authoritatively, and it never writes anything.

*Verified against DragonsIre.* Zone identity is a flat `room.db.zone_id` string —
`the_landing`, `barbarian-guild-map`, `drowned-undercroft-map`. No parent-child hierarchy
exists in the live database; parentage lives only in source-tree organisation and naming
convention. `typeclasses/rooms.py` defaults unassigned rooms to `default_region`, which is
exactly the remainder graph inference exists to sort. The adapter for this game is trivial,
and the hierarchy must be declared in the studio, because the database does not contain it.

### 8.3 Worldfile

The unit of reviewed world change. Human-readable, diffable, committable, replayable.

    {
      "schema_version": "...",
      "world":   { "seed": "...", "generator_version": "...", "parameters": {} },
      "anchors": [],
      "seams":   [],
      "area_requests": []
    }

It is **desired state, not a script**: "this is what the Worldbuilder-managed portion of this
game should look like", never "run these commands in this order". Worldfile schema version
and generator version are separate fields solving separate problems, and are never
conflated.

Serialisation is deterministic — stable ordering, stable float formatting, logical
identifiers rather than runtime ids, and no timestamps that dirty an otherwise unchanged
export.

## 9. Identity

The hardest correctness problem in Mark 2, and it has two answers, because the two ownership
regimes differ.

A seam that names its target by dbref is broken by design: a game that rebuilds an area from
a build script issues fresh dbrefs, and every seam into that area dangles silently. Durable
identity is logical; the database id is a cache.

### 9.1 Greenfield — objects Worldbuilder created

Three identities, each answering a different question:

    Evennia prototype tag     which template created this
      category: Evennia's own prototype category
      value:    wb_harbour_room

    worldbuilder.area         which anchored area it belongs to
      value:    gannet_harbour

    worldbuilder.instance     which logical object this is
      value:    gannet_harbour.main_seam

Area identity alone is insufficient and must not be relied on. A stub area happens to hold
one room, but a forty-room generated area gives all forty the same area id, and the seam can
no longer say which one it means.

Instance identity is what the worldfile references:

    { "seam_id": "gannet_harbour_port", "target_instance": "gannet_harbour.main_seam" }

That survives primary-key changes, export and import, reconstruction, reapplication, and
areas with many rooms. The ownership table may cache the ObjectDB id for speed; it is never
the durable identity.

### 9.2 Brownfield — objects the host owns

The no-write rule forbids stamping identity onto host rooms, so identity is reconstructed
rather than assigned. Worldbuilder stores the dbref plus a validation fingerprint — the room
key, the host's own zone identity where the adapter reports one, and the sorted names of the
room's exits — and resolves in order:

    1. try the dbref, and validate it against the fingerprint
    2. if invalid, search by fingerprint
    3. relink only on exactly one strong match
    4. otherwise report a conflict
    5. never guess

A host may **opt in** to a stable Worldbuilder tag on its own rooms for exact identity. That
is a host decision, one reversible write, and never the default.

## 10. Object reconciliation belongs to Evennia

Evennia 5.0.1 already ships a desired-state reconciler for objects. Worldbuilder stands on
it rather than reimplementing it:

    Worldbuilder owns          Evennia prototypes own
      area identity              object template
      instance identity          object diff
      anchor tree                KEEP / UPDATE / REPLACE / REMOVE directives
      seams                      builder-edit preservation
      graph topology             object-level update execution
      existence and deletion     formatted object diff
      ownership

Greenfield creation therefore **spawns through the prototype system**, and object updates run
through `batch_update_objects_with_prototype` with `exact=False` and implicit-keep diff
semantics — so a builder's edits to a generated room are preserved by the host framework's
own rules rather than by a scheme Worldbuilder invented and would have to defend.

An earlier draft proposed attribute fingerprints for this. It is dropped. Two competing
answers to "has the builder intentionally changed this object?" is worse architecture than
one, and the framework's answer is the one the builder already expects.

**The boundary, stated explicitly.** `exact=False` answers *what happens to attributes on an
object that still exists*. It does not answer *whether the object should exist*. That is
Worldbuilder's, and the prototype reconciler must never become the authority on graph
existence.

## 11. Ownership, modification and deletion

Three facts, kept apart:

    ownership       Worldbuilder created this object
    modification    this object has diverged from its prototype
    removability    it is safe to delete this object now

Ownership does not lapse. A builder renaming, redescribing or re-attributing a
Worldbuilder-created room does not make Worldbuilder stop knowing it created that room.
Modification is Evennia's question, answered by prototype diff. Ownership is Worldbuilder's,
answered by its ownership row and instance identity.

Deletion is a third question, and the dangerous one. A generated seam room that has since
acquired builder-dug exits, quest hooks, persistent contents and a spawn point is still
unambiguously Worldbuilder-owned — and deleting it automatically would be vandalism. So
owned objects carry a removability state, and the deletion planner inspects what depends on
them: host-owned exits attached, contents not owned by Worldbuilder, discoverable external
references, builder-created topology, prototype divergence.

Dry run says:

    REMOVE wb:gannet_harbour.main_seam
      BLOCKED: 3 host-owned exits depend on this room

and not "we no longer own this". Brownfield objects are never deleted because an anchor left
a worldfile; removing a brownfield anchor removes Worldbuilder metadata and nothing else.

## 12. Anchors, seams and exposure

Anchors form a **tree**, not a list. Only root anchors carry a planetary position; children
carry a relationship to their parent, so The Landing is one thing to place rather than forty.

    Planet
      +-- The Landing              root, placed on the globe
            +-- contained     -> Temple District, guild halls
            +-- subterranean  -> Undercroft

The tree must be acyclic, each child has exactly one containment parent, and a child
referencing a missing parent fails validation. Worldbuilder's tree is its own: it need not
mirror any hierarchy in the host database, and it leaves `room.db.zone_id` alone.

**Field exposure** is separate from containment, because containment says where a thing is
and exposure says which planetary fields reach it. A surface district gets terrain, weather
and daylight; a building interior gets neither weather nor daylight; a subterranean area
inherits the ground above it and none of its sky. Mark 2 establishes the concept so a later
climate system has somewhere correct to land — an Undercroft reporting light rain is the kind
of wrongness that ends a builder's trust in the tool.

A **seam** is a precise connection between generated and authored space: a world position, a
target instance, a kind, and metadata. A fuzzy footprint and an exact seam coexist by design
— Crossing occupies roughly this area; its North Gate is at this exact point.

Kinds are general from the start: port, road, gate, ferry, trailhead, dungeon entrance.
Maritime's port seams will need approach and berth depth, maximum draft, shelter, approach
heading and tidal restriction, so a game can answer "can this vessel enter?" without
importing Worldbuilder types.

Terrain remains a **total field** underneath all of it. An anchored area governs
representation authority, never whether physical geography exists; `terrain_z_at` answers
beneath an authored harbour, or a ship cannot anchor in its own port.

## 13. Apply

    worldbuilder export
    worldbuilder apply <worldfile>

Two commands. A small trust surface for a game that already exists.

**APPLY-001 — apply runs in the live Evennia process.** Either an in-game administrative
command or a server-side API invoked through the running game, using the same live object
layer as any other Evennia mutation. Objects written from an unrelated process are invisible
to the running server's cached object identity until reload, which would present as: apply
reports success, the game shows nothing, a second apply reads the same stale state and
reports a no-op. That failure looks like a Worldbuilder bug and is not one, so the
architecture refuses to permit it. An offline maintenance command may exist later, but it
must require a stopped server or force reload semantics. Mark 2 avoids the complication
entirely.

**Validation before mutation.** Schema, generator version support, parameter validity,
referenced identities, anchor-tree integrity, missing parents, cycles, invalid seam targets,
identifier collisions, requests to modify host-owned objects, and whether requested creation
is permitted. Failure occurs before partial mutation wherever practicable, and coherent
groups of mutations run inside transactional boundaries. Ownership is recorded atomically
with the objects it describes — never create content and then attempt to remember owning it.

**Dry run is the default.** The plan distinguishes CREATE, UPDATE, REMOVE, UNCHANGED,
CONFLICT and HOST-OWNED–WILL-NOT-MODIFY. Reviewability is mandatory; the wording is not.

**Apply is idempotent.** The same worldfile applied twice performs its changes once and then
does nothing.

## 14. Creating an area

One primitive — **plant anchored area** — with creation parameters. Not a stub path and a
generation path; those diverge, and the divergence is where the bugs live.

**Stub is the default**, and creates one Room and no Exit. An exit needs two ends and the far
end is generated space, which has no Room to point at; the connection *is* the seam record.
So the smallest owned change is genuinely one object, and the builder gets: here is where the
generated world meets yours — now dig, normally, with the tools you already use.

The typeclass is the host's own `settings.BASE_ROOM_TYPECLASS`, overridable per area. Never a
Worldbuilder typeclass: a stub in DragonsIre must be a DragonsIre room, or it misses every
hook that game's rooms rely on. After spawn, an optional
`adapter.on_room_created(room, area_context)` lets the host stamp its own conventions —
DragonsIre's `zone_id`, and whatever else it needs. Worldbuilder does not decide what a host's
room looks like; the host does. The object was created by Worldbuilder, so no brownfield rule
is engaged.

Generated mode is the identical path with N rooms: spawned through the same prototypes,
carrying the same three identities, recorded in the same ownership rows, reconciled by the
same machinery, reversed by the same deletion-safety rules. The first room is the seam room.

**Room count and footprint are different quantities**, and the schema keeps them apart. A
forty-room dungeon may occupy a small planetary footprint; a six-room wilderness crossing may
span many kilometres.

## 15. Acceptance

**Determinism.** For a fixed seed, generator version and parameter set, a fixed coordinate
corpus evaluated through the WASM engine and through the Python-bound native engine returns
bit-identical values for every value in the deterministic contract.

**Brownfield.** Export inventory; load it; place a pre-existing zone on the globe; save;
apply; verify anchor and seam state exists in Worldbuilder tables; verify no host Room or
Exit field changed; apply again and verify a no-op; remove the anchor from desired state;
reconcile; verify host rooms still exist, unchanged.

**Greenfield stub.** Plant an area with `kind: stub`; save; dry-run and review exactly one
creation; apply; verify ownership and all three identities recorded; reapply and verify a
no-op; remove the area from desired state; reconcile; verify only Worldbuilder-owned content
is eligible for removal, and that removal blocks where host content depends on it.

**Generated.** The same primitive with `kind: generated, rooms: N` produces the same anchor
type, seam semantics and ownership semantics through the same pipeline, with no second
creation path, and remains idempotent on reapply.

## 16. Non-goals for Mark 2

Live collaborative editing. Direct browser-to-database connection. Procedural city, quest or
culture generation. Production climate simulation. Realistic rivers. Economies. Flora and
fauna. Discovery economy. Seasonal ice. Automatic conversion of rooms to coordinates.
Authoritative automatic hierarchy inference. Destructive migration of any existing game data.

## 17. Amendments carried into this baseline

    APPLY-001         apply executes in the live Evennia process
    DETERMINISM-001   CI forbids non-approved math in generator-critical Rust, and runs the
                      native-vs-WASM equality corpus
    VERSION-001       unsupported generator versions fail closed; no silent substitution, and
                      no commitment to permanent retention
    IDENTITY-001      two identity strategies: stable instance tags for Worldbuilder-owned
                      objects, dbref-plus-fingerprint for host-owned ones

## 18. Still open

**Build order — Open Item B.** The four subsystems (shared Rust engine, studio, inventory and
discovery, desired-state apply) have no settled implementation order. It should be decided as
vertical slices against dependency and risk, not as four isolated projects, and not by
whichever is most convenient to start. The bit-equality spike of §4.1 precedes anything that
assumes it.

## Appendix — verified facts and their sources

Distinguishing what was checked from what was reasoned.

**Evennia 5.0.1**, `evennia/prototypes/spawner.py`, read from source because the published
documentation page understates the system and is wrong on two points: it claims spawned
objects cannot be found from their prototype, and that no update mechanism exists.

- `spawn()` stamps `(prototype_key, PROTOTYPE_TAG_CATEGORY)` on every spawned object —
  `:1008-1011`, applied at `:800-801`.
- Lookup by that tag: `ObjectDB.objects.get_by_tag(...)` — `:675`.
- Diff engine: `prototype_diff` `:368`, `prototype_diff_from_object` `:525`, emitting
  `KEEP / ADD / UPDATE / REMOVE`, flattened by explicit precedence at `:476-518`.
- Builder-edit preservation: `implicit_keep=True` and `exact=False` defaults, documented
  in-source as retaining unspecified properties rather than removing them.
- Plan rendering: `format_diff(diff, minimal=True)` — `:562`.
- Reading a prototype back off an object: `prototype_from_object` — `:299`, via the same tag
  at `:312`.
- Limit: `prototype_key` identifies a *class* of spawned objects, not an instance. Instance
  identity, topology and deletion remain Worldbuilder's.

**Evennia batch processors**, from the official documentation — evaluated as a possible
transport, and rejected.

- `.ev` batch-command files separate commands with `#` lines, support `#INSERT`, reference
  objects only by the caller's current position, provide no way to capture or assign a stable
  identifier to a created object, and do not stop when a command fails.
- `.py` batch-code files are superuser-only, run each `#CODE` block in isolation, and are
  documented as not idempotent — re-running risks duplicating objects.
- Both are replay scripts, which §8.3 requires a worldfile not to be. Retained instead as a
  possible *export* format for builders who want generated areas as source in their own
  repository.
- The documentation's own suggested workaround for the identity problem — unique tags or
  aliases — is the mechanism §9.1 adopts.

**DragonsIre**, read from the game source: `room.db.zone_id` is a flat string; guild and
undercroft areas live under `world/areas/crossing/` in the source tree but carry unrelated
flat zone ids; no parentage exists in the database; `typeclasses/rooms.py` defaults unassigned
rooms to `default_region`.

**Mark 1 measurements** are not restated here; see `mark-1-scope.md` and
`2026-08-31-generator-spec.md`.
