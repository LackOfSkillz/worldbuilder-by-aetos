// Click a route onto the globe, describe each stop, and hand the result over to be built.
//
// **This is the smallest useful half of the visual area builder**, and it is deliberately the
// half that only a person can do. Deciding that a path leaves the shore here, bends around a
// stand of alders there, and ends at a slip on that headland is a judgement about a place. The
// room names, the descriptions and the exit graph are all derivable from those decisions; the
// decisions are not derivable from anything.
//
// So this file collects: an ordered list of points on the planet, each with a note in the
// author's own words, and nothing else. What it produces is an intent, not a map.
//
// **Order matters and is the author's, not the tool's.** Nodes are kept in the sequence they
// were clicked, because that sequence IS the path - a walk from the first to the last. Sorting
// them by anything would be the tool overruling the only thing it was told.
//
// **Every node carries the ground under it, read from the engine.** A route node in the water
// is either a dock or a mistake, and the difference is worth knowing while the author is still
// standing there rather than after an area has been generated from it.

/// Colours: the path, and the node under edit.
const PATH_COLOUR = "#ff9f1c";
const NODE_COLOUR = "#ffd166";
const SELECTED_COLOUR = "#ff4d6d";

/// One route being authored.
export class Route {
  constructor(name = "route") {
    this.name = name;
    this.nodes = [];
    this.selected = -1;
    this.serial = 0;
  }

  add(latitude, longitude, elevationM) {
    this.serial += 1;
    this.nodes.push({
      id: this.serial,
      latitude_deg: latitude,
      longitude_deg: longitude,
      elevation_m: elevationM,
      note: "",
    });
    this.selected = this.nodes.length - 1;
    return this.nodes[this.selected];
  }

  remove(index) {
    if (index < 0 || index >= this.nodes.length) return;
    this.nodes.splice(index, 1);
    if (this.selected >= this.nodes.length) this.selected = this.nodes.length - 1;
  }

  clear() {
    this.nodes = [];
    this.selected = -1;
  }

  /// Metres between consecutive nodes, and the total. The author is drawing distances
  /// whether or not they mean to, and a 40 km "path through the woods" is worth noticing
  /// before it becomes eighty rooms.
  legs(radiusM) {
    const out = [];
    for (let i = 1; i < this.nodes.length; i += 1) {
      out.push(haversine(this.nodes[i - 1], this.nodes[i], radiusM));
    }
    return { legs: out, total: out.reduce((a, b) => a + b, 0) };
  }

  toJSON(planet) {
    return {
      route_version: 1,
      name: this.name,
      planet,
      saved_at: new Date().toISOString(),
      nodes: this.nodes.map((node, index) => ({ ...node, order: index })),
    };
  }
}

function haversine(a, b, radiusM) {
  const toRad = (d) => (d * Math.PI) / 180;
  const dLat = toRad(b.latitude_deg - a.latitude_deg);
  const dLon = toRad(b.longitude_deg - a.longitude_deg);
  const la1 = toRad(a.latitude_deg);
  const la2 = toRad(b.latitude_deg);
  const h = Math.sin(dLat / 2) ** 2 + Math.cos(la1) * Math.cos(la2) * Math.sin(dLon / 2) ** 2;
  return 2 * Math.asin(Math.sqrt(h)) * radiusM;
}

/// Draw a route: a line through the nodes, and a numbered pin at each.
///
/// **One data source, created once and mutated.** The obvious version removed the previous
/// source and added a new one each time, and it leaked: `dataSources.add()` is ASYNCHRONOUS,
/// so a source captured and handed back for removal on the next redraw had sometimes not
/// been added yet, and removing it did nothing. Three sources called `wb-route` were live
/// after one clear, two of them still holding pins - a "cleared" route still on the globe.
///
/// Counting them is what found it. `viewer.dataSources.length` is the check that a redraw
/// is a redraw rather than an accumulation, and it belongs in any test of this file.
export function drawRoute(viewer, Cesium, route, source = null) {
  if (!source) {
    source = new Cesium.CustomDataSource("wb-route");
    viewer.dataSources.add(source);
  }
  source.entities.removeAll();

  if (route.nodes.length > 1) {
    source.entities.add({
      polyline: {
        positions: Cesium.Cartesian3.fromDegreesArray(
          route.nodes.flatMap((n) => [n.longitude_deg, n.latitude_deg]),
        ),
        width: 3,
        material: Cesium.Color.fromCssColorString(PATH_COLOUR).withAlpha(0.85),
        clampToGround: true,
      },
    });
  }

  route.nodes.forEach((node, index) => {
    const chosen = index === route.selected;
    const colour = Cesium.Color.fromCssColorString(chosen ? SELECTED_COLOUR : NODE_COLOUR);
    source.entities.add({
      name: `node ${index + 1}`,
      position: Cesium.Cartesian3.fromDegrees(node.longitude_deg, node.latitude_deg),
      point: {
        pixelSize: chosen ? 13 : 9,
        color: colour,
        outlineColor: Cesium.Color.BLACK.withAlpha(0.85),
        outlineWidth: 2,
        heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
        // The same rules the area pins follow: never depth-tested, never range-culled. A
        // node you cannot see from orbit is a node you will place twice.
        disableDepthTestDistance: Number.POSITIVE_INFINITY,
      },
      label: {
        text: node.note ? `${index + 1}. ${node.note.slice(0, 28)}` : `${index + 1}`,
        font: "12px system-ui, sans-serif",
        fillColor: Cesium.Color.WHITE,
        outlineColor: Cesium.Color.BLACK,
        outlineWidth: 3,
        style: Cesium.LabelStyle.FILL_AND_OUTLINE,
        pixelOffset: new Cesium.Cartesian2(0, -16),
        verticalOrigin: Cesium.VerticalOrigin.BOTTOM,
        disableDepthTestDistance: Number.POSITIVE_INFINITY,
        heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
      },
    });
  });

  return source;
}

/// Save a route to the server, so it can be read without anybody handing over a file.
export async function saveRoute(document_) {
  const response = await fetch("/routes/", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(document_),
  });
  if (!response.ok) throw new Error(`server refused: ${response.status}`);
  return response.json();
}
