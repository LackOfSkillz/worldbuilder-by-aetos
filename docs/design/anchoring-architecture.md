# The anchoring architecture

**This, and not tectonics, is the core of Worldbuilder.**

The planet generator is a subsystem. The product is the ability to take an Evennia world
somebody has already spent years building and put a planet around it without touching it.

## The top-level requirement

> Worldbuilder must be additive to existing Evennia games. Existing room graphs,
> coordinate systems, wilderness grids, exits, zones and builder workflows remain valid.
> Worldbuilder provides a deterministic planetary context around them, through anchored
> areas and coordinate adapters. Developers may place existing areas onto the generated
> world without rewriting or migrating those areas.

And underneath it:

> Planet coordinates are authoritative for generated planetary space. Existing local
> coordinates remain authoritative inside imported areas.

That second line resolves what looked like a genuine conflict. A globe wants to own
position; a fifteen-year-old MUD area has its own perfectly good representation and no
reason to give it up. Both are authoritative, in different domains, with a deterministic
transform between them.

This is the same rule maritime already lives by, arrived at independently: maritime is
additive and will not move a host's layout, money is the host's, the taxonomy of ships is
the host's, and the world is a seam. That convergence is the best evidence available that
the rule is right.

## Two coordinate domains

    PLANET SPACE                     LOCAL GAME SPACE
    spherical                        whatever the game already uses
    latitude / longitude             x/y rooms, wilderness grids,
    generated terrain                hand-built room graphs, or
    oceans, climate                  nothing at all
    exploration

## The bridge: anchors and seams

Two different things, deliberately kept apart.

**A `WorldAnchor` gives a zone geographic *context*** - roughly where it is, how big it
is, and optionally how its internal coordinates map onto the globe.

**A `WorldSeam` gives one room geographic *precision*** - an exact planetary position
where generated space connects to authored space.

    WorldAnchor                      WorldSeam
        zone_id                          zone_id
        footprint                        room_id
        local_transform | None           kind
        revision                         world_position
        authority                        heading | None
                                         metadata

The distinction is what lets an entirely abstract zone work. A town may be a fuzzy
12 x 8 km footprint with 900 rooms that have no coordinates at all - and its dock still
sits at an exact latitude and longitude, because a dock is a place ships sail *to*.

    Market                           Town footprint:
      |                                  approximately 12 x 8 km
    Inn - Town Square - Temple           (no room-level coordinates)
      |
    Docks                            Docks seam:
                                         lat, lon - exact shoreline position

The zone has geographic context. The seam has geographic precision.

### Seams are not port-specific

The same problem appears everywhere once a planet exists, so the concept is general from
the start:

    generated ocean      -> authored dock
    generated road       -> authored city gate
    generated river      -> authored ferry landing
    generated wilderness -> authored trailhead
    generated cave       -> authored dungeon entrance

Maritime asks for `seams(kind="port")`. A future wilderness system asks for
`seams(kind="road")`. Worldbuilder is then not merely surrounding existing content
geographically - it is connecting generated space to it.

**A port seam carries more than a position.** It has to answer "can *this* vessel enter?"
- approach depth, shelter, and the draft it will take. A seam that says where the dock is
but not that a frigate cannot get in will ground somebody who trusted it.

## Three integration levels

Evennia games do not represent geography the same way, and requiring them to would end
adoption before it began.

**Level 1 - topological.** No coordinates at all, just rooms and exits. The zone is
dropped onto the planet and gets a footprint; individual rooms have no geographic
position. Sufficient for climate, weather, sunrise, regional resources, which continent
the city is on, and - with seams - maritime ports. This covers a great many existing
games, and it is the case to build first.

**Level 2 - existing 2D coordinates.** The game already has `room.db.x` and `room.db.y`,
or a wilderness grid. Worldbuilder consumes them unchanged through an adapter; the anchor
supplies the transform. No migration.

**Level 3 - geospatially aware.** Rooms carry true planetary positions. Optional, never
the baseline.

`scale=None` is valid and important: *this area is located here, but its internal geometry
is abstract*. One room north is not the same distance everywhere, and pretending otherwise
would be a lie the generator then builds on.

## Authority, not terrain replacement

**The physical field is total.** `terrain_z_at` and `bottom_type_at` return an answer at
every point on the planet, always - under a castle, inside an abstract city, anywhere.
They may never answer "there is a city here".

For maritime this is not a nicety. A handcrafted harbour of authored rooms still needs the
planetary field to say *water 7.4 m, bottom mud*, or a ship cannot anchor in its own port.

So an anchored area's policy governs **representation authority**, never whether physical
geography exists:

| Policy | Meaning |
|---|---|
| `LOCAL_AUTHORITY` | authored room topology wins |
| `HYBRID_AUTHORITY` | authored topology, generated environmental context |
| `WORLD_AUTHORITY` | generated terrain primarily defines traversability |

The earlier names - preserve, blend, conform - were dropped because "preserve" sounds like
terrain replacement, which is exactly the confusion to avoid.

## Revisions, and why charts care

Both the generator and every anchored area carry an immutable version.

    WorldSpec(seed, generator_version="planet-v1", radius, sea_level)

    AnchoredAreaRevision(
        id="crossing-r1",
        generator_version="planet-v1",
        anchor_definition, footprint, seams,
    )

Moving a zone later does not modify `r1`; it creates `crossing-r2`.

This is not bookkeeping. **A survey records what it was made against**, so the game can
tell two things apart that would otherwise be indistinguishable:

    your chart is wrong because your navigator was careless
    this harbour was rebuilt between your voyages

The second becomes a story instead of a bug report. Charts in maritime are already
deliberately wrong in fixed places; provenance is what stops *changed* being mistaken for
*mistaken*.

## Never rewrite what the builder made

Absolute. If a zone has `Town Square north -> Market Street`, Worldbuilder does not touch
that exit because the generated geography disagrees. It may warn -

    The western edge of this zone overlaps generated ocean.
    [ Move Zone ]  [ Raise Local Terrain ]  [ Create Waterfront ]  [ Accept ]

- and the builder decides. Existing maps are frequently abstract on purpose.

## Generated land can become authored land

Players find a fine natural harbour; six months later the builders want a town there. That
must not require regenerating the planet. Selecting a region and creating an anchored area
turns the existing procedural terrain into its backdrop, and builders add permanent rooms.

Which makes this not a planet generator but a **world-expansion substrate**, and that is a
larger and more useful thing.

## Three kinds of world data

The cleanest statement of the whole architecture, and the thing that dissolves the
"everything must be a pure function" worry:

| Kind | Examples | Cheap arbitrary-coordinate query? |
|---|---|---|
| **Functional fields** | terrain, bathymetry, temperature, biome | **required** |
| **Generated structures** | plate seeds, rivers, gyres, settlements | no - generated once, stored small |
| **Authored structures** | existing zones, ports, builder edits | no - sparse |

Only the first kind must answer at any point in microseconds. Everything else is generated
once, deterministically, and stored - which is still deterministic. **Determinism is the
requirement; stateless scalar evaluation was only one implementation of it**, and treating
them as the same thing was an error in the first draft of this design.

## The whole picture

                        WORLD SPEC
                            |
                  deterministic planet
                            |
                +-----------+-----------+
                |                       |
         Functional Fields       Generated Structures
         terrain, bathymetry     rivers, gyres,
         climate, biome          settlements, roads
                |                       |
                +-----------+-----------+
                            |
                      PLANET SPACE
                            |
                      World Anchors
                            |
                 +----------+----------+
                 |                     |
           Anchored Areas          World Seams
           existing zones          precise connections
           room graphs             ports, gates, roads,
           wilderness grids        ferries, entrances
                 |                     |
                 +----------+----------+
                            |
                        Seam Layer
                            |
                 travel / ports / roads

## What to build first

**Not tectonics.** A deliberately ugly planet is enough to prove the product claim, and
the product claim is the risky part. Building continents first risks discovering months
later that imported areas do not fit the model.

The first vertical slice:

1. Generate a planet - trivial terrain is fine.
2. Register an existing Evennia area without modifying it.
3. Give the area an abstract footprint.
4. Add one exact seam room, a harbour.
5. Display the area on the globe.
6. Query generated terrain around and underneath it.
7. Move through generated planet space to the seam.
8. Cross the seam into the existing room graph.
9. Leave through the seam and return to generated space.
10. Save, reload, and prove identical placement.
11. **Prove maritime sails to that seam through the provider interface alone**, knowing
    nothing about Worldbuilder's types.

Step 10 is the determinism test. Step 11 is the integration claim, and it is the one that
would embarrass us if the seam turned out to need a back channel.

**Make the first demo deliberately awkward: use a coordinate-less room graph.** If Level 1
works, a clean 2D wilderness grid is easy by comparison. Building the easy case first
would prove nothing.
