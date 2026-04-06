# artisan-herolab: HeroLab Parser/Unparser Architecture

## Overview

`artisan-herolab` is a bidirectional parser/unparser system for HeroLab Classic interchange formats. It reads HeroLab XML and archive formats into an Artisan-canonical AST (defined in `artisan-core`), and can write that AST back to HeroLab-equivalent output. This enables cross-format conversion (HeroLab <-> PCGen) via a shared AST.

Initial target formats:

- `.user` (user content definitions)
- `.stock` (portfolio-style ZIP packages with leads/statblocks)
- `.por` (portfolio ZIP packages)

## Goals

1. **Parse static and scripted HeroLab data**
   - Load XML content and embedded Bootstrap script blocks from disk/memory.
2. **Unparse to HeroLab-equivalent output**
   - Write AST back to XML/ZIP formats with functional equivalence (not byte-identical).
3. **Round-trip fidelity for behavior**
   - HeroLab -> AST -> HeroLab should preserve mechanics, prerequisites, and scripted behavior.
4. **Cross-format bridge via shared core**
   - Provide conversion path HeroLab -> `artisan-core` AST -> PCGen and vice versa.
5. **Loss-aware preservation**
   - Preserve unknown XML/script constructs as opaque extension payloads.

## Observed Format Facts (from current repos)

From `externals/shadowchemosh/HL-Pack-Pathfinder` and `externals/SKlore/HL_DD_5e_Colab`:

- `.user` files: root `<document signature="Hero Lab Data">`
- Definition primitives include `<thing>`, `<tag>`, `<fieldval>`, `<arrayval>`, `<bootstrap>`, `<usesource>`
- Rule/script nodes include `<eval>`, `<procedure>`, `<exprreq>`, `<evalrule>`, `<prereq><validate>`
- Script text in CDATA uses procedural Bootstrap idioms (`doneif`, `foreach`, `perform`, field/tag pathing)
- `.stock` files are ZIP archives containing:
  - `index.xml` root `<document signature="Portfolio Index">`
  - `herolab/lead*.xml` root `<document signature="Hero Lab Lead">`
  - `statblocks_text/*` and `statblocks_xml/*`
- Lead XML persists hero state as a large pick graph (`<hero>` -> `<container>` -> `<pick>`, `<reference>`, `<chain>`, `<field>`)

These are treated as baseline invariants for parser design.

## Architecture Layers

```
User Code
    ↓
[artisan-herolab] High-Level API
    ├─ Content Loader (.user)
    ├─ Portfolio Loader (.stock/.por)
    ├─ Script Parser Bridge (Bootstrap)
    └─ Unparser (XML + ZIP writers)
    ↓
[artisan-herolab] Mid-Level Parsing
    ├─ XML Reader (ordered nodes + attributes)
    ├─ HeroLab Node Parsers (thing/eval/procedure/etc.)
    ├─ Lead Graph Parser (hero/container/pick graph)
    ├─ Bootstrap Lexer/Parser
    └─ XML/Archive Serializer
    ↓
[artisan-core] Generic AST + Script IR
    ├─ Entity / Effect / Prerequisite / Source Attribution
    ├─ Script Program / Statements / Expressions
    ├─ CharacterGraph model (for lead/portfolio state)
    └─ OpaqueExtension nodes for unknown constructs
    ↓
[Low-level] File I/O, ZIP, XML, text processing
```

## Core Data Shapes

### Shared (`artisan-core`)

The HeroLab adapter depends on shared structures in `artisan-core`:

- `Entity` (id/name/type/attributes/effects/prereqs/source)
- `RuleHook` (phase/priority/index + script linkage)
- `ScriptProgram` AST (control flow, calls, assignments, expressions)
- `CharacterGraph` (nodes, references, fields, sources, metadata)
- `OpaqueExtension` (unparsed XML/script payload for lossless pass-through)

### HeroLab-specific (`artisan-herolab`)

```rust
pub struct HlDocument {
    signature: String,
    body: HlBody,
    extensions: Vec<OpaqueXmlNode>,
}

pub enum HlBody {
    UserContent(HlUserContent),
    PortfolioIndex(HlPortfolioIndex),
    HeroLead(HlHeroLead),
}

pub struct HlScriptBlock {
    kind: HlScriptKind, // Eval, Procedure, ExprReq, EvalRule, Validate
    phase: Option<String>,
    priority: Option<i32>,
    index: Option<i32>,
    raw_text: String,
    parsed: Option<ScriptProgram>,
}
```

`raw_text` is retained even when `parsed` exists so unparse can preserve unsupported or partially supported script regions.

## Processing Pipelines

### `.user` Loading (HeroLab -> AST)

```
.user XML
    ↓
XmlReader (preserve ordering and attributes)
    ↓
HlUserParser
    ├─ Parse <thing> definitions
    ├─ Parse tags/fields/bootstrap/source metadata
    ├─ Parse requirement and validation nodes
    └─ Extract script blocks (eval/procedure/...)
    ↓
BootstrapParser
    ├─ Parse control flow and calls into ScriptProgram
    └─ Fallback to opaque raw script sections as needed
    ↓
CoreAssembler
    ├─ Build Entity + Effect + Prerequisite
    ├─ Attach RuleHook + ScriptProgram
    └─ Preserve unknowns as OpaqueExtension
    ↓
artisan-core AST
```

### `.stock` / `.por` Loading (HeroLab -> AST)

```
.stock/.por ZIP
    ↓
ArchiveReader
    ├─ Read and index members
    ├─ Parse index.xml (portfolio metadata + roster)
    ├─ Parse herolab/lead*.xml (hero pick graph)
    └─ Capture statblock artifacts (text/xml references)
    ↓
LeadGraphParser
    ├─ hero metadata
    ├─ container/pick/reference/chain graph
    ├─ field values and source activation
    └─ unresolved links captured as opaque refs
    ↓
CharacterGraph (artisan-core)
```

### Unparse (AST -> HeroLab)

```
artisan-core AST / CharacterGraph
    ↓
HlSerializer
    ├─ Entity/rules -> .user XML nodes
    ├─ ScriptProgram -> Bootstrap text
    ├─ CharacterGraph -> lead XML + index entries
    └─ OpaqueExtension passthrough
    ↓
XmlWriter (stable ordering + escaping)
    ↓
ArchiveWriter (ZIP members for .stock/.por)
```

## Script Handling Strategy (Bootstrap)

Bootstrap is procedural and must be handled as first-class syntax.

1. **Structured parse where feasible**
   - Parse major constructs: `if/elseif/else/endif`, `foreach/nexteach`, `doneif`, assignment, procedure-style calls.
2. **Preserve all source text**
   - Keep exact raw CDATA script content alongside parsed AST.
3. **Partial-parse safe mode**
   - If a region fails parse, retain opaque block and continue.
4. **Round-trip preference rules**
   - If a script block is untouched and partially parsed, prefer original raw text on unparse.

## Crate Structure

```
artisan-herolab/
├── src/
│   ├── lib.rs
│   ├── error.rs
│   ├── loader.rs                    # high-level entry points
│   ├── xml/
│   │   ├── reader.rs                # ordered XML parsing
│   │   ├── writer.rs                # ordered XML writing
│   │   └── node.rs                  # lightweight node model
│   ├── user/
│   │   ├── parser.rs                # .user document parser
│   │   ├── thing.rs                 # thing/tag/field mapping
│   │   ├── rules.rs                 # exprreq/evalrule/prereq mapping
│   │   └── serializer.rs
│   ├── portfolio/
│   │   ├── archive.rs               # ZIP reader/writer
│   │   ├── index.rs                 # Portfolio Index parser
│   │   ├── lead.rs                  # Hero Lead parser
│   │   └── serializer.rs
│   ├── script/
│   │   ├── lexer.rs
│   │   ├── parser.rs                # Bootstrap -> ScriptProgram
│   │   ├── ast_bridge.rs            # script AST <-> core IR
│   │   └── unparse.rs
│   ├── bridge/
│   │   ├── to_core.rs               # HeroLab models -> artisan-core
│   │   └── from_core.rs             # artisan-core -> HeroLab models
│   └── preserve/
│       ├── opaque.rs                # unknown node/script preservation
│       └── ordering.rs              # stable order + formatting hints
├── tests/
│   ├── fixtures/                    # sampled .user/.stock/.por data
│   ├── user_roundtrip.rs
│   ├── stock_roundtrip.rs
│   ├── script_parse.rs
│   └── cross_bridge.rs
└── DESIGN.md
```

## Design Decisions

### 1. Separate XML parsing from semantic mapping

HeroLab files contain mixed concerns (definition, execution hooks, saved state). A two-step parse (generic XML node model, then semantic mapping) simplifies resilience and round-trip preservation.

### 2. Script parsing is mandatory but tolerant

Cross-format conversion requires script structure, but complete Bootstrap coverage will be incremental. Therefore parse what we can, preserve what we cannot.

### 3. Portfolio/lead graph is first-class

`lead*.xml` is not a trivial export; it is an indexed pick graph with references/chains and live state. Model this directly instead of flattening too early.

### 4. Stable writer with preservation metadata

Unparse should be deterministic and readable. Preserve node order, unknown attributes/elements, and script raw text to prevent semantic drift.

## Fallback Strategy

Unknown elements and script fragments are preserved in extension records.

```rust
pub struct OpaqueExtension {
    path: String,              // logical location in source model
    raw_xml: Option<String>,
    raw_script: Option<String>,
    attributes: Vec<(String, String)>,
}
```

Rules:

- Never drop unknown content silently.
- Emit parse diagnostics but continue where possible.
- During unparse, re-insert opaque payloads at their recorded locations.

## Testing Strategy

1. **Unit tests per parser module**
   - XML node parsing, script lex/parse, thing/rule mapping, lead graph mapping.
2. **Round-trip tests using real fixtures**
   - `.user -> AST -> .user`
   - `.stock -> AST -> .stock`
   - `.por -> AST -> .por`
3. **Behavioral equivalence checks**
   - Verify rule hooks, prereqs, and effect mappings survive round-trip.
4. **Loss-awareness tests**
   - Introduce unknown nodes/scripts; assert preservation.
5. **Cross-format bridge smoke tests**
   - HeroLab fixture -> core AST -> PCGen output, with expected mappings and known-loss reports.

## Implementation Roadmap

**Phase 1: Scaffolding + `.user` essentials**
- [ ] Error model and diagnostics
- [ ] Ordered XML reader/writer
- [ ] Parse core `.user` nodes (`thing`, tags, fields, sources)
- [ ] Script block extraction + raw preservation
- [ ] Initial Bootstrap parser (control flow + calls + assignments)
- [ ] First `.user` round-trip tests

**Phase 2: Rule/script depth + bridge hooks**
- [ ] `exprreq` / `evalrule` / `prereq` semantic mapping
- [ ] Script AST -> `artisan-core` script IR
- [ ] Partial parse recovery and opaque extension insertion
- [ ] Conversion hooks to/from `artisan-core` entities

**Phase 3: Portfolio support (`.stock`/`.por`)**
- [ ] ZIP archive reader/writer
- [ ] `index.xml` parser + serializer
- [ ] `lead*.xml` hero/pick graph parser + serializer
- [ ] CharacterGraph mapping in `artisan-core`
- [ ] `.stock` and `.por` round-trip suites

**Phase 4: Cross-format confidence**
- [ ] HeroLab <-> core <-> PCGen integration tests
- [ ] Mismatch reporting for unsupported semantics
- [ ] Performance pass on large data packs

## Files to Start With

From current externals analysis:

1. `.user` script-heavy file:
   - `externals/shadowchemosh/HL-Pack-Pathfinder/COM_BasicPack - Procedures.user`
2. `.user` thing-heavy file:
   - `externals/SKlore/HL_DD_5e_Colab/5e_CommunityPack_Feats_PHC.user`
3. `.stock` archive sample (Pathfinder):
   - `externals/shadowchemosh/HL-Pack-Pathfinder/CB - Summon Monster I.stock`
4. `.stock` archive sample (5e):
   - `externals/SKlore/HL_DD_5e_Colab/COM_5ePack_EL_Stock_File.stock`

## Next Steps

1. Add this crate as a workspace member when implementation begins.
2. Replace `src/lib.rs` placeholder with loader API surface.
3. Implement ordered XML parser/writer and first `.user` parser slice.
4. Add fixture-based script extraction tests.
5. Iterate to full script and portfolio coverage.
