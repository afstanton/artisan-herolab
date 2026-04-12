# HeroLab Entity-Type Test Matrix

This matrix defines the first wave of HeroLab definition-layer entity typing
work. It is driven by observed `thing@compset` frequency from
[TOKEN_INVENTORY.txt](/Users/afstanton/code/afstanton/artisan/code/rust/libs/artisan-herolab/TOKEN_INVENTORY.txt),
not by guesswork.

The goal is not to solve every `compset` at once. The goal is to create a
repeatable matrix that turns real HeroLab patterns into:

- focused fixtures
- semantic parser tests
- inferred entity-type mappings
- an explicit backlog of unknowns

## Matrix Columns

- `Priority`: implementation order for fixtures and tests
- `Compset`: HeroLab `thing@compset`
- `Observed`: corpus frequency from the current report
- `Proposed Entity Type`: first-pass canonical key inside `artisan-herolab`
- `Why First`: why this matters early
- `Expected Common Children`: first-pass grammar to test
- `Status`: `seed`, `fixture next`, `in progress`, or `covered`

## Phase 1 Matrix

| Priority | Compset | Observed | Proposed Entity Type | Why First | Expected Common Children | Status |
|---|---:|---:|---|---|---|---|
| 1 | `RaceSpec` | 7483 | `herolab.race_spec` | Highest-frequency specialization layer; likely reveals race trait grammar | `fieldval`, `tag`, `autotag`, `bootstrap`, `eval`, `exprreq`, `usesource` | seed |
| 2 | `Race` | 3642 | `herolab.race` | Core ancestry/race definition layer; already represented in the small fixture | `fieldval`, `tag`, `usesource`, `bootstrap`, `eval`, `comment` | seed |
| 3 | `CustomSpec` | 3577 | `herolab.custom_spec` | Broad customization bucket; likely exposes reusable grammar edges | `fieldval`, `arrayval`, `tag`, `autotag`, `bootstrap`, `evalrule` | seed |
| 4 | `Spell` | 3101 | `herolab.spell` | High-frequency rules content with clear mechanical fields | `fieldval`, `tag`, `usesource`, `exprreq`, `eval`, `procedure` | seed |
| 5 | `ClSpecial` | 3084 | `herolab.class_special` | Important for class-feature modeling and script coverage | `fieldval`, `tag`, `bootstrap`, `eval`, `exprreq`, `prereq/validate` | seed |
| 6 | `Feat` | 1899 | `herolab.feat` | Clear analogue to other systems and likely rich prerequisite coverage | `fieldval`, `tag`, `usesource`, `exprreq`, `prereq/validate`, `eval` | seed |
| 7 | `RaceCustom` | 1485 | `herolab.race_custom` | Likely bridges base race plus user customization | `fieldval`, `arrayval`, `tag`, `bootstrap`, `eval` | seed |
| 8 | `Wondrous` | 1051 | `herolab.wondrous_item` | High-signal item grammar for gear/equipment families | `fieldval`, `tag`, `usesource`, `eval`, `bootstrap` | seed |
| 9 | `Gear` | 704 | `herolab.gear` | Base item/equipment layer | `fieldval`, `tag`, `usesource`, `eval` | seed |
| 10 | `Ability` | 460 | `herolab.ability` | Generic ability grammar likely reused in multiple systems | `fieldval`, `tag`, `bootstrap`, `eval`, `exprreq` | seed |
| 11 | `Deity` | 359 | `herolab.deity` | Distinct domain type with strong descriptive/source fields | `fieldval`, `tag`, `usesource`, `comment` | seed |
| 12 | `Weapon` | 349 | `herolab.weapon` | Core mechanical item family with downstream statblock impact | `fieldval`, `tag`, `usesource`, `eval`, `bootstrap` | seed |
| 13 | `Language` | 310 | `herolab.language` | Simpler type useful for early clean end-to-end mapping | `fieldval`, `tag`, `usesource` | seed |
| 14 | `Template` | 241 | `herolab.template` | Likely modifies other entities and helps later portfolio modeling | `fieldval`, `tag`, `bootstrap`, `evalrule` | seed |
| 15 | `Trait` | 211 | `herolab.trait` | Smaller ability-like grammar that should be easy to fixture | `fieldval`, `tag`, `usesource`, `exprreq` | seed |
| 16 | `SubRace` | 171 | `herolab.subrace` | Important for inheritance/variation patterns in race content | `fieldval`, `tag`, `usesource`, `eval` | seed |
| 17 | `Skill` | 139 | `herolab.skill` | Good target for strongly typed descriptive/mechanical fields | `fieldval`, `tag`, `bootstrap`, `eval` | seed |
| 18 | `Class` | 130 | `herolab.class` | Foundational type for class-level modeling | `fieldval`, `tag`, `usesource`, `bootstrap`, `eval`, `procedure` | seed |
| 19 | `ClassLevel` | 130 | `herolab.class_level` | Important bridge between static definitions and progression logic | `fieldval`, `arrayval`, `bootstrap`, `eval`, `exprreq` | seed |
| 20 | `Background` | 102 | `herolab.background` | Good low-complexity descriptive type from modern systems | `fieldval`, `tag`, `usesource`, `comment` | seed |

## Test Expectations Per Matrix Row

Each matrix row should eventually gain at least one focused fixture and one
semantic parser test that asserts:

1. `thing@compset` maps to the expected inferred entity type key.
2. `thing@id` and `thing@name` are preserved as identity.
3. common child nodes are captured into canonical fields, rule hooks, or
   prerequisites as appropriate.
4. source metadata is preserved when present.
5. unsupported child nodes are preserved in a loss-aware way until modeled.

## First Fixture Batch

The first fixture batch should target these six `compset`s:

- `Race`
- `Feat`
- `Spell`
- `Class`
- `Weapon`
- `Template`

Reason:
- they are high-frequency
- they span descriptive, mechanical, and scripted content
- they should reveal most of the recurring child-node grammar quickly

## Open Questions

- Does `RaceSpec` behave as a subtype, add-on, or independent entity family?
- Is `CustomSpec` too broad for a single inferred type, or should it split by
  child-tag pattern?
- Should some item families (`Wondrous`, `Weapon`, `Armor`, `Gear`) map to a
  shared parent type with subtype-specific keys?
- Which child nodes should become canonical effects versus remain raw rule hooks
  in the first implementation passes?

## Definition Of Done For The Matrix

The matrix is useful when:

- each `seed` row has either a fixture or a concrete extraction plan
- the top six rows have explicit parser tests
- semantic inventory starts reporting multiple inferred entity types instead of a
  single generic HeroLab thing type
