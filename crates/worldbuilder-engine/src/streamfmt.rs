//! The stream graph's on-disk format: a section table, a header, and a reader that fails
//! closed.
//!
//! # Two things this format must do
//!
//! **1. Fail closed on an unsupported generator version.** VERSION-001 is strictly binary
//! (`lib.rs`, `GENERATOR_VERSION`): supported means evaluate, unsupported means refuse.
//! There is no partial compatibility, no negotiation and no compatibility matrix, which is
//! why the version is a bare `u32` and why every refusal below is a hard `Err` rather than
//! a warning, a default, or a best-effort read.
//!
//! **2. Read a region without parsing the whole file.** This is forced, not chosen. Task 4
//! measured a 20,000,000-node graph at 1.45 GB of arrays and 2.16 GB peak RSS, and found
//! `node_neighbours`' nested vector exceeding a gigabyte on its own at that size. Neither
//! fits a 32-bit WASM heap under any field arrangement, so the browser must load a *region*
//! and never the planet. Retrofitting that later is exactly the change VERSION-001 makes
//! expensive, so it belongs in the first version of the format.
//!
//! Everything that follows from those two:
//!
//! - Every per-node array is its own **section**: fixed-width, contiguous, in node order.
//!   Element `i` lives at `offset + i * elem_width` and nowhere else, so the byte range for
//!   nodes `[first, first + count)` is arithmetic on the section table. No compression, no
//!   delta coding, no variable-length records — each of those would make element `i`'s
//!   position depend on elements `0..i`, which is precisely what a region read must not
//!   need. That is what region-sliceability costs here, and it is the whole cost.
//! - The section table is fixed-size and sits immediately after the header, so a client
//!   fetches a **288-byte prefix**, learns the whole layout, and then asks for exactly the
//!   bytes it wants. `GraphReader::open` takes that prefix and a file length — it never
//!   requires the payload — and `GraphReader::node_byte_range` turns a node range into a
//!   byte range.
//! - Sections are struct-of-arrays, matching `StreamGraph`. A reader that wants only
//!   heights pays for heights alone; the price is that a region costs one seek per column
//!   (five) rather than one per node.
//!
//! # Serialise the flags, never the rule that derived them
//!
//! `StreamGraph::build` classifies a root as `MOUTH` or `LAKE_MEMBER` with a sea-level
//! test. **That test is a default classifier, not part of the model**, and it is physically
//! wrong in two named cases: a submarine local minimum becomes a "mouth", and a land
//! depression below the datum — the Dead Sea, Death Valley, the Qattara Depression — is a
//! lake that the test calls a mouth. Slice 5 must be able to replace that classifier
//! *without a format or generator version bump*.
//!
//! So this format stores the **flags**, and the invariant that travels with them is section
//! 14.2's actual claim — *every root is exactly one of MOUTH or LAKE* — and never "root and
//! below-datum implies MOUTH". Nothing in this module compares an elevation against the
//! datum; `the_format_never_applies_the_datum_classifier` scans this source to keep it that
//! way, and a file whose below-datum root is a lake reads back as a lake. `sea_level_m` is
//! carried in the header regardless, because the datum a classification was made at is not
//! recoverable from the flags and a graph read at a different datum is wrong without being
//! malformed.
//!
//! `LAKE_MEMBER` is deliberately **not** required to imply a lake record: slice 1p sets it
//! on roots only, and slice 5 will set it on every member of a filled basin. The record is
//! keyed on the root, so the invariant is checked at roots and nowhere else.
//!
//! # What is not in the format
//!
//! Positions are not stored: the header carries Task 3's `position_checksum` and a reader
//! regenerates them (8 bytes rather than 24 per node). `GraphReader::verify_sampling`
//! is that regeneration, and `positions_match` is the comparison for a caller who already
//! has the nodes; both call `stream::position_checksum`, which is public **so that this
//! module never grows a second copy of the hash**. A second copy would agree until one was
//! touched, and then every existing worldfile would fail its own checksum. That also makes
//! `sampling_kind` a *verifiable* field rather than a recorded claim: the sampler named in
//! the header, run on the seed and count in the header, must hash to the checksum in the
//! header, so a file that lies about any of the three is refused. `SamplingKind::Supplied`
//! declines to verify rather than passing, because this crate cannot reproduce a node set
//! it was handed. Deferred deliberately, all of it
//! recomputable or slice 5's: the lake super-graph beyond the root, thermal-correction
//! state, uplift, erodibility, anything derived from receivers, reach geometry, further
//! flag bits. `pond_max_drainage_area_m2` is a *build parameter with no default* (Task 4
//! refused to invent one) and is deliberately absent — it produced `LakeKind`, and
//! `LakeKind` is what is stored.

use crate::sphere::SpherePoint;
use crate::stream::{
    flag, node_positions, position_checksum, GraphHeader, Lake, LakeKind, Reach, SamplingKind,
    StreamGraph, MAX_NODES, NO_DOWNHILL, NO_LAKE,
};
use core::ops::Range;

/// The file's first eight bytes. Not a version: the version is a field, so that a bumped
/// version is *read and refused* rather than mistaken for a different kind of file.
pub const MAGIC: [u8; 8] = *b"WBSTRMG\0";

/// The **format** version, which bumps when the byte layout changes — never when a world
/// changes. Kept rigorously distinct from `crate::GENERATOR_VERSION`, per `lib.rs`:
/// conflating them would force every existing worldfile through a generator migration it
/// does not need every time a section is appended.
pub const FORMAT_VERSION: u32 = 1;

/// Bytes in the fixed header.
pub const HEADER_BYTES: usize = 64;

/// Bytes in one section-table entry.
pub const SECTION_ENTRY_BYTES: usize = 32;

/// Every section payload starts on this boundary, so a future zero-copy reader can view a
/// section as a slice of `f64` or `u32` without a realignment copy. The cost is up to seven
/// padding bytes per section — at most 49 bytes for a whole file, at any node count.
pub const SECTION_ALIGN: u64 = 8;

// Header field offsets. Named because the failure-path tests patch them by hand, and a test
// that computes its own offsets from the writer proves nothing about the layout.
pub const OFF_MAGIC: usize = 0;
pub const OFF_FORMAT_VERSION: usize = 8;
pub const OFF_GENERATOR_VERSION: usize = 12;
pub const OFF_WORLD_SEED: usize = 16;
pub const OFF_RADIUS_M: usize = 24;
pub const OFF_SEA_LEVEL_M: usize = 32;
pub const OFF_POSITION_CHECKSUM: usize = 40;
pub const OFF_NODE_COUNT: usize = 48;
pub const OFF_SECTION_COUNT: usize = 52;
pub const OFF_SAMPLING_KIND: usize = 56;
pub const OFF_HEADER_RESERVED: usize = 57;

// Section-table entry field offsets, from the start of the entry.
pub const ENTRY_OFF_KIND: usize = 0;
pub const ENTRY_OFF_ELEM_WIDTH: usize = 4;
pub const ENTRY_OFF_ELEM_COUNT: usize = 8;
pub const ENTRY_OFF_OFFSET: usize = 16;
pub const ENTRY_OFF_BYTE_LEN: usize = 24;

/// Bytes in one lake record: `root_node u32`, `outflow_lake u32`, `level_m f64`, `kind u8`,
/// seven reserved bytes that must read zero.
pub const LAKE_RECORD_BYTES: u32 = 24;

/// Bytes in one reach record: `from_node u32`, `to_node u32`, `gradient f64`.
pub const REACH_RECORD_BYTES: u32 = 16;

/// The sections. The discriminants are the on-disk codes and are never renumbered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum SectionKind {
    HeightM = 1,
    AreaM2 = 2,
    Downhill = 3,
    DrainageAreaM2 = 4,
    Flags = 5,
    Lakes = 6,
    Reaches = 7,
}

/// Every section, in the order the writer emits them. All seven are mandatory; because the
/// table holds exactly seven entries, each a known kind and none repeated, "all present"
/// follows by pigeonhole and needs no separate check.
pub const SECTIONS: [SectionKind; 7] = [
    SectionKind::HeightM,
    SectionKind::AreaM2,
    SectionKind::Downhill,
    SectionKind::DrainageAreaM2,
    SectionKind::Flags,
    SectionKind::Lakes,
    SectionKind::Reaches,
];

/// The five per-node sections. A region read touches exactly these.
pub const NODE_SECTIONS: [SectionKind; 5] = [
    SectionKind::HeightM,
    SectionKind::AreaM2,
    SectionKind::Downhill,
    SectionKind::DrainageAreaM2,
    SectionKind::Flags,
];

/// Bytes per node across all five per-node sections: 8 + 8 + 4 + 8 + 1.
pub const REGION_BYTES_PER_NODE: u64 = 29;

impl SectionKind {
    pub fn code(self) -> u32 {
        self as u32 // cast-ok: an enum discriminant declared #[repr(u32)]
    }

    pub fn from_code(code: u32) -> Option<Self> {
        match code {
            1 => Some(SectionKind::HeightM),
            2 => Some(SectionKind::AreaM2),
            3 => Some(SectionKind::Downhill),
            4 => Some(SectionKind::DrainageAreaM2),
            5 => Some(SectionKind::Flags),
            6 => Some(SectionKind::Lakes),
            7 => Some(SectionKind::Reaches),
            _ => None,
        }
    }

    /// The element width the format fixes for this section. A file that disagrees is
    /// refused rather than trusted: a wrong width silently reinterprets every element after
    /// the first, which is the kind of corruption that produces a plausible wrong world.
    pub fn elem_width(self) -> u32 {
        match self {
            SectionKind::HeightM | SectionKind::AreaM2 | SectionKind::DrainageAreaM2 => 8,
            SectionKind::Downhill => 4,
            SectionKind::Flags => 1,
            SectionKind::Lakes => LAKE_RECORD_BYTES,
            SectionKind::Reaches => REACH_RECORD_BYTES,
        }
    }

    /// True when the section has exactly one element per node.
    pub fn is_per_node(self) -> bool {
        !matches!(self, SectionKind::Lakes | SectionKind::Reaches)
    }
}

/// One row of the section table, as read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub kind: SectionKind,
    pub elem_width: u32,
    pub elem_count: u64,
    pub offset: u64,
    pub byte_len: u64,
}

impl Section {
    pub fn range(&self) -> Range<u64> {
        self.offset..self.offset + self.byte_len
    }
}

/// Why a reader refused. Every variant is a refusal; none of them means "read it anyway".
#[derive(Debug, Clone, PartialEq)]
pub enum FormatError {
    /// Fewer bytes than the structure being read requires.
    TooShort { need: usize, got: usize },
    BadMagic { found: [u8; 8] },
    /// VERSION-001, for the byte layout.
    UnsupportedFormatVersion { found: u32, supported: u32 },
    /// VERSION-001, for the generator.
    UnsupportedGeneratorVersion { found: u32, supported: u32 },
    UnknownSamplingKind { found: u8 },
    ReservedBytesNotZero { what: &'static str, index: u64 },
    NonFiniteHeaderField { what: &'static str, bits: u64 },
    NodeCountTooLarge { count: u32, max: u32 },
    SectionCountWrong { found: u32, expected: u32 },
    UnknownSectionKind { code: u32 },
    DuplicateSection { code: u32 },
    SectionWidthWrong { kind: SectionKind, found: u32, expected: u32 },
    /// `byte_len` disagrees with `elem_width * elem_count`.
    SectionLengthInconsistent {
        kind: SectionKind,
        elem_width: u32,
        elem_count: u64,
        byte_len: u64,
    },
    /// A per-node section whose element count is not the header's node count.
    SectionElementCountWrong { kind: SectionKind, found: u64, expected: u64 },
    SectionMisaligned { kind: SectionKind, offset: u64, align: u64 },
    /// A payload that would sit inside the header or the section table.
    SectionInsidePrefix { kind: SectionKind, offset: u64 },
    /// The section table points past the end of the file.
    SectionOutOfBounds { kind: SectionKind, offset: u64, byte_len: u64, file_len: u64 },
    /// A 64-bit offset or length this host cannot address. Reachable on wasm32, which is
    /// the target this whole format exists for.
    OffsetTooLargeForHost { value: u64 },
    NotAPerNodeSection { kind: SectionKind },
    RegionOutOfRange { first_node: u32, count: u32, node_count: u32 },
    /// Garbage where a float should be.
    NonFiniteValue { kind: SectionKind, index: u64, bits: u64 },
    DownhillOutOfRange { node: u32, target: u32 },
    ReservedFlagBitSet { node: u32, bits: u8 },
    UnknownLakeKind { index: u64, found: u8 },
    LakeAtNonRoot { node: u32 },
    LakeRootOutOfRange { node: u32 },
    DuplicateLakeRoot { node: u32 },
    OutflowLakeOutOfRange { index: u64, target: u32 },
    /// Section 14.2's actual claim, and the only classification invariant this format
    /// enforces. **Not** the datum rule that produced the flags.
    RootIsNotExactlyOneClass { node: u32, mouth: bool, lake: bool },
    /// A root carries `LAKE_MEMBER` without a record, or a record without the flag. Checked
    /// at roots only: slice 5 sets the flag on non-root members, which have no record.
    LakeFlagRecordMismatch { node: u32, flagged: bool, record: bool },
    MouthAtNonRoot { node: u32 },
    ReachEndpointOutOfRange { index: u64, node: u32 },
    /// A regenerated node set does not hash to the checksum the header declares. The
    /// positions are not in the file, so this is the only thing standing between a reader
    /// and a graph whose edges refer to somewhere else entirely.
    PositionChecksumMismatch { found: u64, expected: u64 },
    /// The header's `SamplingKind` names no sampler this crate can run, so the positions
    /// cannot be regenerated and the checksum cannot be checked. `Supplied` is the case:
    /// the caller had the nodes, and only the caller can produce them again.
    PositionsNotRegenerable { kind: SamplingKind },
}

/// A contiguous run of nodes, decoded. `downhill` holds **global** node indices, not
/// region-relative ones, and may point outside the region — flow leaves a region, and a
/// renumbering would make one region's edges meaningless against another's.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeRegion {
    pub first_node: u32,
    pub height_m: Vec<f64>,
    pub area_m2: Vec<f64>,
    pub downhill: Vec<u32>,
    pub drainage_area_m2: Vec<f64>,
    pub flags: Vec<u8>,
}

impl NodeRegion {
    pub fn len(&self) -> usize {
        self.height_m.len()
    }

    pub fn is_empty(&self) -> bool {
        self.height_m.is_empty()
    }
}

/// The bytes a client fetched for a region, one slice per per-node section. Separate slices
/// rather than one buffer because the five ranges are not adjacent in the file — which is
/// the whole point of the layout.
#[derive(Debug, Clone, Copy)]
pub struct RegionBytes<'a> {
    pub height_m: &'a [u8],
    pub area_m2: &'a [u8],
    pub downhill: &'a [u8],
    pub drainage_area_m2: &'a [u8],
    pub flags: &'a [u8],
}

/// A whole graph, decoded. Deliberately **not** a `StreamGraph`: that type's invariants are
/// established by `build`, and handing back a `StreamGraph` assembled from bytes nobody
/// rebuilt would let a corrupt file wear a validated type's name.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedGraph {
    pub header: GraphHeader,
    pub nodes: NodeRegion,
    pub lakes: Vec<Lake>,
    pub reaches: Vec<Reach>,
}

/// A file's layout, read from its prefix alone.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphReader {
    header: GraphHeader,
    format_version: u32,
    sections: Vec<Section>,
    file_len: u64,
}

/// The number of prefix bytes a client must fetch before it knows the layout: the header
/// plus the whole section table. Constant, and that is the point.
pub const fn prefix_len() -> usize {
    HEADER_BYTES + SECTIONS.len() * SECTION_ENTRY_BYTES
}

// ---- small helpers -------------------------------------------------------------------

/// Every fail-closed guard in this module is one of these, on one line, so that a mutation
/// campaign can disable exactly one check at a time and see which test notices.
fn ensure(ok: bool, err: impl FnOnce() -> FormatError) -> Result<(), FormatError> {
    if ok {
        Ok(())
    } else {
        Err(err())
    }
}

fn u64_of(x: usize) -> u64 {
    x as u64 // cast-ok: a container length; usize is at most 64 bits on every target
}

fn usize_of(v: u64) -> Result<usize, FormatError> {
    usize::try_from(v).map_err(|_| FormatError::OffsetTooLargeForHost { value: v })
}

fn slice_at(bytes: &[u8], off: usize, len: usize) -> Result<&[u8], FormatError> {
    let end = off
        .checked_add(len)
        .ok_or(FormatError::OffsetTooLargeForHost { value: u64::MAX })?;
    ensure(end <= bytes.len(), || FormatError::TooShort { need: end, got: bytes.len() })?;
    Ok(&bytes[off..end])
}

fn read_u8(bytes: &[u8], off: usize) -> Result<u8, FormatError> {
    Ok(slice_at(bytes, off, 1)?[0])
}

fn read_u32(bytes: &[u8], off: usize) -> Result<u32, FormatError> {
    let raw: [u8; 4] = slice_at(bytes, off, 4)?.try_into().expect("exactly four bytes");
    Ok(u32::from_le_bytes(raw))
}

fn read_u64(bytes: &[u8], off: usize) -> Result<u64, FormatError> {
    let raw: [u8; 8] = slice_at(bytes, off, 8)?.try_into().expect("exactly eight bytes");
    Ok(u64::from_le_bytes(raw))
}

fn read_f64(bytes: &[u8], off: usize) -> Result<f64, FormatError> {
    Ok(f64::from_bits(read_u64(bytes, off)?))
}

fn align_up(value: u64) -> u64 {
    let remainder = value % SECTION_ALIGN;
    if remainder == 0 {
        value
    } else {
        value + (SECTION_ALIGN - remainder)
    }
}

fn sampling_kind_code(kind: SamplingKind) -> u8 {
    match kind {
        SamplingKind::Supplied => 0,
        SamplingKind::Spiral => 1,
    }
}

fn sampling_kind_from_code(code: u8) -> Option<SamplingKind> {
    match code {
        0 => Some(SamplingKind::Supplied),
        1 => Some(SamplingKind::Spiral),
        _ => None,
    }
}

fn lake_kind_code(kind: LakeKind) -> u8 {
    match kind {
        LakeKind::Pond => 0,
        LakeKind::Lake => 1,
    }
}

fn lake_kind_from_code(code: u8) -> Option<LakeKind> {
    match code {
        0 => Some(LakeKind::Pond),
        1 => Some(LakeKind::Lake),
        _ => None,
    }
}

// ---- writer --------------------------------------------------------------------------

/// The section table `write_graph` will emit, computed without emitting it. Public because
/// it is also the definition of the layout.
pub fn section_table(node_count: u32, lake_count: u64, reach_count: u64) -> Vec<Section> {
    let mut offset = u64_of(prefix_len());
    let mut table = Vec::with_capacity(SECTIONS.len());
    for kind in SECTIONS {
        let elem_count = match kind {
            SectionKind::Lakes => lake_count,
            SectionKind::Reaches => reach_count,
            _ => u64::from(node_count),
        };
        let elem_width = kind.elem_width();
        let byte_len = u64::from(elem_width) * elem_count;
        table.push(Section { kind, elem_width, elem_count, offset, byte_len });
        offset = align_up(offset + byte_len);
    }
    table
}

/// The byte length `write_graph` will produce, without producing it.
pub fn encoded_len(node_count: u32, lake_count: u64, reach_count: u64) -> u64 {
    let table = section_table(node_count, lake_count, reach_count);
    let last = table[table.len() - 1];
    align_up(last.offset + last.byte_len)
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_f64(out: &mut Vec<u8>, value: f64) {
    out.extend_from_slice(&value.to_bits().to_le_bytes());
}

fn pad_to(out: &mut Vec<u8>, offset: u64) {
    while u64_of(out.len()) < offset {
        out.push(0);
    }
}

/// Serialise a graph. Infallible: a `StreamGraph` exists only if `build` validated it, so
/// there is no state here that could fail to encode.
pub fn write_graph(graph: &StreamGraph) -> Vec<u8> {
    let header = graph.header();
    let node_count = header.node_count;
    let lakes = graph.lakes();
    let reaches = graph.reaches();
    let table = section_table(node_count, u64_of(lakes.len()), u64_of(reaches.len()));
    let total = encoded_len(node_count, u64_of(lakes.len()), u64_of(reaches.len()));

    let mut out: Vec<u8> = Vec::with_capacity(usize::try_from(total).unwrap_or(0));
    out.extend_from_slice(&MAGIC);
    push_u32(&mut out, FORMAT_VERSION);
    push_u32(&mut out, header.generator_version);
    push_u64(&mut out, header.world_seed);
    push_f64(&mut out, header.radius_m);
    push_f64(&mut out, header.sea_level_m);
    push_u64(&mut out, header.position_checksum);
    push_u32(&mut out, node_count);
    push_u32(&mut out, u32::try_from(SECTIONS.len()).expect("seven sections"));
    out.push(sampling_kind_code(header.sampling_kind));
    out.extend_from_slice(&[0u8; 7]);
    debug_assert_eq!(out.len(), HEADER_BYTES);

    for section in &table {
        push_u32(&mut out, section.kind.code());
        push_u32(&mut out, section.elem_width);
        push_u64(&mut out, section.elem_count);
        push_u64(&mut out, section.offset);
        push_u64(&mut out, section.byte_len);
    }
    debug_assert_eq!(out.len(), prefix_len());

    for section in &table {
        pad_to(&mut out, section.offset);
        debug_assert_eq!(u64_of(out.len()), section.offset);
        match section.kind {
            SectionKind::HeightM => {
                for node in 0..node_count {
                    push_f64(&mut out, graph.height_m(node));
                }
            }
            SectionKind::AreaM2 => {
                for node in 0..node_count {
                    push_f64(&mut out, graph.area_m2(node));
                }
            }
            SectionKind::Downhill => {
                for node in 0..node_count {
                    push_u32(&mut out, graph.downhill_raw(node));
                }
            }
            SectionKind::DrainageAreaM2 => {
                for node in 0..node_count {
                    push_f64(&mut out, graph.drainage_area_m2(node));
                }
            }
            SectionKind::Flags => {
                for node in 0..node_count {
                    out.push(graph.flags_of(node));
                }
            }
            SectionKind::Lakes => {
                for lake in lakes {
                    push_u32(&mut out, lake.root_node);
                    push_u32(&mut out, lake.outflow_lake);
                    push_f64(&mut out, lake.level_m);
                    out.push(lake_kind_code(lake.kind));
                    out.extend_from_slice(&[0u8; 7]);
                }
            }
            SectionKind::Reaches => {
                for reach in reaches {
                    push_u32(&mut out, reach.from_node);
                    push_u32(&mut out, reach.to_node);
                    push_f64(&mut out, reach.gradient);
                }
            }
        }
    }
    pad_to(&mut out, total);
    debug_assert_eq!(u64_of(out.len()), total);
    out
}

// ---- reader --------------------------------------------------------------------------

impl GraphReader {
    /// Read the header and section table from `prefix`, validating both against a file of
    /// `file_len` bytes. **`prefix` need only be `prefix_len()` bytes long** — this is the
    /// region-sliceability entry point, and passing the whole file is a convenience rather
    /// than a requirement.
    pub fn open(prefix: &[u8], file_len: u64) -> Result<Self, FormatError> {
        ensure(prefix.len() >= prefix_len(), || FormatError::TooShort { need: prefix_len(), got: prefix.len() })?; // MUT-01

        let magic: [u8; 8] = slice_at(prefix, OFF_MAGIC, 8)?.try_into().expect("eight bytes");
        ensure(magic == MAGIC, || FormatError::BadMagic { found: magic })?; // MUT-02

        let format_version = read_u32(prefix, OFF_FORMAT_VERSION)?;
        ensure(format_version == FORMAT_VERSION, || FormatError::UnsupportedFormatVersion { found: format_version, supported: FORMAT_VERSION })?; // MUT-03

        let generator_version = read_u32(prefix, OFF_GENERATOR_VERSION)?;
        ensure(generator_version == crate::GENERATOR_VERSION, || FormatError::UnsupportedGeneratorVersion { found: generator_version, supported: crate::GENERATOR_VERSION })?; // MUT-04

        let world_seed = read_u64(prefix, OFF_WORLD_SEED)?;
        let radius_m = read_f64(prefix, OFF_RADIUS_M)?;
        ensure(radius_m.is_finite(), || FormatError::NonFiniteHeaderField { what: "radius_m", bits: radius_m.to_bits() })?; // MUT-05
        let sea_level_m = read_f64(prefix, OFF_SEA_LEVEL_M)?;
        ensure(sea_level_m.is_finite(), || FormatError::NonFiniteHeaderField { what: "sea_level_m", bits: sea_level_m.to_bits() })?; // MUT-06
        let position_checksum = read_u64(prefix, OFF_POSITION_CHECKSUM)?;

        let node_count = read_u32(prefix, OFF_NODE_COUNT)?;
        ensure(node_count <= MAX_NODES, || FormatError::NodeCountTooLarge { count: node_count, max: MAX_NODES })?; // MUT-07

        let section_count = read_u32(prefix, OFF_SECTION_COUNT)?;
        let expected_sections = u32::try_from(SECTIONS.len()).expect("seven sections");
        ensure(section_count == expected_sections, || FormatError::SectionCountWrong { found: section_count, expected: expected_sections })?; // MUT-08

        let sampling_code = read_u8(prefix, OFF_SAMPLING_KIND)?;
        let sampling_kind = match sampling_kind_from_code(sampling_code) {
            Some(kind) => kind,
            None => return Err(FormatError::UnknownSamplingKind { found: sampling_code }), // MUT-09
        };

        let reserved = slice_at(prefix, OFF_HEADER_RESERVED, HEADER_BYTES - OFF_HEADER_RESERVED)?;
        for (index, byte) in reserved.iter().enumerate() {
            ensure(*byte == 0, || FormatError::ReservedBytesNotZero { what: "header", index: u64_of(index) })?; // MUT-10
        }

        let mut sections: Vec<Section> = Vec::with_capacity(SECTIONS.len());
        for entry in 0..SECTIONS.len() {
            let base = HEADER_BYTES + entry * SECTION_ENTRY_BYTES;
            let code = read_u32(prefix, base + ENTRY_OFF_KIND)?;
            let kind = match SectionKind::from_code(code) {
                Some(kind) => kind,
                None => return Err(FormatError::UnknownSectionKind { code }), // MUT-11
            };
            ensure(!sections.iter().any(|s| s.kind == kind), || FormatError::DuplicateSection { code })?; // MUT-12

            let elem_width = read_u32(prefix, base + ENTRY_OFF_ELEM_WIDTH)?;
            ensure(elem_width == kind.elem_width(), || FormatError::SectionWidthWrong { kind, found: elem_width, expected: kind.elem_width() })?; // MUT-13

            let elem_count = read_u64(prefix, base + ENTRY_OFF_ELEM_COUNT)?;
            let offset = read_u64(prefix, base + ENTRY_OFF_OFFSET)?;
            let byte_len = read_u64(prefix, base + ENTRY_OFF_BYTE_LEN)?;

            let computed = u64::from(elem_width).checked_mul(elem_count);
            ensure(computed == Some(byte_len), || FormatError::SectionLengthInconsistent { kind, elem_width, elem_count, byte_len })?; // MUT-14

            if kind.is_per_node() {
                ensure(elem_count == u64::from(node_count), || FormatError::SectionElementCountWrong { kind, found: elem_count, expected: u64::from(node_count) })?; // MUT-15
            }

            ensure(offset % SECTION_ALIGN == 0, || FormatError::SectionMisaligned { kind, offset, align: SECTION_ALIGN })?; // MUT-16
            ensure(offset >= u64_of(prefix_len()), || FormatError::SectionInsidePrefix { kind, offset })?; // MUT-17

            let end = offset.checked_add(byte_len);
            ensure(end.is_some() && end <= Some(file_len), || FormatError::SectionOutOfBounds { kind, offset, byte_len, file_len })?; // MUT-18

            sections.push(Section { kind, elem_width, elem_count, offset, byte_len });
        }

        Ok(GraphReader {
            header: GraphHeader {
                generator_version,
                world_seed,
                radius_m,
                node_count,
                sampling_kind,
                sea_level_m,
                position_checksum,
            },
            format_version,
            sections,
            file_len,
        })
    }

    /// `open` against a buffer that is the whole file.
    pub fn open_whole(bytes: &[u8]) -> Result<Self, FormatError> {
        GraphReader::open(bytes, u64_of(bytes.len()))
    }

    pub fn header(&self) -> &GraphHeader {
        &self.header
    }

    pub fn format_version(&self) -> u32 {
        self.format_version
    }

    pub fn file_len(&self) -> u64 {
        self.file_len
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// Every kind is present exactly once by the time a reader exists (seven entries, all
    /// known kinds, no duplicates), so this cannot fail.
    pub fn section(&self, kind: SectionKind) -> Section {
        *self
            .sections
            .iter()
            .find(|s| s.kind == kind)
            .expect("every section kind is present by pigeonhole")
    }

    /// The byte range holding nodes `[first_node, first_node + count)` of one per-node
    /// section. Pure arithmetic on the section table: no payload is touched, which is what
    /// makes a range request possible from the prefix alone.
    pub fn node_byte_range(
        &self,
        kind: SectionKind,
        first_node: u32,
        count: u32,
    ) -> Result<Range<u64>, FormatError> {
        ensure(kind.is_per_node(), || FormatError::NotAPerNodeSection { kind })?; // MUT-19
        let last = first_node.checked_add(count);
        ensure(last.is_some() && last <= Some(self.header.node_count), || FormatError::RegionOutOfRange { first_node, count, node_count: self.header.node_count })?; // MUT-20
        let section = self.section(kind);
        let width = u64::from(section.elem_width);
        let start = section.offset + u64::from(first_node) * width;
        Ok(start..start + u64::from(count) * width)
    }

    /// The five ranges a region read needs, in `NODE_SECTIONS` order.
    pub fn region_byte_ranges(
        &self,
        first_node: u32,
        count: u32,
    ) -> Result<Vec<Range<u64>>, FormatError> {
        let mut out = Vec::with_capacity(NODE_SECTIONS.len());
        for kind in NODE_SECTIONS {
            out.push(self.node_byte_range(kind, first_node, count)?);
        }
        Ok(out)
    }

    fn expect_len(&self, kind: SectionKind, got: usize, count: u32) -> Result<(), FormatError> {
        let need = usize_of(u64::from(count) * u64::from(kind.elem_width()))?;
        ensure(got == need, || FormatError::TooShort { need, got }) // MUT-21
    }

    fn decode_floats(
        &self,
        kind: SectionKind,
        first_node: u32,
        count: u32,
        bytes: &[u8],
    ) -> Result<Vec<f64>, FormatError> {
        self.expect_len(kind, bytes.len(), count)?;
        let mut out = Vec::with_capacity(bytes.len() / 8);
        for element in 0..count {
            let offset = usize_of(u64::from(element) * 8)?;
            let value = read_f64(bytes, offset)?;
            let index = u64::from(first_node) + u64::from(element);
            ensure(value.is_finite(), || FormatError::NonFiniteValue { kind, index, bits: value.to_bits() })?; // MUT-22
            out.push(value);
        }
        Ok(out)
    }

    /// Decode a region from bytes a client fetched itself.
    pub fn decode_region(
        &self,
        first_node: u32,
        count: u32,
        bytes: &RegionBytes<'_>,
    ) -> Result<NodeRegion, FormatError> {
        let last = first_node.checked_add(count);
        ensure(last.is_some() && last <= Some(self.header.node_count), || FormatError::RegionOutOfRange { first_node, count, node_count: self.header.node_count })?; // MUT-23

        let height_m = self.decode_floats(SectionKind::HeightM, first_node, count, bytes.height_m)?;
        let area_m2 = self.decode_floats(SectionKind::AreaM2, first_node, count, bytes.area_m2)?;
        let drainage_area_m2 = self.decode_floats(
            SectionKind::DrainageAreaM2,
            first_node,
            count,
            bytes.drainage_area_m2,
        )?;

        self.expect_len(SectionKind::Downhill, bytes.downhill.len(), count)?;
        let mut downhill = Vec::with_capacity(bytes.downhill.len() / 4);
        for element in 0..count {
            let node = first_node + element;
            let target = read_u32(bytes.downhill, usize_of(u64::from(element) * 4)?)?;
            ensure(target == NO_DOWNHILL || target < self.header.node_count, || FormatError::DownhillOutOfRange { node, target })?; // MUT-24
            downhill.push(target);
        }

        self.expect_len(SectionKind::Flags, bytes.flags.len(), count)?;
        let mut flags = Vec::with_capacity(bytes.flags.len());
        for element in 0..count {
            let node = first_node + element;
            let value = read_u8(bytes.flags, usize_of(u64::from(element))?)?;
            let extra = value & !flag::DEFINED;
            ensure(extra == 0, || FormatError::ReservedFlagBitSet { node, bits: extra })?; // MUT-25
            flags.push(value);
        }

        Ok(NodeRegion { first_node, height_m, area_m2, downhill, drainage_area_m2, flags })
    }

    fn slice_of<'a>(&self, whole: &'a [u8], range: Range<u64>) -> Result<&'a [u8], FormatError> {
        let start = usize_of(range.start)?;
        let end = usize_of(range.end)?;
        ensure(end <= whole.len(), || FormatError::TooShort { need: end, got: whole.len() })?; // MUT-26
        Ok(&whole[start..end])
    }

    /// Decode a region from a buffer holding the whole file. Reads only the region's bytes.
    pub fn read_region(
        &self,
        whole: &[u8],
        first_node: u32,
        count: u32,
    ) -> Result<NodeRegion, FormatError> {
        let ranges = self.region_byte_ranges(first_node, count)?;
        let height_m = self.slice_of(whole, ranges[0].clone())?;
        let area_m2 = self.slice_of(whole, ranges[1].clone())?;
        let downhill = self.slice_of(whole, ranges[2].clone())?;
        let drainage_area_m2 = self.slice_of(whole, ranges[3].clone())?;
        let flags = self.slice_of(whole, ranges[4].clone())?;
        self.decode_region(
            first_node,
            count,
            &RegionBytes { height_m, area_m2, downhill, drainage_area_m2, flags },
        )
    }

    /// The lake table, read whole or not at all.
    ///
    /// **The figure that justified "whole" was wrong for this crate, and the conclusion
    /// survives anyway.** It was the extraction's §8.3 5,647 roots at 20,000,000 nodes,
    /// which would be a 135,528-byte section. Task 6 measured this crate's own `Surface` at
    /// that size: 597,687 roots, of which 225,821 are lakes, so the section is **5,419,704
    /// bytes** -- forty times larger. It is still 0.93% of a 585,419,992-byte file, so
    /// reading it whole is still right, and a client that wants only a region still never
    /// touches it. But 5.4 MB is a fetch a browser notices, and slice 5 -- which populates
    /// `outflow_lake` and turns this table into a graph -- should expect to make it
    /// region-sliceable too. Recorded rather than fixed: changing it now would be a
    /// format-version bump for a cost nothing yet pays.
    pub fn read_lakes(&self, whole: &[u8]) -> Result<Vec<Lake>, FormatError> {
        let section = self.section(SectionKind::Lakes);
        let bytes = self.slice_of(whole, section.range())?;
        let mut out = Vec::with_capacity(usize_of(section.elem_count)?);
        for index in 0..section.elem_count {
            let base = usize_of(index * u64::from(LAKE_RECORD_BYTES))?;
            let root_node = read_u32(bytes, base)?;
            ensure(root_node < self.header.node_count, || FormatError::LakeRootOutOfRange { node: root_node })?; // MUT-27
            let outflow_lake = read_u32(bytes, base + 4)?;
            ensure(outflow_lake == NO_LAKE || u64::from(outflow_lake) < section.elem_count, || FormatError::OutflowLakeOutOfRange { index, target: outflow_lake })?; // MUT-28
            let level_m = read_f64(bytes, base + 8)?;
            ensure(level_m.is_finite(), || FormatError::NonFiniteValue { kind: SectionKind::Lakes, index, bits: level_m.to_bits() })?; // MUT-29
            let kind_code = read_u8(bytes, base + 16)?;
            let kind = match lake_kind_from_code(kind_code) {
                Some(kind) => kind,
                None => return Err(FormatError::UnknownLakeKind { index, found: kind_code }), // MUT-30
            };
            for reserved in 17..24 {
                let byte = read_u8(bytes, base + reserved)?;
                ensure(byte == 0, || FormatError::ReservedBytesNotZero { what: "lake", index })?; // MUT-31
            }
            out.push(Lake { root_node, level_m, kind, outflow_lake });
        }
        Ok(out)
    }

    pub fn read_reaches(&self, whole: &[u8]) -> Result<Vec<Reach>, FormatError> {
        let section = self.section(SectionKind::Reaches);
        let bytes = self.slice_of(whole, section.range())?;
        let mut out = Vec::with_capacity(usize_of(section.elem_count)?);
        for index in 0..section.elem_count {
            let base = usize_of(index * u64::from(REACH_RECORD_BYTES))?;
            let from_node = read_u32(bytes, base)?;
            let to_node = read_u32(bytes, base + 4)?;
            ensure(from_node < self.header.node_count, || FormatError::ReachEndpointOutOfRange { index, node: from_node })?; // MUT-32
            ensure(to_node < self.header.node_count, || FormatError::ReachEndpointOutOfRange { index, node: to_node })?; // MUT-33
            let gradient = read_f64(bytes, base + 8)?;
            ensure(gradient.is_finite(), || FormatError::NonFiniteValue { kind: SectionKind::Reaches, index, bits: gradient.to_bits() })?; // MUT-34
            out.push(Reach { from_node, to_node, gradient });
        }
        Ok(out)
    }

    /// True when `positions` are exactly the node set this file was written from.
    ///
    /// **Restored.** Task 5 dropped this rather than copy `stream.rs`'s FNV, which was the
    /// right call and the wrong end state: without it the header's `position_checksum` was
    /// eight bytes nothing could read, and `SamplingKind` was a claim with nothing behind
    /// it. `stream::position_checksum` is public now, so this is the one hash, called
    /// twice.
    ///
    /// The length is part of the answer: a hash over the wrong number of points is not a
    /// near miss, it is a different graph.
    pub fn positions_match(&self, positions: &[SpherePoint]) -> bool {
        u64_of(positions.len()) == u64::from(self.header.node_count)
            && position_checksum(positions) == self.header.position_checksum
    }

    /// Regenerate the node set the header claims and check it against the checksum.
    ///
    /// **This is what makes `SamplingKind` verifiable rather than merely recorded.** The
    /// header names a sampler, a seed and a node count; every one of the three is an input
    /// to `stream::node_positions`, so a file that lies about any of them fails here. A
    /// file that lies about being `Spiral` when its nodes were supplied fails here too,
    /// which was the last unverifiable field in the header.
    ///
    /// `Supplied` is refused rather than passed: this crate does not know where those
    /// nodes came from, and answering "verified" for a set nobody can reproduce would be
    /// the opposite of failing closed. Such a caller has the positions and calls
    /// `positions_match`.
    ///
    /// **Costs what the node set costs** — 20,000,000 nodes is 480 MB of `SpherePoint` and
    /// the sampler's own runtime — so it is a separate call and never folded into `open`,
    /// which must stay a 288-byte operation.
    pub fn verify_sampling(&self) -> Result<Vec<SpherePoint>, FormatError> {
        match self.header.sampling_kind {
            SamplingKind::Supplied => {
                Err(FormatError::PositionsNotRegenerable { kind: SamplingKind::Supplied }) // MUT-40
            }
            SamplingKind::Spiral => {
                // `node_positions` returns exactly `count` points by construction, so
                // there is no length check here and no unreachable error arm for one --
                // the same call Task 5 made on `MissingSection`. `positions_match` does
                // check a length, because there the points come from a caller.
                let positions = node_positions(self.header.world_seed, self.header.node_count);
                let found = position_checksum(&positions);
                ensure(found == self.header.position_checksum, || FormatError::PositionChecksumMismatch { found, expected: self.header.position_checksum })?; // MUT-41
                Ok(positions)
            }
        }
    }

    /// Everything, plus the cross-section invariant a region read cannot see: **every root
    /// is exactly one of MOUTH or LAKE**. That is section 14.2's claim and the only
    /// classification rule enforced here; the datum test that produced the flags is not
    /// reapplied, so a below-datum root that a future classifier calls a lake reads back as
    /// a lake.
    pub fn read_all(&self, whole: &[u8]) -> Result<DecodedGraph, FormatError> {
        let nodes = self.read_region(whole, 0, self.header.node_count)?;
        let lakes = self.read_lakes(whole)?;
        let reaches = self.read_reaches(whole)?;

        let count = usize_of(u64::from(self.header.node_count))?;
        let mut has_record = vec![false; count];
        for lake in &lakes {
            let root = usize_of(u64::from(lake.root_node))?;
            ensure(nodes.downhill[root] == NO_DOWNHILL, || FormatError::LakeAtNonRoot { node: lake.root_node })?; // MUT-35
            ensure(!has_record[root], || FormatError::DuplicateLakeRoot { node: lake.root_node })?; // MUT-36
            has_record[root] = true;
        }

        for index in 0..count {
            let node = u32::try_from(index).expect("a node index below node_count");
            let is_root = nodes.downhill[index] == NO_DOWNHILL;
            let is_mouth = nodes.flags[index] & flag::MOUTH != 0;
            let flagged_lake = nodes.flags[index] & flag::LAKE_MEMBER != 0;
            if !is_root {
                ensure(!is_mouth, || FormatError::MouthAtNonRoot { node })?; // MUT-37
                continue;
            }
            ensure(is_mouth != has_record[index], || FormatError::RootIsNotExactlyOneClass { node, mouth: is_mouth, lake: has_record[index] })?; // MUT-38
            ensure(flagged_lake == has_record[index], || FormatError::LakeFlagRecordMismatch { node, flagged: flagged_lake, record: has_record[index] })?; // MUT-39
        }

        Ok(DecodedGraph { header: self.header, nodes, lakes, reaches })
    }
}

/// Open and read a whole file in one step.
pub fn read_graph(bytes: &[u8]) -> Result<DecodedGraph, FormatError> {
    GraphReader::open_whole(bytes)?.read_all(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sphere::SpherePoint;
    use crate::stream::{sample_nodes, BuildParams};

    // ---- the fixture -----------------------------------------------------------------
    //
    // Four nodes, chosen so that one 432-byte artifact exercises every structural case:
    // two interior nodes, a below-datum root that `build` classifies as a mouth, an
    // above-datum root that `build` classifies as a lake, a lake record, an empty reach
    // container, and two sections whose payload does not end on the alignment boundary.
    //
    //   node  height_m  area_m2  neighbours  downhill  drainage_area_m2  flags
    //   0        100.0   1000.0  [1]         1                   1000.0  LAND
    //   1         50.0   2000.0  [0, 2]      2                   3000.0  LAND
    //   2        -10.0   3000.0  [1]         NO_DOWNHILL         6000.0  BOUNDARY|MOUTH
    //   3         20.0   4000.0  []          NO_DOWNHILL         4000.0  LAND|LAKE_MEMBER
    //
    // Node 3's drainage area (4,000) exceeds `FIXTURE_POND_MAX_M2` (1,000), so its record
    // is a Lake rather than a Pond -- which is how a build parameter the format
    // deliberately does not store still shows up in the bytes, as `LakeKind`.

    const FIXTURE_SEED: u64 = 20_260_904;
    const FIXTURE_RADIUS_M: f64 = 6_371_000.0;
    const FIXTURE_SEA_LEVEL_M: f64 = 0.0;
    const FIXTURE_POND_MAX_M2: f64 = 1_000.0;
    const FIXTURE_HEIGHTS: [f64; 4] = [100.0, 50.0, -10.0, 20.0];
    const FIXTURE_AREAS: [f64; 4] = [1_000.0, 2_000.0, 3_000.0, 4_000.0];
    const FIXTURE_FILE_LEN: usize = 432;

    /// The datum for the sampled-graph tests. Zero, with the synthetic field below
    /// straddling it, so that one graph carries both mouths and lakes: a round trip over a
    /// graph with an empty lake table would leave the lake section untested.
    const SAMPLED_DATUM_M: f64 = 0.0;

    /// A bumpy synthetic elevation field for the sampled-graph tests. Not a world -- this
    /// module serialises whatever it is handed -- but fractal rather than monotone, because
    /// a monotone field drains everything to one pole and produces no lakes at all.
    fn sampled_heights(positions: &[SpherePoint]) -> Vec<f64> {
        let noise = crate::noise::Noise::new(FIXTURE_SEED, 0x73_74_72_65_61_6d_66_74);
        positions
            .iter()
            .map(|p| {
                let v = p.vector;
                4_000.0 * noise.fbm(v.x, v.y, v.z, 6.0, 5, 0.5, 2.0)
            })
            .collect()
    }

    /// Task 3's FNV-1a over the fixture positions. Pinned so that a change to *that* hash
    /// shows up here as a deliberate decision rather than as a silently different file.
    const FIXTURE_CHECKSUM: u64 = 0x4487_9107_ACB1_03D6;

    fn fixture_positions() -> Vec<SpherePoint> {
        vec![
            SpherePoint::from_latlon(10.0, 20.0),
            SpherePoint::from_latlon(10.5, 20.0),
            SpherePoint::from_latlon(11.0, 20.0),
            SpherePoint::from_latlon(-40.0, 100.0),
        ]
    }

    fn fixture_neighbours() -> Vec<Vec<u32>> {
        vec![vec![1], vec![0, 2], vec![1], vec![]]
    }

    fn fixture_graph() -> StreamGraph {
        let params = BuildParams {
            world_seed: FIXTURE_SEED,
            radius_m: FIXTURE_RADIUS_M,
            sea_level_m: FIXTURE_SEA_LEVEL_M,
            sampling_kind: SamplingKind::Supplied,
            pond_max_drainage_area_m2: FIXTURE_POND_MAX_M2,
        };
        StreamGraph::build(
            &params,
            &fixture_positions(),
            &FIXTURE_HEIGHTS,
            &FIXTURE_AREAS,
            &fixture_neighbours(),
        )
        .expect("the fixture graph builds")
    }

    // ---- an independent encoder ------------------------------------------------------
    //
    // Hand-written from the layout documented at the top of this file, and NOT from
    // `write_graph`. A writer and a reader wrong in the same way round-trip perfectly, so
    // the round-trip tests below are checked against this, and this is checked against
    // literal bytes.

    #[derive(Debug, Clone)]
    struct Parts {
        format_version: u32,
        generator_version: u32,
        world_seed: u64,
        radius_m: f64,
        sea_level_m: f64,
        position_checksum: u64,
        node_count: u32,
        section_count: u32,
        sampling_kind: u8,
        header_reserved: [u8; 7],
        height_m: Vec<f64>,
        area_m2: Vec<f64>,
        downhill: Vec<u32>,
        drainage_area_m2: Vec<f64>,
        flags: Vec<u8>,
        /// `(root_node, outflow_lake, level_m, kind_code)`.
        lakes: Vec<(u32, u32, f64, u8)>,
        /// `(from_node, to_node, gradient)`.
        reaches: Vec<(u32, u32, f64)>,
        /// The section codes, in the order the table lists them.
        order: Vec<u32>,
    }

    impl Parts {
        fn fixture() -> Self {
            Parts {
                format_version: FORMAT_VERSION,
                generator_version: crate::GENERATOR_VERSION,
                world_seed: FIXTURE_SEED,
                radius_m: FIXTURE_RADIUS_M,
                sea_level_m: FIXTURE_SEA_LEVEL_M,
                position_checksum: FIXTURE_CHECKSUM,
                node_count: 4,
                section_count: 7,
                sampling_kind: 0,
                header_reserved: [0; 7],
                height_m: FIXTURE_HEIGHTS.to_vec(),
                area_m2: FIXTURE_AREAS.to_vec(),
                downhill: vec![1, 2, NO_DOWNHILL, NO_DOWNHILL],
                drainage_area_m2: vec![1_000.0, 3_000.0, 6_000.0, 4_000.0],
                flags: vec![
                    flag::LAND,
                    flag::LAND,
                    flag::BOUNDARY | flag::MOUTH,
                    flag::LAND | flag::LAKE_MEMBER,
                ],
                lakes: vec![(3, NO_LAKE, 20.0, 1)],
                reaches: Vec::new(),
                order: vec![1, 2, 3, 4, 5, 6, 7],
            }
        }

        fn payload(&self, code: u32) -> Vec<u8> {
            let mut out = Vec::new();
            match code {
                1 => {
                    for v in &self.height_m {
                        out.extend_from_slice(&v.to_bits().to_le_bytes());
                    }
                }
                2 => {
                    for v in &self.area_m2 {
                        out.extend_from_slice(&v.to_bits().to_le_bytes());
                    }
                }
                3 => {
                    for v in &self.downhill {
                        out.extend_from_slice(&v.to_le_bytes());
                    }
                }
                4 => {
                    for v in &self.drainage_area_m2 {
                        out.extend_from_slice(&v.to_bits().to_le_bytes());
                    }
                }
                5 => out.extend_from_slice(&self.flags),
                6 => {
                    for (root, outflow, level, kind) in &self.lakes {
                        out.extend_from_slice(&root.to_le_bytes());
                        out.extend_from_slice(&outflow.to_le_bytes());
                        out.extend_from_slice(&level.to_bits().to_le_bytes());
                        out.push(*kind);
                        out.extend_from_slice(&[0u8; 7]);
                    }
                }
                7 => {
                    for (from, to, gradient) in &self.reaches {
                        out.extend_from_slice(&from.to_le_bytes());
                        out.extend_from_slice(&to.to_le_bytes());
                        out.extend_from_slice(&gradient.to_bits().to_le_bytes());
                    }
                }
                other => panic!("no such section code {other}"),
            }
            out
        }

        fn width(&self, code: u32) -> u32 {
            match code {
                1 | 2 | 4 => 8,
                3 => 4,
                5 => 1,
                6 => 24,
                7 => 16,
                other => panic!("no such section code {other}"),
            }
        }

        fn elements(&self, code: u32) -> u64 {
            match code {
                6 => u64::try_from(self.lakes.len()).expect("small"),
                7 => u64::try_from(self.reaches.len()).expect("small"),
                _ => u64::from(self.node_count),
            }
        }

        fn assemble(&self) -> Vec<u8> {
            let mut header = Vec::new();
            header.extend_from_slice(&MAGIC);
            header.extend_from_slice(&self.format_version.to_le_bytes());
            header.extend_from_slice(&self.generator_version.to_le_bytes());
            header.extend_from_slice(&self.world_seed.to_le_bytes());
            header.extend_from_slice(&self.radius_m.to_bits().to_le_bytes());
            header.extend_from_slice(&self.sea_level_m.to_bits().to_le_bytes());
            header.extend_from_slice(&self.position_checksum.to_le_bytes());
            header.extend_from_slice(&self.node_count.to_le_bytes());
            header.extend_from_slice(&self.section_count.to_le_bytes());
            header.push(self.sampling_kind);
            header.extend_from_slice(&self.header_reserved);
            assert_eq!(header.len(), HEADER_BYTES);

            let mut offset = u64::try_from(prefix_len()).expect("small");
            let mut table = Vec::new();
            let mut payloads = Vec::new();
            for code in &self.order {
                let bytes = self.payload(*code);
                let byte_len = u64::try_from(bytes.len()).expect("small");
                table.push((*code, self.width(*code), self.elements(*code), offset, byte_len));
                payloads.push((offset, bytes));
                offset += byte_len;
                while offset % SECTION_ALIGN != 0 {
                    offset += 1;
                }
            }

            let mut out = header;
            for (code, width, elements, at, byte_len) in &table {
                out.extend_from_slice(&code.to_le_bytes());
                out.extend_from_slice(&width.to_le_bytes());
                out.extend_from_slice(&elements.to_le_bytes());
                out.extend_from_slice(&at.to_le_bytes());
                out.extend_from_slice(&byte_len.to_le_bytes());
            }
            assert_eq!(out.len(), prefix_len());
            for (at, bytes) in payloads {
                while u64::try_from(out.len()).expect("small") < at {
                    out.push(0);
                }
                out.extend_from_slice(&bytes);
            }
            while out.len() % 8 != 0 {
                out.push(0);
            }
            out
        }
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn entry_at(index: usize) -> usize {
        HEADER_BYTES + index * SECTION_ENTRY_BYTES
    }

    fn err(bytes: &[u8]) -> FormatError {
        read_graph(bytes).expect_err("the reader must refuse this file")
    }

    // ---- the success path, against known bytes ---------------------------------------

    #[test]
    fn the_fixture_checksum_is_pinned() {
        assert_eq!(fixture_graph().header().position_checksum, FIXTURE_CHECKSUM);
    }

    #[test]
    fn the_fixture_graph_is_the_one_documented() {
        let graph = fixture_graph();
        assert_eq!(graph.node_count(), 4);
        assert_eq!(graph.downhill_raw(0), 1);
        assert_eq!(graph.downhill_raw(1), 2);
        assert_eq!(graph.downhill_raw(2), NO_DOWNHILL);
        assert_eq!(graph.downhill_raw(3), NO_DOWNHILL);
        assert_eq!(graph.drainage_area_m2(2), 6_000.0);
        assert_eq!(graph.flags_of(2), flag::BOUNDARY | flag::MOUTH);
        assert_eq!(graph.flags_of(3), flag::LAND | flag::LAKE_MEMBER);
        assert_eq!(graph.lakes().len(), 1);
        assert_eq!(graph.lakes()[0].root_node, 3);
        assert_eq!(graph.lakes()[0].kind, LakeKind::Lake);
        assert_eq!(graph.lakes()[0].outflow_lake, NO_LAKE);
        assert!(graph.reaches().is_empty());
    }

    /// The layout, asserted as literal bytes rather than as whatever the writer produced.
    /// A writer and reader wrong in the same way round-trip perfectly; this is the test a
    /// symmetric error cannot pass.
    #[test]
    fn the_fixture_file_is_these_exact_bytes() {
        let file = write_graph(&fixture_graph());
        assert_eq!(file.len(), FIXTURE_FILE_LEN);

        // Header, field by field, at the documented offsets.
        assert_eq!(&file[0..8], b"WBSTRMG\0");
        assert_eq!(&file[8..12], &[1, 0, 0, 0]); // format_version = 1
        assert_eq!(&file[12..16], &[1, 0, 0, 0]); // generator_version = 1
        assert_eq!(&file[16..24], &[0x28, 0x28, 0x35, 0x01, 0, 0, 0, 0]); // seed 20260904
        assert_eq!(&file[24..32], &[0, 0, 0, 0, 0xae, 0x4d, 0x58, 0x41]); // 6371000.0
        assert_eq!(&file[32..40], &[0, 0, 0, 0, 0, 0, 0, 0]); // sea_level_m = 0.0
        assert_eq!(&file[40..48], &FIXTURE_CHECKSUM.to_le_bytes());
        assert_eq!(&file[48..52], &[4, 0, 0, 0]); // node_count = 4
        assert_eq!(&file[52..56], &[7, 0, 0, 0]); // section_count = 7
        assert_eq!(file[56], 0); // SamplingKind::Supplied
        assert_eq!(&file[57..64], &[0, 0, 0, 0, 0, 0, 0]);

        // The section table, as literal numbers.
        let expected: [(u32, u32, u64, u64, u64); 7] = [
            (1, 8, 4, 288, 32),
            (2, 8, 4, 320, 32),
            (3, 4, 4, 352, 16),
            (4, 8, 4, 368, 32),
            (5, 1, 4, 400, 4),
            (6, 24, 1, 408, 24),
            (7, 16, 0, 432, 0),
        ];
        for (index, (code, width, elements, offset, byte_len)) in expected.iter().enumerate() {
            let base = entry_at(index);
            assert_eq!(u32::from_le_bytes(file[base..base + 4].try_into().unwrap()), *code);
            assert_eq!(u32::from_le_bytes(file[base + 4..base + 8].try_into().unwrap()), *width);
            assert_eq!(
                u64::from_le_bytes(file[base + 8..base + 16].try_into().unwrap()),
                *elements
            );
            assert_eq!(u64::from_le_bytes(file[base + 16..base + 24].try_into().unwrap()), *offset);
            assert_eq!(
                u64::from_le_bytes(file[base + 24..base + 32].try_into().unwrap()),
                *byte_len
            );
        }

        // Payloads. The float bit patterns are hand-derived from IEEE-754, not read back.
        assert_eq!(&file[288..296], &[0, 0, 0, 0, 0, 0, 0x59, 0x40]); // 100.0
        assert_eq!(&file[296..304], &[0, 0, 0, 0, 0, 0, 0x49, 0x40]); // 50.0
        assert_eq!(&file[304..312], &[0, 0, 0, 0, 0, 0, 0x24, 0xc0]); // -10.0
        assert_eq!(&file[312..320], &[0, 0, 0, 0, 0, 0, 0x34, 0x40]); // 20.0
        assert_eq!(&file[320..328], &[0, 0, 0, 0, 0, 0x40, 0x8f, 0x40]); // 1000.0
        assert_eq!(
            &file[352..368],
            &[1, 0, 0, 0, 2, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
        );
        assert_eq!(&file[368..376], &[0, 0, 0, 0, 0, 0x40, 0x8f, 0x40]); // 1000.0
        assert_eq!(&file[384..392], &[0, 0, 0, 0, 0, 0x70, 0xb7, 0x40]); // 6000.0
        assert_eq!(&file[400..404], &[1, 1, 6, 9]); // the flags, as bits
        assert_eq!(&file[404..408], &[0, 0, 0, 0]); // alignment padding
        assert_eq!(
            &file[408..432],
            &[
                3, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0x34, 0x40, 1, 0, 0, 0, 0,
                0, 0, 0
            ]
        );
    }

    #[test]
    fn the_writer_agrees_with_the_independent_hand_encoder() {
        assert_eq!(write_graph(&fixture_graph()), Parts::fixture().assemble());
    }

    #[test]
    fn encoded_len_predicts_the_file_length() {
        let graph = fixture_graph();
        let predicted = encoded_len(
            graph.node_count(),
            u64::try_from(graph.lakes().len()).unwrap(),
            u64::try_from(graph.reaches().len()).unwrap(),
        );
        assert_eq!(predicted, u64::try_from(write_graph(&graph).len()).unwrap());
    }

    #[test]
    fn the_prefix_is_two_hundred_and_eighty_eight_bytes() {
        assert_eq!(prefix_len(), 288);
        assert_eq!(HEADER_BYTES + 7 * SECTION_ENTRY_BYTES, 288);
    }

    #[test]
    fn a_fixture_file_reads_back_field_for_field() {
        let file = write_graph(&fixture_graph());
        let decoded = read_graph(&file).expect("the fixture file reads");
        assert_eq!(decoded.header.generator_version, crate::GENERATOR_VERSION);
        assert_eq!(decoded.header.world_seed, FIXTURE_SEED);
        assert_eq!(decoded.header.radius_m, FIXTURE_RADIUS_M);
        assert_eq!(decoded.header.sea_level_m, FIXTURE_SEA_LEVEL_M);
        assert_eq!(decoded.header.node_count, 4);
        assert_eq!(decoded.header.sampling_kind, SamplingKind::Supplied);
        assert_eq!(decoded.header.position_checksum, FIXTURE_CHECKSUM);
        assert_eq!(decoded.nodes.height_m, FIXTURE_HEIGHTS.to_vec());
        assert_eq!(decoded.nodes.area_m2, FIXTURE_AREAS.to_vec());
        assert_eq!(decoded.nodes.downhill, vec![1, 2, NO_DOWNHILL, NO_DOWNHILL]);
        assert_eq!(decoded.nodes.drainage_area_m2, vec![1_000.0, 3_000.0, 6_000.0, 4_000.0]);
        assert_eq!(decoded.nodes.flags, vec![1, 1, 6, 9]);
        assert_eq!(decoded.lakes.len(), 1);
        assert_eq!(decoded.lakes[0].root_node, 3);
        assert_eq!(decoded.lakes[0].level_m, 20.0);
        assert_eq!(decoded.lakes[0].kind, LakeKind::Lake);
        assert_eq!(decoded.lakes[0].outflow_lake, NO_LAKE);
        assert!(decoded.reaches.is_empty());
    }

    /// A sampled graph, not the fixture: four hand-chosen nodes could hide a bug that only
    /// appears with a real root population.
    #[test]
    fn a_sampled_graph_round_trips_bit_for_bit() {
        let sampling = sample_nodes(FIXTURE_SEED, 4_000, FIXTURE_RADIUS_M).expect("sampled");
        let heights = sampled_heights(&sampling.positions);
        let params = BuildParams {
            world_seed: FIXTURE_SEED,
            radius_m: FIXTURE_RADIUS_M,
            sea_level_m: SAMPLED_DATUM_M,
            sampling_kind: SamplingKind::Spiral,
            pond_max_drainage_area_m2: 5.0e9,
        };
        let graph = StreamGraph::build(
            &params,
            &sampling.positions,
            &heights,
            &sampling.area_m2,
            &sampling.neighbours,
        )
        .expect("the sampled graph builds");
        assert!(!graph.lakes().is_empty(), "the round trip must exercise a lake table");
        assert!(graph.mouth_count() > 0, "the round trip must exercise a mouth");

        let file = write_graph(&graph);
        let decoded = read_graph(&file).expect("a sampled graph reads back");
        assert_eq!(decoded.header, *graph.header());
        assert_eq!(decoded.header.sampling_kind, SamplingKind::Spiral);
        for node in 0..graph.node_count() {
            let index = usize::try_from(node).unwrap();
            assert_eq!(decoded.nodes.height_m[index].to_bits(), graph.height_m(node).to_bits());
            assert_eq!(decoded.nodes.area_m2[index].to_bits(), graph.area_m2(node).to_bits());
            assert_eq!(
                decoded.nodes.drainage_area_m2[index].to_bits(),
                graph.drainage_area_m2(node).to_bits()
            );
            assert_eq!(decoded.nodes.downhill[index], graph.downhill_raw(node));
            assert_eq!(decoded.nodes.flags[index], graph.flags_of(node));
        }
        assert_eq!(decoded.lakes.len(), graph.lakes().len());
        for (read, built) in decoded.lakes.iter().zip(graph.lakes()) {
            assert_eq!(read.root_node, built.root_node);
            assert_eq!(read.level_m.to_bits(), built.level_m.to_bits());
            assert_eq!(read.kind, built.kind);
            assert_eq!(read.outflow_lake, built.outflow_lake);
        }
    }

    // ---- the position checksum, and what it makes verifiable -------------------------

    fn sampled_graph(count: u32) -> StreamGraph {
        let sampling = sample_nodes(FIXTURE_SEED, count, FIXTURE_RADIUS_M).expect("sampled");
        let heights = sampled_heights(&sampling.positions);
        let params = BuildParams {
            world_seed: FIXTURE_SEED,
            radius_m: FIXTURE_RADIUS_M,
            sea_level_m: SAMPLED_DATUM_M,
            sampling_kind: SamplingKind::Spiral,
            pond_max_drainage_area_m2: 5.0e9,
        };
        StreamGraph::build(
            &params,
            &sampling.positions,
            &heights,
            &sampling.area_m2,
            &sampling.neighbours,
        )
        .expect("the sampled graph builds")
    }

    /// Task 5's dropped check, restored: a reader can now tell whether a node set is the
    /// one the file was written from, using `stream.rs`'s own hash rather than a copy.
    #[test]
    fn a_regenerated_node_set_is_checkable_against_the_header() {
        let graph = sampled_graph(4_000);
        let file = write_graph(&graph);
        let reader = GraphReader::open_whole(&file).expect("opens");

        let right = crate::stream::node_positions(FIXTURE_SEED, 4_000);
        assert!(reader.positions_match(&right));

        // A different seed is a different planet, and the checksum says so.
        let wrong_seed = crate::stream::node_positions(FIXTURE_SEED + 1, 4_000);
        assert!(!reader.positions_match(&wrong_seed));

        // A different node count is not a near miss either -- and the length check catches
        // it before the hash does, which is why the length is part of the answer.
        let wrong_count = crate::stream::node_positions(FIXTURE_SEED, 3_999);
        assert!(!reader.positions_match(&wrong_count));

        // One node moved by one bit.
        let mut nudged = right.clone();
        nudged[2_000].vector.x = f64::from_bits(nudged[2_000].vector.x.to_bits() ^ 1);
        assert!(!reader.positions_match(&nudged));
    }

    /// `SamplingKind` was "recorded but unverifiable" after Task 5. It is verifiable now:
    /// the header names the sampler, the seed and the count, and all three are inputs to
    /// the sampler the claim names.
    #[test]
    fn the_sampling_kind_is_verifiable_and_not_merely_recorded() {
        let graph = sampled_graph(4_000);
        let file = write_graph(&graph);
        let reader = GraphReader::open_whole(&file).expect("opens");
        let regenerated = reader.verify_sampling().expect("a Spiral header regenerates");
        assert_eq!(regenerated.len(), 4_000);
        assert!(reader.positions_match(&regenerated));
    }

    /// A file that claims `Spiral` over nodes the spiral did not produce. This is the case
    /// Task 5 reported as reading clean, and it does not any more.
    #[test]
    fn a_file_that_lies_about_its_sampler_is_refused() {
        // The four-node fixture's positions are hand-placed lat/lons, nothing to do with
        // the spiral -- but its header is rewritten to claim the spiral produced them.
        let graph = fixture_graph();
        let mut file = write_graph(&graph);
        assert_eq!(file[OFF_SAMPLING_KIND], 0, "the fixture is built as Supplied");
        file[OFF_SAMPLING_KIND] = 1;
        let reader = GraphReader::open_whole(&file).expect("the lie is structurally legal");
        assert_eq!(reader.header().sampling_kind, SamplingKind::Spiral);
        let err = reader.verify_sampling().expect_err("the spiral does not produce those nodes");
        assert!(matches!(err, FormatError::PositionChecksumMismatch { .. }), "{err:?}");
        // And the mismatch names both sides, so a reader can log what it saw.
        if let FormatError::PositionChecksumMismatch { found, expected } = err {
            assert_eq!(expected, FIXTURE_CHECKSUM);
            assert_ne!(found, FIXTURE_CHECKSUM);
        }
    }

    /// A corrupted checksum over a genuine spiral file: the other direction of the same
    /// guard, so neither side of the comparison is the only one exercised.
    #[test]
    fn a_corrupted_checksum_is_refused_against_a_genuine_node_set() {
        let graph = sampled_graph(4_000);
        let mut file = write_graph(&graph);
        let real = graph.header().position_checksum;
        file[OFF_POSITION_CHECKSUM..OFF_POSITION_CHECKSUM + 8]
            .copy_from_slice(&(real ^ 1).to_le_bytes());
        let reader = GraphReader::open_whole(&file).expect("a wrong checksum is not malformed");
        let err = reader.verify_sampling().expect_err("the regenerated nodes do not match");
        assert_eq!(
            err,
            FormatError::PositionChecksumMismatch { found: real, expected: real ^ 1 }
        );
        assert!(!reader.positions_match(&crate::stream::node_positions(FIXTURE_SEED, 4_000)));
    }

    /// `Supplied` is refused rather than passed. Answering "verified" for a node set this
    /// crate cannot reproduce would be the exact opposite of failing closed.
    #[test]
    fn a_supplied_sampling_declines_to_verify_rather_than_passing() {
        let file = write_graph(&fixture_graph());
        let reader = GraphReader::open_whole(&file).expect("opens");
        assert_eq!(reader.header().sampling_kind, SamplingKind::Supplied);
        assert_eq!(
            reader.verify_sampling(),
            Err(FormatError::PositionsNotRegenerable { kind: SamplingKind::Supplied })
        );
        // The caller who has the positions can still check them.
        assert!(reader.positions_match(&fixture_positions()));
        assert!(!reader.positions_match(&crate::stream::node_positions(FIXTURE_SEED, 4)));
    }

    /// There is exactly one FNV in the crate. A second copy would agree until one of them
    /// was touched, and then every existing worldfile would fail its own checksum -- which
    /// is why Task 5 refused to write one and this task made the original public instead.
    #[test]
    fn the_checksum_is_the_one_in_stream_rs_and_not_a_copy_of_it() {
        let source = include_str!("streamfmt.rs");
        // The offset basis and the prime, in every spelling a copy would plausibly use.
        // **Assembled at run time from halves**, because a needle written whole would be
        // found in the needle list itself and the test would fail on its own text.
        let halves: [(&str, &str); 7] = [
            ("0xcbf2_9ce4", "_8422_2325"),
            ("0xcbf29ce4", "84222325"),
            ("14695981039", "346656037"),
            ("0x0000_0100", "_0000_01b3"),
            ("0x00000100", "000001b3"),
            ("10995116", "28211"),
            ("const ", "fnv_offset"),
        ];
        let lower = source.to_ascii_lowercase();
        for (head, tail) in halves {
            let needle = format!("{head}{tail}");
            assert!(
                !lower.contains(&needle),
                "streamfmt must not carry its own copy of the hash: found {needle}"
            );
        }
        // And the one it does call is stream.rs's, over the same bytes.
        assert_eq!(
            position_checksum(&fixture_positions()),
            fixture_graph().header().position_checksum
        );
    }

    // ---- region-sliceability ---------------------------------------------------------

    /// The whole reason the format looks like this. The reader is given **only the 288-byte
    /// prefix**, works out five byte ranges, and those ranges are the only other bytes
    /// fetched. Nothing between them is parsed, and the lake and reach sections are never
    /// touched at all.
    #[test]
    fn a_region_reads_from_the_prefix_and_its_own_bytes_alone() {
        let sampling = sample_nodes(FIXTURE_SEED, 4_000, FIXTURE_RADIUS_M).expect("sampled");
        let heights = sampled_heights(&sampling.positions);
        let params = BuildParams {
            world_seed: FIXTURE_SEED,
            radius_m: FIXTURE_RADIUS_M,
            sea_level_m: SAMPLED_DATUM_M,
            sampling_kind: SamplingKind::Spiral,
            pond_max_drainage_area_m2: 5.0e9,
        };
        let graph = StreamGraph::build(
            &params,
            &sampling.positions,
            &heights,
            &sampling.area_m2,
            &sampling.neighbours,
        )
        .expect("builds");
        let file = write_graph(&graph);

        // A client that has fetched nothing but the prefix.
        let prefix = &file[..prefix_len()];
        let reader = GraphReader::open(prefix, u64::try_from(file.len()).unwrap())
            .expect("the layout is knowable from the prefix alone");

        let first = 1_000u32;
        let count = 250u32;
        let ranges = reader.region_byte_ranges(first, count).expect("ranges");

        // Exactly 29 bytes per node, and no more.
        let fetched: u64 = ranges.iter().map(|r| r.end - r.start).sum();
        assert_eq!(fetched, u64::from(count) * REGION_BYTES_PER_NODE);
        assert!(fetched * 10 < u64::try_from(file.len()).unwrap());

        // Copy out only those bytes, as a range request would deliver them.
        let grab = |r: &Range<u64>| -> Vec<u8> {
            file[usize::try_from(r.start).unwrap()..usize::try_from(r.end).unwrap()].to_vec()
        };
        let (h, a, d, g, f) = (
            grab(&ranges[0]),
            grab(&ranges[1]),
            grab(&ranges[2]),
            grab(&ranges[3]),
            grab(&ranges[4]),
        );
        let region = reader
            .decode_region(
                first,
                count,
                &RegionBytes {
                    height_m: &h,
                    area_m2: &a,
                    downhill: &d,
                    drainage_area_m2: &g,
                    flags: &f,
                },
            )
            .expect("a region decodes from its own bytes");

        assert_eq!(region.first_node, first);
        assert_eq!(region.len(), usize::try_from(count).unwrap());
        for element in 0..count {
            let node = first + element;
            let index = usize::try_from(element).unwrap();
            assert_eq!(region.height_m[index].to_bits(), graph.height_m(node).to_bits());
            assert_eq!(region.downhill[index], graph.downhill_raw(node));
            assert_eq!(region.flags[index], graph.flags_of(node));
        }
    }

    #[test]
    fn a_regions_downhill_targets_stay_global() {
        let file = write_graph(&fixture_graph());
        let reader = GraphReader::open_whole(&file).expect("opens");
        let region = reader.read_region(&file, 1, 2).expect("a two-node region");
        assert_eq!(region.first_node, 1);
        assert_eq!(region.downhill, vec![2, NO_DOWNHILL]);
        let head = reader.read_region(&file, 0, 1).expect("a one-node region");
        assert_eq!(head.downhill, vec![1], "a target outside the region is still global");
    }

    #[test]
    fn a_region_matches_the_same_slice_of_a_whole_read() {
        let file = write_graph(&fixture_graph());
        let reader = GraphReader::open_whole(&file).expect("opens");
        let whole = reader.read_all(&file).expect("reads");
        let region = reader.read_region(&file, 2, 2).expect("a region");
        assert_eq!(region.height_m, whole.nodes.height_m[2..4].to_vec());
        assert_eq!(region.flags, whole.nodes.flags[2..4].to_vec());
    }

    #[test]
    fn an_empty_region_is_legal() {
        let file = write_graph(&fixture_graph());
        let reader = GraphReader::open_whole(&file).expect("opens");
        let region = reader.read_region(&file, 4, 0).expect("an empty region at the end");
        assert!(region.is_empty());
    }

    /// The table is read, not assumed: a file whose sections are listed in a different
    /// order reads identically. If the reader were indexing the table by position, this
    /// would decode heights as areas.
    #[test]
    fn the_section_table_is_read_rather_than_assumed() {
        let mut parts = Parts::fixture();
        parts.order = vec![7, 6, 5, 4, 3, 2, 1];
        let file = parts.assemble();
        let decoded = read_graph(&file).expect("a reordered table still reads");
        assert_eq!(decoded.nodes.height_m, FIXTURE_HEIGHTS.to_vec());
        assert_eq!(decoded.nodes.area_m2, FIXTURE_AREAS.to_vec());
    }

    // ---- the ruling: the flags travel, the classifier does not ------------------------

    /// **The Dead Sea case.** A root below the datum, flagged as a lake with a lake record
    /// and no MOUTH bit. `StreamGraph::build`'s default classifier would never produce this
    /// file; slice 5 must be able to, without a version bump. The reader must accept it,
    /// which it can only do if it never reapplies the datum test.
    #[test]
    fn a_root_below_the_datum_reads_back_as_a_lake() {
        let mut parts = Parts::fixture();
        // Node 2 is at -10.0 m with the datum at 0.0 m: below sea level, and a lake.
        parts.flags[2] = flag::BOUNDARY | flag::LAKE_MEMBER;
        parts.lakes = vec![(2, NO_LAKE, -12.0, 1), (3, NO_LAKE, 20.0, 1)];
        let decoded = read_graph(&parts.assemble()).expect("a below-datum lake is legal");
        assert_eq!(decoded.nodes.flags[2], flag::BOUNDARY | flag::LAKE_MEMBER);
        assert_eq!(decoded.lakes.len(), 2);
        assert_eq!(decoded.lakes[0].root_node, 2);
        assert_eq!(decoded.lakes[0].level_m, -12.0);
        assert!(decoded.nodes.height_m[2] < decoded.header.sea_level_m);
    }

    /// The mirror case: a root **above** the datum flagged as a mouth. Equally not what the
    /// default classifier produces, equally not the reader's business.
    #[test]
    fn a_root_above_the_datum_reads_back_as_a_mouth() {
        let mut parts = Parts::fixture();
        parts.flags[3] = flag::LAND | flag::MOUTH;
        parts.lakes = Vec::new();
        let decoded = read_graph(&parts.assemble()).expect("an above-datum mouth is legal");
        assert_eq!(decoded.nodes.flags[3], flag::LAND | flag::MOUTH);
        assert!(decoded.lakes.is_empty());
        assert!(decoded.nodes.height_m[3] > decoded.header.sea_level_m);
    }

    /// Slice 5 sets `LAKE_MEMBER` on every node of a filled basin, not only its root. Only
    /// roots carry records, so the flag must not imply one anywhere else.
    #[test]
    fn a_lake_member_that_is_not_a_root_needs_no_record() {
        let mut parts = Parts::fixture();
        parts.flags[1] = flag::LAND | flag::LAKE_MEMBER;
        let decoded = read_graph(&parts.assemble()).expect("a non-root lake member is legal");
        assert_eq!(decoded.nodes.flags[1], flag::LAND | flag::LAKE_MEMBER);
    }

    /// This module's source with the test module removed. The rulings constrain the
    /// format code; the tests below deliberately mention the datum in order to prove the
    /// format does not consult it.
    fn module_source_without_tests() -> String {
        let mut kept = String::new();
        for line in include_str!("streamfmt.rs").lines() {
            if line.trim_end() == "#[cfg(test)]" {
                break;
            }
            kept.push_str(line);
            kept.push('\n');
        }
        kept
    }

    /// Scans this module's own source. The rule is not "we did not write the datum test
    /// today", it is "this file may never contain it".
    ///
    /// **What this is and is not.** It is **line-based**: an offence needs `sea_level`, a
    /// comparison and an elevation word on *one* line. A two-line spelling evades it --
    /// `let datum = header.sea_level_m;` and then `if h > datum { flags |= MOUTH; }` scans
    /// clean. So this is an honest tripwire against the obvious regression, and it does fire
    /// on real planted code (verified by injecting a classifier into the non-test source),
    /// but it is **not** a proof of the property the test name asserts. The property itself
    /// is carried by review and by `validate` never mentioning the datum at all.
    fn datum_offences(text: &str) -> Vec<String> {
        let comparisons = [" < ", " > ", " <= ", " >= ", "<=", ">=", ".min(", ".max("];
        let elevations = ["height", "elevation", "level_m"];
        let mut offences = Vec::new();
        for (number, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            if !line.contains("sea_level") {
                continue;
            }
            let compares = comparisons.iter().any(|c| line.contains(c));
            let elevates = elevations.iter().any(|e| line.contains(e));
            if compares && elevates {
                offences.push(format!("{}: {}", number + 1, trimmed));
            }
        }
        offences
    }

    #[test]
    fn the_format_never_applies_the_datum_classifier() {
        let offences = datum_offences(&module_source_without_tests());
        assert!(
            offences.is_empty(),
            "streamfmt must carry sea_level_m and never test against it:\n{}",
            offences.join("\n")
        );
    }

    #[test]
    fn the_datum_scanner_can_actually_fail() {
        let planted = "fn f() { if height_m[i] > header.sea_level_m { flags |= MOUTH; } }";
        assert!(!datum_offences(planted).is_empty(), "the scanner missed the datum test");
        let innocent = "    sea_level_m: read_f64(prefix, OFF_SEA_LEVEL_M)?,";
        assert!(datum_offences(innocent).is_empty(), "the scanner flagged carrying the datum");
    }

    /// `pond_max_drainage_area_m2` is required-with-no-default and unmeasured. It must not
    /// acquire one by being written into a file with a default somewhere.
    #[test]
    fn the_pond_threshold_is_not_a_field_of_the_format() {
        for (number, line) in module_source_without_tests().lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            assert!(
                !line.contains("pond_max"),
                "line {}: the pond threshold must not be a stored field: {}",
                number + 1,
                trimmed
            );
        }
    }

    // ---- the failure paths -----------------------------------------------------------

    #[test]
    fn refuses_an_unsupported_generator_version() {
        let mut file = write_graph(&fixture_graph());
        put_u32(&mut file, OFF_GENERATOR_VERSION, crate::GENERATOR_VERSION + 1);
        assert_eq!(
            err(&file),
            FormatError::UnsupportedGeneratorVersion {
                found: crate::GENERATOR_VERSION + 1,
                supported: crate::GENERATOR_VERSION,
            }
        );
    }

    /// Not only the next version: a *lower* one, and a wildly higher one, are equally
    /// refused. VERSION-001 recognises no ordering between generator versions.
    #[test]
    fn refuses_every_generator_version_that_is_not_this_one() {
        for found in [0u32, 2, 7, u32::MAX] {
            let mut file = write_graph(&fixture_graph());
            put_u32(&mut file, OFF_GENERATOR_VERSION, found);
            assert_eq!(
                err(&file),
                FormatError::UnsupportedGeneratorVersion {
                    found,
                    supported: crate::GENERATOR_VERSION
                },
                "generator version {found} must be refused"
            );
        }
    }

    #[test]
    fn refuses_an_unsupported_format_version() {
        let mut file = write_graph(&fixture_graph());
        put_u32(&mut file, OFF_FORMAT_VERSION, 2);
        assert_eq!(
            err(&file),
            FormatError::UnsupportedFormatVersion { found: 2, supported: FORMAT_VERSION }
        );
    }

    /// The same sweep the generator version gets, for the same reason: VERSION-001
    /// recognises **no ordering** between format versions either. Testing the guard only at
    /// 2 left `format_version != FORMAT_VERSION` and `format_version <= FORMAT_VERSION` both
    /// alive -- a regressed reader would have accepted a file declaring 0, 7 or `u32::MAX`
    /// and no test would have said so.
    #[test]
    fn refuses_every_format_version_that_is_not_this_one() {
        for found in [0u32, 2, 7, u32::MAX] {
            if found == FORMAT_VERSION {
                continue;
            }
            let mut file = write_graph(&fixture_graph());
            put_u32(&mut file, OFF_FORMAT_VERSION, found);
            assert_eq!(
                err(&file),
                FormatError::UnsupportedFormatVersion { found, supported: FORMAT_VERSION },
                "format version {found} must be refused"
            );
        }
    }

    #[test]
    fn refuses_bad_magic() {
        let mut file = write_graph(&fixture_graph());
        file[0] = b'X';
        match err(&file) {
            FormatError::BadMagic { found } => assert_eq!(found[0], b'X'),
            other => panic!("expected BadMagic, got {other:?}"),
        }
    }

    #[test]
    fn refuses_a_buffer_shorter_than_the_prefix() {
        let file = write_graph(&fixture_graph());
        for length in [0usize, 1, HEADER_BYTES - 1, HEADER_BYTES, prefix_len() - 1] {
            assert_eq!(
                GraphReader::open(&file[..length], u64::try_from(file.len()).unwrap()),
                Err(FormatError::TooShort { need: prefix_len(), got: length }),
                "a {length}-byte prefix must be refused"
            );
        }
        assert!(GraphReader::open(&file[..prefix_len()], u64::try_from(file.len()).unwrap())
            .is_ok());
    }

    #[test]
    fn refuses_a_file_truncated_inside_a_payload() {
        let file = write_graph(&fixture_graph());
        for cut in [prefix_len(), 300, 400, FIXTURE_FILE_LEN - 1] {
            let truncated = &file[..cut];
            assert!(read_graph(truncated).is_err(), "a file cut to {cut} bytes must be refused");
        }
        assert!(read_graph(&file).is_ok());
    }

    #[test]
    fn refuses_a_section_table_pointing_past_the_end() {
        let mut file = write_graph(&fixture_graph());
        put_u64(&mut file, entry_at(0) + ENTRY_OFF_OFFSET, 100_000);
        assert_eq!(
            err(&file),
            FormatError::SectionOutOfBounds {
                kind: SectionKind::HeightM,
                offset: 100_000,
                byte_len: 32,
                file_len: u64::try_from(FIXTURE_FILE_LEN).unwrap(),
            }
        );
    }

    #[test]
    fn refuses_a_section_that_would_end_one_byte_past_the_end() {
        let mut file = write_graph(&fixture_graph());
        // The lake section is 24 bytes at 408, in a 432-byte file. Move it eight bytes on.
        put_u64(&mut file, entry_at(5) + ENTRY_OFF_OFFSET, 416);
        assert_eq!(
            err(&file),
            FormatError::SectionOutOfBounds {
                kind: SectionKind::Lakes,
                offset: 416,
                byte_len: 24,
                file_len: u64::try_from(FIXTURE_FILE_LEN).unwrap(),
            }
        );
    }

    #[test]
    fn refuses_a_section_whose_offset_overflows() {
        let mut file = write_graph(&fixture_graph());
        put_u64(&mut file, entry_at(0) + ENTRY_OFF_OFFSET, u64::MAX - 7);
        assert!(matches!(err(&file), FormatError::SectionOutOfBounds { .. }));
    }

    #[test]
    fn refuses_a_section_that_overlaps_the_prefix() {
        let mut file = write_graph(&fixture_graph());
        put_u64(&mut file, entry_at(0) + ENTRY_OFF_OFFSET, 64);
        assert_eq!(
            err(&file),
            FormatError::SectionInsidePrefix { kind: SectionKind::HeightM, offset: 64 }
        );
    }

    #[test]
    fn refuses_a_per_node_length_that_disagrees_with_the_node_count() {
        let mut file = write_graph(&fixture_graph());
        // Three heights where the header says four nodes -- and a byte_len that agrees with
        // the wrong count, so only the cross-check against node_count can catch it.
        put_u64(&mut file, entry_at(0) + ENTRY_OFF_ELEM_COUNT, 3);
        put_u64(&mut file, entry_at(0) + ENTRY_OFF_BYTE_LEN, 24);
        assert_eq!(
            err(&file),
            FormatError::SectionElementCountWrong {
                kind: SectionKind::HeightM,
                found: 3,
                expected: 4,
            }
        );
    }

    #[test]
    fn refuses_a_byte_length_that_disagrees_with_width_times_count() {
        let mut file = write_graph(&fixture_graph());
        put_u64(&mut file, entry_at(0) + ENTRY_OFF_BYTE_LEN, 40);
        assert_eq!(
            err(&file),
            FormatError::SectionLengthInconsistent {
                kind: SectionKind::HeightM,
                elem_width: 8,
                elem_count: 4,
                byte_len: 40,
            }
        );
    }

    #[test]
    fn refuses_a_length_product_that_overflows() {
        let mut file = write_graph(&fixture_graph());
        put_u64(&mut file, entry_at(5) + ENTRY_OFF_ELEM_COUNT, u64::MAX);
        assert!(matches!(err(&file), FormatError::SectionLengthInconsistent { .. }));
    }

    #[test]
    fn refuses_an_element_width_the_format_does_not_fix() {
        let mut file = write_graph(&fixture_graph());
        put_u32(&mut file, entry_at(0) + ENTRY_OFF_ELEM_WIDTH, 4);
        assert_eq!(
            err(&file),
            FormatError::SectionWidthWrong { kind: SectionKind::HeightM, found: 4, expected: 8 }
        );
    }

    #[test]
    fn refuses_a_misaligned_section() {
        let mut file = write_graph(&fixture_graph());
        put_u64(&mut file, entry_at(0) + ENTRY_OFF_OFFSET, 292);
        assert_eq!(
            err(&file),
            FormatError::SectionMisaligned {
                kind: SectionKind::HeightM,
                offset: 292,
                align: SECTION_ALIGN,
            }
        );
    }

    #[test]
    fn refuses_an_unknown_section_kind() {
        let mut file = write_graph(&fixture_graph());
        put_u32(&mut file, entry_at(0) + ENTRY_OFF_KIND, 99);
        assert_eq!(err(&file), FormatError::UnknownSectionKind { code: 99 });
    }

    #[test]
    fn refuses_a_duplicate_section() {
        let mut file = write_graph(&fixture_graph());
        // Entry 1 (areas) relabelled as heights: seven entries, one kind twice, one absent.
        put_u32(&mut file, entry_at(1) + ENTRY_OFF_KIND, 1);
        assert_eq!(err(&file), FormatError::DuplicateSection { code: 1 });
    }

    #[test]
    fn refuses_a_section_count_that_is_not_seven() {
        for found in [0u32, 6, 8, 1_000] {
            let mut file = write_graph(&fixture_graph());
            put_u32(&mut file, OFF_SECTION_COUNT, found);
            assert_eq!(err(&file), FormatError::SectionCountWrong { found, expected: 7 });
        }
    }

    #[test]
    fn refuses_an_unknown_sampling_kind() {
        let mut file = write_graph(&fixture_graph());
        file[OFF_SAMPLING_KIND] = 2;
        assert_eq!(err(&file), FormatError::UnknownSamplingKind { found: 2 });
    }

    #[test]
    fn refuses_nonzero_header_reserved_bytes() {
        for index in 0..7usize {
            let mut file = write_graph(&fixture_graph());
            file[OFF_HEADER_RESERVED + index] = 1;
            assert_eq!(
                err(&file),
                FormatError::ReservedBytesNotZero {
                    what: "header",
                    index: u64::try_from(index).unwrap()
                }
            );
        }
    }

    #[test]
    fn refuses_a_node_count_above_max_nodes() {
        let mut file = write_graph(&fixture_graph());
        put_u32(&mut file, OFF_NODE_COUNT, u32::MAX);
        assert_eq!(err(&file), FormatError::NodeCountTooLarge { count: u32::MAX, max: MAX_NODES });
    }

    #[test]
    fn refuses_a_nan_height() {
        let mut file = write_graph(&fixture_graph());
        put_u64(&mut file, 288 + 8, f64::NAN.to_bits());
        assert_eq!(
            err(&file),
            FormatError::NonFiniteValue {
                kind: SectionKind::HeightM,
                index: 1,
                bits: f64::NAN.to_bits(),
            }
        );
    }

    #[test]
    fn refuses_an_infinite_drainage_area() {
        let mut file = write_graph(&fixture_graph());
        put_u64(&mut file, 368 + 16, f64::INFINITY.to_bits());
        assert_eq!(
            err(&file),
            FormatError::NonFiniteValue {
                kind: SectionKind::DrainageAreaM2,
                index: 2,
                bits: f64::INFINITY.to_bits(),
            }
        );
    }

    /// Not a named special float but arbitrary rubbish, which is the realistic corruption.
    #[test]
    fn refuses_garbage_where_an_area_should_be() {
        let mut file = write_graph(&fixture_graph());
        put_u64(&mut file, 320, 0x7ff0_0000_dead_beef);
        assert_eq!(
            err(&file),
            FormatError::NonFiniteValue {
                kind: SectionKind::AreaM2,
                index: 0,
                bits: 0x7ff0_0000_dead_beef,
            }
        );
    }

    #[test]
    fn refuses_a_nan_lake_level() {
        let mut file = write_graph(&fixture_graph());
        put_u64(&mut file, 408 + 8, f64::NAN.to_bits());
        assert_eq!(
            err(&file),
            FormatError::NonFiniteValue {
                kind: SectionKind::Lakes,
                index: 0,
                bits: f64::NAN.to_bits(),
            }
        );
    }

    #[test]
    fn refuses_a_nan_radius_in_the_header() {
        let mut file = write_graph(&fixture_graph());
        put_u64(&mut file, OFF_RADIUS_M, f64::NAN.to_bits());
        assert_eq!(
            err(&file),
            FormatError::NonFiniteHeaderField { what: "radius_m", bits: f64::NAN.to_bits() }
        );
    }

    #[test]
    fn refuses_a_nan_datum_in_the_header() {
        let mut file = write_graph(&fixture_graph());
        put_u64(&mut file, OFF_SEA_LEVEL_M, f64::NAN.to_bits());
        assert_eq!(
            err(&file),
            FormatError::NonFiniteHeaderField { what: "sea_level_m", bits: f64::NAN.to_bits() }
        );
    }

    #[test]
    fn refuses_a_downhill_past_the_node_count() {
        let mut file = write_graph(&fixture_graph());
        put_u32(&mut file, 352, 9);
        assert_eq!(err(&file), FormatError::DownhillOutOfRange { node: 0, target: 9 });
    }

    /// The sentinel is *not* out of range, and a reader that checked `target < node_count`
    /// alone would reject every root in every file.
    #[test]
    fn accepts_the_sentinel_as_a_downhill_target() {
        let mut parts = Parts::fixture();
        parts.downhill = vec![NO_DOWNHILL; 4];
        parts.flags = vec![
            flag::LAND | flag::LAKE_MEMBER,
            flag::LAND | flag::LAKE_MEMBER,
            flag::BOUNDARY | flag::MOUTH,
            flag::LAND | flag::LAKE_MEMBER,
        ];
        parts.lakes = vec![(0, NO_LAKE, 100.0, 0), (1, NO_LAKE, 50.0, 0), (3, NO_LAKE, 20.0, 1)];
        let decoded = read_graph(&parts.assemble()).expect("an all-roots graph is legal");
        assert_eq!(decoded.nodes.downhill, vec![NO_DOWNHILL; 4]);
    }

    #[test]
    fn refuses_a_reserved_flag_bit() {
        let mut file = write_graph(&fixture_graph());
        file[400 + 1] = flag::LAND | 0b1000_0000;
        assert_eq!(err(&file), FormatError::ReservedFlagBitSet { node: 1, bits: 0b1000_0000 });
    }

    #[test]
    fn refuses_an_unknown_lake_kind() {
        let mut file = write_graph(&fixture_graph());
        file[408 + 16] = 3;
        assert_eq!(err(&file), FormatError::UnknownLakeKind { index: 0, found: 3 });
    }

    #[test]
    fn refuses_nonzero_lake_reserved_bytes() {
        let mut file = write_graph(&fixture_graph());
        file[408 + 20] = 1;
        assert_eq!(err(&file), FormatError::ReservedBytesNotZero { what: "lake", index: 0 });
    }

    #[test]
    fn refuses_a_lake_root_past_the_node_count() {
        let mut file = write_graph(&fixture_graph());
        put_u32(&mut file, 408, 40);
        assert_eq!(err(&file), FormatError::LakeRootOutOfRange { node: 40 });
    }

    #[test]
    fn refuses_an_outflow_lake_past_the_lake_table() {
        let mut file = write_graph(&fixture_graph());
        put_u32(&mut file, 408 + 4, 5);
        assert_eq!(err(&file), FormatError::OutflowLakeOutOfRange { index: 0, target: 5 });
    }

    #[test]
    fn refuses_a_lake_record_at_a_node_that_is_not_a_root() {
        let mut parts = Parts::fixture();
        parts.lakes = vec![(1, NO_LAKE, 50.0, 1), (3, NO_LAKE, 20.0, 1)];
        assert_eq!(err(&parts.assemble()), FormatError::LakeAtNonRoot { node: 1 });
    }

    #[test]
    fn refuses_a_duplicate_lake_root() {
        let mut parts = Parts::fixture();
        parts.lakes = vec![(3, NO_LAKE, 20.0, 1), (3, NO_LAKE, 21.0, 1)];
        assert_eq!(err(&parts.assemble()), FormatError::DuplicateLakeRoot { node: 3 });
    }

    /// Section 14.2's claim, the "both" arm. This is the invariant that travels with the
    /// flags, and it is checked without reference to the datum.
    #[test]
    fn refuses_a_root_that_is_both_mouth_and_lake() {
        let mut parts = Parts::fixture();
        parts.flags[2] = flag::BOUNDARY | flag::MOUTH | flag::LAKE_MEMBER;
        parts.lakes = vec![(2, NO_LAKE, -10.0, 1), (3, NO_LAKE, 20.0, 1)];
        assert_eq!(
            err(&parts.assemble()),
            FormatError::RootIsNotExactlyOneClass { node: 2, mouth: true, lake: true }
        );
    }

    /// The "neither" arm.
    #[test]
    fn refuses_a_root_that_is_neither_mouth_nor_lake() {
        let mut parts = Parts::fixture();
        parts.flags[3] = flag::LAND;
        parts.lakes = Vec::new();
        assert_eq!(
            err(&parts.assemble()),
            FormatError::RootIsNotExactlyOneClass { node: 3, mouth: false, lake: false }
        );
    }

    #[test]
    fn refuses_a_lake_record_whose_root_lacks_the_flag() {
        let mut parts = Parts::fixture();
        parts.flags[3] = flag::LAND;
        assert_eq!(
            err(&parts.assemble()),
            FormatError::LakeFlagRecordMismatch { node: 3, flagged: false, record: true }
        );
    }

    #[test]
    fn refuses_a_mouth_at_a_node_that_is_not_a_root() {
        let mut parts = Parts::fixture();
        parts.flags[0] = flag::LAND | flag::MOUTH;
        assert_eq!(err(&parts.assemble()), FormatError::MouthAtNonRoot { node: 0 });
    }

    #[test]
    fn refuses_a_reach_endpoint_past_the_node_count() {
        let mut parts = Parts::fixture();
        parts.reaches = vec![(0, 77, 0.01)];
        assert_eq!(
            err(&parts.assemble()),
            FormatError::ReachEndpointOutOfRange { index: 0, node: 77 }
        );
    }

    #[test]
    fn refuses_a_nan_reach_gradient() {
        let mut parts = Parts::fixture();
        parts.reaches = vec![(0, 1, f64::NAN)];
        assert_eq!(
            err(&parts.assemble()),
            FormatError::NonFiniteValue {
                kind: SectionKind::Reaches,
                index: 0,
                bits: f64::NAN.to_bits(),
            }
        );
    }

    #[test]
    fn refuses_a_region_past_the_node_count() {
        let file = write_graph(&fixture_graph());
        let reader = GraphReader::open_whole(&file).expect("opens");
        assert_eq!(
            reader.read_region(&file, 3, 2),
            Err(FormatError::RegionOutOfRange { first_node: 3, count: 2, node_count: 4 })
        );
        assert_eq!(
            reader.node_byte_range(SectionKind::HeightM, 0, 5),
            Err(FormatError::RegionOutOfRange { first_node: 0, count: 5, node_count: 4 })
        );
        assert_eq!(
            reader.node_byte_range(SectionKind::HeightM, u32::MAX, 2),
            Err(FormatError::RegionOutOfRange { first_node: u32::MAX, count: 2, node_count: 4 })
        );
    }

    #[test]
    fn refuses_a_region_of_a_section_that_is_not_per_node() {
        let file = write_graph(&fixture_graph());
        let reader = GraphReader::open_whole(&file).expect("opens");
        assert_eq!(
            reader.node_byte_range(SectionKind::Lakes, 0, 1),
            Err(FormatError::NotAPerNodeSection { kind: SectionKind::Lakes })
        );
    }

    /// A client that fetched the wrong number of bytes must be told, not quietly given a
    /// short region: the missing nodes would read as absent rather than as an error.
    #[test]
    fn refuses_region_bytes_of_the_wrong_length() {
        let file = write_graph(&fixture_graph());
        let reader = GraphReader::open_whole(&file).expect("opens");
        let full = [0u8; 32];
        assert_eq!(
            reader.decode_region(
                0,
                4,
                &RegionBytes {
                    height_m: &[0; 24],
                    area_m2: &full,
                    downhill: &[0; 16],
                    drainage_area_m2: &full,
                    flags: &[0; 4],
                }
            ),
            Err(FormatError::TooShort { need: 32, got: 24 })
        );
        assert_eq!(
            reader.decode_region(
                0,
                4,
                &RegionBytes {
                    height_m: &full,
                    area_m2: &full,
                    downhill: &[0; 20],
                    drainage_area_m2: &full,
                    flags: &[0; 4],
                }
            ),
            Err(FormatError::TooShort { need: 16, got: 20 }),
            "too many bytes is as wrong as too few"
        );
    }

    /// `decode_region` is a public entry point: a client that fetched its own bytes calls
    /// it directly, and then nothing else has checked the node range. Found by mutation --
    /// deleting this guard broke no test until this one existed.
    #[test]
    fn refuses_a_region_out_of_range_even_when_the_bytes_are_the_right_length() {
        let file = write_graph(&fixture_graph());
        let reader = GraphReader::open_whole(&file).expect("opens");
        // Two nodes starting at node 3, in a four-node graph: nodes 3 and 4, and 4 does not
        // exist. Every slice is exactly the right length for a two-node region.
        assert_eq!(
            reader.decode_region(
                3,
                2,
                &RegionBytes {
                    height_m: &[0; 16],
                    area_m2: &[0; 16],
                    downhill: &[0; 8],
                    drainage_area_m2: &[0; 16],
                    flags: &[0; 2],
                }
            ),
            Err(FormatError::RegionOutOfRange { first_node: 3, count: 2, node_count: 4 })
        );
    }

    /// A reader opened against a declared file length, then handed a buffer shorter than
    /// that. This is the shape of a real range-request client whose fetch came back short,
    /// and without the guard the slice would panic rather than refuse. Found by mutation.
    #[test]
    fn refuses_a_buffer_shorter_than_the_length_the_reader_was_opened_with() {
        let file = write_graph(&fixture_graph());
        let reader = GraphReader::open(&file[..prefix_len()], 10_000)
            .expect("a larger declared length is not itself an error");
        assert_eq!(
            reader.read_all(&file[..300]),
            Err(FormatError::TooShort { need: 320, got: 300 })
        );
    }

    /// The upstream endpoint, not only the downstream one. Found by mutation: the two
    /// checks are separate lines and only one of them was covered.
    #[test]
    fn refuses_a_reach_source_past_the_node_count() {
        let mut parts = Parts::fixture();
        parts.reaches = vec![(88, 1, 0.01)];
        assert_eq!(
            err(&parts.assemble()),
            FormatError::ReachEndpointOutOfRange { index: 0, node: 88 }
        );
    }

    #[test]
    fn an_empty_file_is_refused() {
        assert_eq!(err(&[]), FormatError::TooShort { need: prefix_len(), got: 0 });
    }

    #[test]
    fn a_file_of_zeroes_is_refused() {
        let zeroes = vec![0u8; 4_096];
        assert!(matches!(err(&zeroes), FormatError::BadMagic { .. }));
    }

    #[test]
    fn random_looking_bytes_are_refused() {
        // A deterministic pseudo-random buffer: no seed, no RNG, just an arithmetic fill.
        let mut noise = Vec::with_capacity(4_096);
        let mut state: u32 = 0x1234_5678;
        for _ in 0..4_096 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            noise.push(u8::try_from(state >> 24).expect("one byte"));
        }
        assert!(read_graph(&noise).is_err());
    }
}
