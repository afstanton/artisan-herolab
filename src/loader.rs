use std::fmt::Write as _;
use std::io::{Cursor, Read};

use artisan_core::{
    CanonicalId, CharacterGraph, Entity, EntityType,
    domain::{
        CitationRecord, FieldCardinality, FieldDef, FieldType, ParsedCatalog, PublisherRecord,
        SourceRecord, SubjectRef, VerificationState, citation::CitationLocator,
        entity::CompletenessState, rules::Prerequisite, rules::RuleHook, script::ScriptProgram,
        script::ScriptStatement,
    },
    id::{ExternalId, FormatId},
};
use indexmap::IndexMap;
use uuid::Uuid;
use zip::ZipArchive;

use crate::error::HerolabError;

pub struct HerolabLoader;

const ENTITY_TYPE_NAMESPACE: Uuid = Uuid::from_u128(0x593f3c8f8e6f40ed92da84ba66de3690);
const ENTITY_NAMESPACE: Uuid = Uuid::from_u128(0x5b38ce0bf8a34f9f9d1e6cfc81929c3f);
const SOURCE_NAMESPACE: Uuid = Uuid::from_u128(0xb03ea06f4dc14745afddf0dfbc6c9967);
const PUBLISHER_NAMESPACE: Uuid = Uuid::from_u128(0x7ca49103a78e43e9804825aef3efffcb);
const CITATION_NAMESPACE: Uuid = Uuid::from_u128(0x26ce72706933475f96edabedfe060ca8);

#[derive(Debug, Clone)]
struct EntityTypeDescriptor {
    key: String,
    name: String,
    external_value: String,
    fields: Vec<FieldDef>,
}

#[derive(Debug, Clone, Default)]
struct UseSourceRef {
    source_key: String,
    display_name: Option<String>,
    parent_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetKind {
    Image,
    Xml,
    Html,
    Text,
    Binary,
}

#[derive(Debug, Clone)]
pub struct ArchivedAsset {
    pub path: String,
    pub size_bytes: u64,
    pub kind: AssetKind,
}

#[derive(Debug, Clone, Default)]
pub struct PortfolioArchiveManifest {
    pub assets: Vec<ArchivedAsset>,
    pub catalog: ParsedCatalog,
}

impl HerolabLoader {
    pub fn parse_user_entities(_input: &str) -> Result<Vec<Entity>, HerolabError> {
        Ok(Self::parse_user_catalog(_input, "inline.user")?.entities)
    }

    pub fn parse_user_catalog(
        input: &str,
        source_name: &str,
    ) -> Result<ParsedCatalog, HerolabError> {
        let doc = roxmltree::Document::parse(input)
            .map_err(|e| HerolabError::Parse(format!("invalid XML: {e}")))?;

        let has_things = doc
            .descendants()
            .any(|n| n.is_element() && n.tag_name().name() == "thing");
        let has_usesources = doc
            .descendants()
            .any(|n| n.is_element() && n.tag_name().name() == "usesource");
        let has_source_defs = doc
            .descendants()
            .any(|n| n.is_element() && n.tag_name().name() == "source");
        if !has_things && !has_usesources && !has_source_defs {
            return Ok(ParsedCatalog::default());
        }

        let mut publishers = Vec::new();
        let mut publishers_by_name: IndexMap<String, CanonicalId> = IndexMap::new();
        let mut sources_by_key: IndexMap<String, SourceRecord> = IndexMap::new();

        for source_node in doc
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "source")
        {
            let Some(source_key) = source_node
                .attribute("id")
                .filter(|value| !value.trim().is_empty())
            else {
                continue;
            };

            let record = build_source_record(
                source_name,
                source_key,
                source_node.attribute("name"),
                source_node
                    .attribute("publisher")
                    .or_else(|| source_node.attribute("parent")),
                "source_id",
                &mut publishers,
                &mut publishers_by_name,
            );
            sources_by_key
                .entry(source_key.to_string())
                .or_insert(record);
        }

        if !has_things && !has_usesources && !sources_by_key.is_empty() {
            return Ok(ParsedCatalog {
                publishers,
                sources: sources_by_key.into_values().collect(),
                citations: Vec::new(),
                entity_types: Vec::new(),
                entities: Vec::new(),
                ..ParsedCatalog::default()
            });
        }

        let mut entities = Vec::new();
        let mut citations = Vec::new();
        let mut entity_types_by_key: IndexMap<String, EntityType> = IndexMap::new();

        for (index, thing) in doc
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "thing")
            .enumerate()
        {
            let name = thing
                .attribute("name")
                .or_else(|| thing.attribute("id"))
                .unwrap_or("thing")
                .to_string();
            let stable_key = format!("{source_name}:{index}:{name}");
            let entity_id = deterministic_id(ENTITY_NAMESPACE, &stable_key);
            let thing_sources = collect_use_sources(&thing);
            for thing_source in &thing_sources {
                let record = build_source_record(
                    source_name,
                    &thing_source.source_key,
                    thing_source.display_name.as_deref(),
                    thing_source.parent_hint.as_deref(),
                    "source_id",
                    &mut publishers,
                    &mut publishers_by_name,
                );
                sources_by_key
                    .entry(thing_source.source_key.clone())
                    .or_insert(record);
            }
            if thing_sources.is_empty() && sources_by_key.is_empty() {
                let fallback = build_source_record(
                    source_name,
                    source_name,
                    Some(source_name),
                    None,
                    "source",
                    &mut publishers,
                    &mut publishers_by_name,
                );
                sources_by_key
                    .entry(source_name.to_string())
                    .or_insert(fallback);
            }
            let entity_game_system = thing_sources
                .iter()
                .filter_map(|thing_source| sources_by_key.get(&thing_source.source_key))
                .find_map(|source| source.game_systems.first())
                .cloned()
                .or_else(|| {
                    sources_by_key
                        .values()
                        .find_map(|source| source.game_systems.first())
                        .cloned()
                });
            let descriptor = infer_entity_type_descriptor(
                thing.attribute("compset"),
                entity_game_system.as_deref(),
            );
            let entity_type_id = deterministic_id(ENTITY_TYPE_NAMESPACE, &descriptor.key);
            entity_types_by_key
                .entry(descriptor.key.clone())
                .or_insert_with(|| EntityType {
                    id: entity_type_id,
                    key: descriptor.key.clone(),
                    name: descriptor.name.clone(),
                    parent: None,
                    fields: descriptor.fields.clone(),
                    relationships: Vec::new(),
                    descriptive_fields: IndexMap::new(),
                    mechanical_fields: IndexMap::new(),
                    external_ids: vec![ExternalId {
                        format: FormatId::Herolab,
                        namespace: Some("entity_type".to_string()),
                        value: descriptor.external_value.clone(),
                    }],
                    provenance: None,
                });

            let mut attributes = IndexMap::new();
            if let Some(id) = thing.attribute("id") {
                attributes.insert(
                    "thing_id".to_string(),
                    serde_json::Value::String(id.to_string()),
                );
            }
            if let Some(compset) = thing.attribute("compset") {
                attributes.insert(
                    "thing_compset".to_string(),
                    serde_json::Value::String(compset.to_string()),
                );
            }
            if let Some(summary) = thing.attribute("summary") {
                attributes.insert(
                    "summary".to_string(),
                    serde_json::Value::String(summary.to_string()),
                );
            }
            if let Some(description) = thing.attribute("description") {
                attributes.insert(
                    "description".to_string(),
                    serde_json::Value::String(description.to_string()),
                );
            }
            if let Some(uniqueness) = thing.attribute("uniqueness") {
                attributes.insert(
                    "uniqueness".to_string(),
                    serde_json::Value::String(uniqueness.to_string()),
                );
            }
            for fieldval in thing
                .children()
                .filter(|n| n.is_element() && n.tag_name().name() == "fieldval")
            {
                if let (Some(field), Some(value)) =
                    (fieldval.attribute("field"), fieldval.attribute("value"))
                {
                    attributes.insert(
                        format!("field:{field}"),
                        serde_json::Value::String(value.to_string()),
                    );
                    if let Some(canonical_key) = canonical_field_alias(field) {
                        attributes.insert(
                            canonical_key.to_string(),
                            serde_json::Value::String(value.to_string()),
                        );
                    }
                }
            }
            for arrayval in thing
                .children()
                .filter(|n| n.is_element() && n.tag_name().name() == "arrayval")
            {
                let Some(field) = arrayval.attribute("field") else {
                    continue;
                };

                let mut values = Vec::new();
                if let Some(value) = arrayval.attribute("value") {
                    values.push(serde_json::Value::String(value.to_string()));
                }

                for value_node in arrayval.children().filter(|n| n.is_element()) {
                    if let Some(value) = value_node.attribute("value") {
                        values.push(serde_json::Value::String(value.to_string()));
                    } else if let Some(text) =
                        value_node.text().map(str::trim).filter(|t| !t.is_empty())
                    {
                        values.push(serde_json::Value::String(text.to_string()));
                    }
                }

                if !values.is_empty() {
                    attributes.insert(format!("array:{field}"), serde_json::Value::Array(values));
                }
            }
            let tags = collect_tag_records(&thing, "tag");
            if !tags.is_empty() {
                derive_canonical_attributes_from_tags(&mut attributes, &tags);
                attributes.insert("tags".to_string(), serde_json::Value::Array(tags));
            }
            let autotags = collect_tag_records(&thing, "autotag");
            if !autotags.is_empty() {
                derive_canonical_attributes_from_autotags(&mut attributes, &autotags);
                attributes.insert("autotags".to_string(), serde_json::Value::Array(autotags));
            }
            let assignvals = collect_assignval_records(&thing);
            if !assignvals.is_empty() {
                attributes.insert(
                    "assignvals".to_string(),
                    serde_json::Value::Array(assignvals.clone()),
                );
            }
            for assignval in &assignvals {
                let Some(field) = assignval.get("field").and_then(|value| value.as_str()) else {
                    continue;
                };
                if let Some(value) = assignval.get("value") {
                    attributes.insert(format!("assigned:{field}"), value.clone());
                }
            }

            let mut rule_hooks = Vec::new();
            for tag in ["eval", "evalrule", "procedure", "bootstrap"] {
                for script_node in thing
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == tag)
                {
                    if let Some(script) = build_script_program(&script_node) {
                        rule_hooks.push(RuleHook {
                            phase: script_node.attribute("phase").map(ToString::to_string),
                            priority: script_node
                                .attribute("priority")
                                .and_then(|value| value.parse::<i32>().ok()),
                            index: script_node
                                .attribute("index")
                                .and_then(|value| value.parse::<i32>().ok()),
                            script: Some(script),
                        });
                    }
                }
            }

            let mut prerequisites = Vec::new();
            for exprreq in thing
                .children()
                .filter(|n| n.is_element() && n.tag_name().name() == "exprreq")
            {
                prerequisites.push(Prerequisite {
                    kind: "exprreq".to_string(),
                    expression: script_text(&exprreq),
                });
            }
            for prereq in thing
                .children()
                .filter(|n| n.is_element() && n.tag_name().name() == "prereq")
            {
                for validate in prereq
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "validate")
                {
                    prerequisites.push(Prerequisite {
                        kind: "validate".to_string(),
                        expression: script_text(&validate),
                    });
                }
            }

            let mut entity_external_ids = vec![ExternalId {
                format: FormatId::Herolab,
                namespace: Some("thing".to_string()),
                value: stable_key.clone(),
            }];
            if let Some(id) = thing.attribute("id") {
                entity_external_ids.push(ExternalId {
                    format: FormatId::Herolab,
                    namespace: Some("thing_id".to_string()),
                    value: id.to_string(),
                });
            }
            if let Some(compset) = thing.attribute("compset") {
                entity_external_ids.push(ExternalId {
                    format: FormatId::Herolab,
                    namespace: Some("compset".to_string()),
                    value: compset.to_string(),
                });
            }

            let citation_sources: Vec<String> = if thing_sources.is_empty() {
                sources_by_key
                    .keys()
                    .next()
                    .map(|key| vec![key.clone()])
                    .unwrap_or_else(|| vec![source_name.to_string()])
            } else {
                thing_sources
                    .iter()
                    .map(|source| source.source_key.clone())
                    .collect()
            };
            let mut entity_citations = Vec::new();
            for source_key in citation_sources {
                let Some(source) = sources_by_key.get(&source_key) else {
                    continue;
                };
                let citation_id = deterministic_id(
                    CITATION_NAMESPACE,
                    &format!("{stable_key}:source:{source_key}"),
                );
                citations.push(CitationRecord {
                    id: citation_id,
                    subject: SubjectRef::Entity(entity_id),
                    source: source.id,
                    locators: vec![CitationLocator {
                        kind: "thing".to_string(),
                        value: name.clone(),
                        canonical: true,
                    }],
                    verification: VerificationState::Unverified,
                    external_ids: vec![
                        ExternalId {
                            format: FormatId::Herolab,
                            namespace: Some("citation".to_string()),
                            value: stable_key.clone(),
                        },
                        ExternalId {
                            format: FormatId::Herolab,
                            namespace: Some("source_id".to_string()),
                            value: source_key.clone(),
                        },
                    ],
                });
                entity_citations.push(citation_id);
            }

            entities.push(Entity {
                id: entity_id,
                entity_type: entity_type_id,
                name,
                attributes,
                effects: Vec::new(),
                prerequisites,
                rule_hooks,
                citations: entity_citations,
                external_ids: entity_external_ids,
                completeness: CompletenessState::Descriptive,
                provenance: None,
            });
        }

        Ok(ParsedCatalog {
            publishers,
            sources: sources_by_key.into_values().collect(),
            citations,
            entity_types: entity_types_by_key.into_values().collect(),
            entities,
            ..ParsedCatalog::default()
        })
    }

    pub fn parse_portfolio_graph(_input: &[u8]) -> Result<CharacterGraph, HerolabError> {
        Ok(CharacterGraph {
            nodes: Default::default(),
            edges: Vec::new(),
            metadata: artisan_core::domain::GraphMetadata {
                name: None,
                notes: Vec::new(),
            },
            opaque_extensions: Vec::new(),
        })
    }

    pub fn inspect_portfolio_archive(
        input: &[u8],
    ) -> Result<PortfolioArchiveManifest, HerolabError> {
        let cursor = Cursor::new(input);
        let mut archive = ZipArchive::new(cursor)
            .map_err(|e| HerolabError::Parse(format!("invalid ZIP archive: {e}")))?;

        let mut assets = Vec::new();
        let mut catalogs = Vec::new();

        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| HerolabError::Parse(format!("invalid ZIP entry at index {i}: {e}")))?;
            let name = entry.name().to_string();
            let kind = classify_asset_kind(&name);
            assets.push(ArchivedAsset {
                path: name.clone(),
                size_bytes: entry.size(),
                kind: kind.clone(),
            });

            if kind == AssetKind::Xml {
                let mut xml = String::new();
                if entry.read_to_string(&mut xml).is_ok() {
                    if let Ok(catalog) = Self::parse_user_catalog(&xml, &name) {
                        catalogs.push(catalog);
                    }
                }
            }
        }

        let mut merged = ParsedCatalog::default();
        for catalog in catalogs {
            merged.publishers.extend(catalog.publishers);
            merged.sources.extend(catalog.sources);
            merged.citations.extend(catalog.citations);
            merged.entity_types.extend(catalog.entity_types);
            merged.entities.extend(catalog.entities);
        }

        dedupe_catalog(&mut merged);

        Ok(PortfolioArchiveManifest {
            assets,
            catalog: merged,
        })
    }

    pub fn unparse_user_entities(_entities: &[Entity]) -> Result<String, HerolabError> {
        let catalog = ParsedCatalog {
            publishers: Vec::new(),
            sources: Vec::new(),
            citations: Vec::new(),
            entity_types: Vec::new(),
            entities: _entities.to_vec(),
            ..ParsedCatalog::default()
        };
        Self::unparse_user_catalog(&catalog)
    }

    pub fn unparse_user_catalog(catalog: &ParsedCatalog) -> Result<String, HerolabError> {
        let citation_by_id: std::collections::BTreeMap<_, _> = catalog
            .citations
            .iter()
            .map(|citation| (citation.id.0.to_string(), citation))
            .collect();
        let source_by_id: std::collections::BTreeMap<_, _> = catalog
            .sources
            .iter()
            .map(|source| (source.id.0.to_string(), source))
            .collect();

        let mut xml = String::new();
        writeln!(xml, r#"<?xml version="1.0" encoding="UTF-8"?>"#)
            .map_err(|e| HerolabError::Unparse(format!("write xml header: {e}")))?;
        writeln!(xml, r#"<document signature="Hero Lab Data">"#)
            .map_err(|e| HerolabError::Unparse(format!("write root start: {e}")))?;

        for entity in &catalog.entities {
            let thing_id = entity
                .attributes
                .get("thing_id")
                .and_then(|value| value.as_str())
                .or_else(|| {
                    entity
                        .external_ids
                        .iter()
                        .find(|id| id.namespace.as_deref() == Some("thing_id"))
                        .map(|id| id.value.as_str())
                })
                .unwrap_or(entity.name.as_str());
            let compset = entity
                .attributes
                .get("thing_compset")
                .and_then(|value| value.as_str())
                .or_else(|| {
                    entity
                        .external_ids
                        .iter()
                        .find(|id| id.namespace.as_deref() == Some("compset"))
                        .map(|id| id.value.as_str())
                });

            write!(
                xml,
                r#"  <thing id="{}" name="{}""#,
                escape_attr(thing_id),
                escape_attr(&entity.name)
            )
            .map_err(|e| HerolabError::Unparse(format!("write thing open: {e}")))?;

            if let Some(compset) = compset {
                write!(xml, r#" compset="{}""#, escape_attr(compset))
                    .map_err(|e| HerolabError::Unparse(format!("write compset: {e}")))?;
            }
            for attr_key in ["summary", "description", "uniqueness"] {
                if let Some(value) = entity
                    .attributes
                    .get(attr_key)
                    .and_then(|value| value.as_str())
                {
                    write!(xml, r#" {attr_key}="{}""#, escape_attr(value))
                        .map_err(|e| HerolabError::Unparse(format!("write {attr_key}: {e}")))?;
                }
            }
            writeln!(xml, ">")
                .map_err(|e| HerolabError::Unparse(format!("write thing tag close: {e}")))?;

            let mut wrote_sources = false;
            for citation_id in &entity.citations {
                let Some(citation) = citation_by_id.get(&citation_id.0.to_string()) else {
                    continue;
                };
                let Some(source) = source_by_id.get(&citation.source.0.to_string()) else {
                    continue;
                };
                write!(xml, "    <usesource")
                    .map_err(|e| HerolabError::Unparse(format!("write usesource start: {e}")))?;
                if let Some(value) = source
                    .external_ids
                    .iter()
                    .find(|id| {
                        matches!(id.namespace.as_deref(), Some("source_id") | Some("source"))
                    })
                    .map(|id| id.value.as_str())
                {
                    write!(xml, r#" source="{}""#, escape_attr(value)).map_err(|e| {
                        HerolabError::Unparse(format!("write usesource source: {e}"))
                    })?;
                }
                write!(xml, r#" name="{}""#, escape_attr(&source.title))
                    .map_err(|e| HerolabError::Unparse(format!("write usesource name: {e}")))?;
                if let Some(publisher) = source.publisher.as_deref() {
                    write!(xml, r#" parent="{}""#, escape_attr(publisher)).map_err(|e| {
                        HerolabError::Unparse(format!("write usesource publisher: {e}"))
                    })?;
                }
                writeln!(xml, " />")
                    .map_err(|e| HerolabError::Unparse(format!("write usesource end: {e}")))?;
                wrote_sources = true;
            }
            if !wrote_sources {
                if let Some(source) = catalog.sources.first() {
                    write!(
                        xml,
                        r#"    <usesource name="{}""#,
                        escape_attr(&source.title)
                    )
                    .map_err(|e| {
                        HerolabError::Unparse(format!("write fallback usesource start: {e}"))
                    })?;
                    if let Some(value) = source
                        .external_ids
                        .iter()
                        .find(|id| {
                            matches!(id.namespace.as_deref(), Some("source_id") | Some("source"))
                        })
                        .map(|id| id.value.as_str())
                    {
                        write!(xml, r#" source="{}""#, escape_attr(value)).map_err(|e| {
                            HerolabError::Unparse(format!("write fallback usesource source: {e}"))
                        })?;
                    }
                    if let Some(publisher) = source.publisher.as_deref() {
                        write!(xml, r#" parent="{}""#, escape_attr(publisher)).map_err(|e| {
                            HerolabError::Unparse(format!(
                                "write fallback usesource publisher: {e}"
                            ))
                        })?;
                    }
                    writeln!(xml, " />").map_err(|e| {
                        HerolabError::Unparse(format!("write fallback usesource end: {e}"))
                    })?;
                }
            }

            let mut field_keys = Vec::new();
            let mut array_keys = Vec::new();
            for key in entity.attributes.keys() {
                if let Some(field) = key.strip_prefix("field:") {
                    field_keys.push(field.to_string());
                } else if let Some(field) = key.strip_prefix("array:") {
                    array_keys.push(field.to_string());
                }
            }
            field_keys.sort();
            array_keys.sort();

            for field in field_keys {
                if let Some(value) = entity
                    .attributes
                    .get(&format!("field:{field}"))
                    .and_then(|value| value.as_str())
                {
                    writeln!(
                        xml,
                        r#"    <fieldval field="{}" value="{}" />"#,
                        escape_attr(&field),
                        escape_attr(value)
                    )
                    .map_err(|e| HerolabError::Unparse(format!("write fieldval: {e}")))?;
                }
            }

            for field in array_keys {
                if let Some(values) = entity
                    .attributes
                    .get(&format!("array:{field}"))
                    .and_then(|value| value.as_array())
                {
                    for (index, value) in values.iter().enumerate() {
                        if let Some(value) = value.as_str() {
                            writeln!(
                                xml,
                                r#"    <arrayval field="{}" index="{}" value="{}" />"#,
                                escape_attr(&field),
                                index,
                                escape_attr(value)
                            )
                            .map_err(|e| HerolabError::Unparse(format!("write arrayval: {e}")))?;
                        }
                    }
                }
            }

            write_tag_records(&mut xml, entity.attributes.get("tags"), "tag")?;
            write_tag_records(&mut xml, entity.attributes.get("autotags"), "autotag")?;
            write_assignval_records(&mut xml, entity.attributes.get("assignvals"))?;

            for prereq in &entity.prerequisites {
                match prereq.kind.as_str() {
                    "exprreq" => {
                        if let Some(expression) = &prereq.expression {
                            writeln!(xml, "    <exprreq><![CDATA[{expression}]]></exprreq>")
                                .map_err(|e| {
                                    HerolabError::Unparse(format!("write exprreq: {e}"))
                                })?;
                        }
                    }
                    "validate" => {
                        if let Some(expression) = &prereq.expression {
                            writeln!(xml, "    <prereq>")
                                .and_then(|_| {
                                    writeln!(
                                        xml,
                                        "      <validate><![CDATA[{expression}]]></validate>"
                                    )
                                })
                                .and_then(|_| writeln!(xml, "    </prereq>"))
                                .map_err(|e| {
                                    HerolabError::Unparse(format!("write validate prereq: {e}"))
                                })?;
                        }
                    }
                    _ => {}
                }
            }

            for rule_hook in &entity.rule_hooks {
                let Some(script) = &rule_hook.script else {
                    continue;
                };
                let Some(source) = &script.source else {
                    continue;
                };
                write!(xml, "    <eval")
                    .map_err(|e| HerolabError::Unparse(format!("write eval start: {e}")))?;
                if let Some(phase) = &rule_hook.phase {
                    write!(xml, r#" phase="{}""#, escape_attr(phase))
                        .map_err(|e| HerolabError::Unparse(format!("write eval phase: {e}")))?;
                }
                if let Some(priority) = rule_hook.priority {
                    write!(xml, r#" priority="{}""#, priority)
                        .map_err(|e| HerolabError::Unparse(format!("write eval priority: {e}")))?;
                }
                if let Some(index) = rule_hook.index {
                    write!(xml, r#" index="{}""#, index)
                        .map_err(|e| HerolabError::Unparse(format!("write eval index: {e}")))?;
                }
                writeln!(xml, "><![CDATA[{source}]]></eval>")
                    .map_err(|e| HerolabError::Unparse(format!("write eval body: {e}")))?;
            }

            writeln!(xml, "  </thing>")
                .map_err(|e| HerolabError::Unparse(format!("write thing end: {e}")))?;
        }

        writeln!(xml, "</document>")
            .map_err(|e| HerolabError::Unparse(format!("write root end: {e}")))?;
        Ok(xml)
    }
}

fn classify_asset_kind(path: &str) -> AssetKind {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".xml") {
        return AssetKind::Xml;
    }
    if lower.ends_with(".html") || lower.ends_with(".htm") {
        return AssetKind::Html;
    }
    if lower.ends_with(".txt") || lower.ends_with(".rtf") {
        return AssetKind::Text;
    }
    if lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
        || lower.ends_with(".bmp")
        || lower.ends_with(".svg")
    {
        return AssetKind::Image;
    }
    AssetKind::Binary
}

fn dedupe_catalog(catalog: &mut ParsedCatalog) {
    let mut seen_publishers = std::collections::BTreeSet::new();
    catalog
        .publishers
        .retain(|p| seen_publishers.insert(p.id.0.to_string()));

    let mut seen_sources = std::collections::BTreeSet::new();
    catalog
        .sources
        .retain(|s| seen_sources.insert(s.id.0.to_string()));

    let mut seen_citations = std::collections::BTreeSet::new();
    catalog
        .citations
        .retain(|c| seen_citations.insert(c.id.0.to_string()));

    let mut seen_entity_types = std::collections::BTreeSet::new();
    catalog
        .entity_types
        .retain(|e| seen_entity_types.insert(e.id.0.to_string()));

    let mut seen_entities = std::collections::BTreeSet::new();
    catalog
        .entities
        .retain(|e| seen_entities.insert(e.id.0.to_string()));
}

fn deterministic_id(namespace: Uuid, key: &str) -> CanonicalId {
    CanonicalId(Uuid::new_v5(&namespace, key.as_bytes()))
}

fn infer_entity_type_descriptor(
    compset: Option<&str>,
    game_system: Option<&str>,
) -> EntityTypeDescriptor {
    match compset.unwrap_or_default() {
        "RaceSpec" => entity_type_descriptor(
            "herolab.race_spec",
            "HeroLab Race Spec",
            "herolab:compset:RaceSpec",
            game_system,
        ),
        "Race" => entity_type_descriptor(
            "herolab.race",
            "HeroLab Race",
            "herolab:compset:Race",
            game_system,
        ),
        "CustomSpec" => entity_type_descriptor(
            "herolab.custom_spec",
            "HeroLab Custom Spec",
            "herolab:compset:CustomSpec",
            game_system,
        ),
        "Spell" => entity_type_descriptor(
            "herolab.spell",
            "HeroLab Spell",
            "herolab:compset:Spell",
            game_system,
        ),
        "ClSpecial" => entity_type_descriptor(
            "herolab.class_special",
            "HeroLab Class Special",
            "herolab:compset:ClSpecial",
            game_system,
        ),
        "Feat" => entity_type_descriptor(
            "herolab.feat",
            "HeroLab Feat",
            "herolab:compset:Feat",
            game_system,
        ),
        "RaceCustom" => entity_type_descriptor(
            "herolab.race_custom",
            "HeroLab Race Custom",
            "herolab:compset:RaceCustom",
            game_system,
        ),
        "Wondrous" => entity_type_descriptor(
            "herolab.wondrous_item",
            "HeroLab Wondrous Item",
            "herolab:compset:Wondrous",
            game_system,
        ),
        "Gear" => entity_type_descriptor(
            "herolab.gear",
            "HeroLab Gear",
            "herolab:compset:Gear",
            game_system,
        ),
        "Ability" => entity_type_descriptor(
            "herolab.ability",
            "HeroLab Ability",
            "herolab:compset:Ability",
            game_system,
        ),
        "Deity" => entity_type_descriptor(
            "herolab.deity",
            "HeroLab Deity",
            "herolab:compset:Deity",
            game_system,
        ),
        "Weapon" => entity_type_descriptor(
            "herolab.weapon",
            "HeroLab Weapon",
            "herolab:compset:Weapon",
            game_system,
        ),
        "Language" => entity_type_descriptor(
            "herolab.language",
            "HeroLab Language",
            "herolab:compset:Language",
            game_system,
        ),
        "Template" => entity_type_descriptor(
            "herolab.template",
            "HeroLab Template",
            "herolab:compset:Template",
            game_system,
        ),
        "Trait" => entity_type_descriptor(
            "herolab.trait",
            "HeroLab Trait",
            "herolab:compset:Trait",
            game_system,
        ),
        "SubRace" => entity_type_descriptor(
            "herolab.subrace",
            "HeroLab Subrace",
            "herolab:compset:SubRace",
            game_system,
        ),
        "Skill" => entity_type_descriptor(
            "herolab.skill",
            "HeroLab Skill",
            "herolab:compset:Skill",
            game_system,
        ),
        "Class" => entity_type_descriptor(
            "herolab.class",
            "HeroLab Class",
            "herolab:compset:Class",
            game_system,
        ),
        "ClassLevel" => entity_type_descriptor(
            "herolab.class_level",
            "HeroLab Class Level",
            "herolab:compset:ClassLevel",
            game_system,
        ),
        "Background" => entity_type_descriptor(
            "herolab.background",
            "HeroLab Background",
            "herolab:compset:Background",
            game_system,
        ),
        "" => entity_type_descriptor(
            "herolab.user.thing",
            "HeroLab Thing",
            "herolab:user:thing",
            game_system,
        ),
        other => {
            let slug = slugify_compset(other);
            entity_type_descriptor(
                &format!("herolab.{slug}"),
                &format!("HeroLab {other}"),
                &format!("herolab:compset:{other}"),
                game_system,
            )
        }
    }
}

fn entity_type_descriptor(
    key: &str,
    name: &str,
    external_value: &str,
    game_system: Option<&str>,
) -> EntityTypeDescriptor {
    let scoped_key = scope_entity_type_key(key, game_system);
    let scoped_name = scope_entity_type_name(name, game_system);
    let scoped_external_value = scope_external_entity_type_value(external_value, game_system);
    EntityTypeDescriptor {
        key: scoped_key.clone(),
        name: scoped_name,
        external_value: scoped_external_value,
        fields: entity_type_fields_for_key(&scoped_key),
    }
}

fn collect_use_sources(thing: &roxmltree::Node<'_, '_>) -> Vec<UseSourceRef> {
    let mut sources = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for node in thing
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "usesource")
    {
        let source_key = node
            .attribute("source")
            .or_else(|| node.attribute("id"))
            .or_else(|| node.attribute("name"))
            .filter(|value| !value.trim().is_empty());
        let Some(source_key) = source_key else {
            continue;
        };
        if !seen.insert(source_key.to_string()) {
            continue;
        }
        sources.push(UseSourceRef {
            source_key: source_key.to_string(),
            display_name: node
                .attribute("name")
                .or_else(|| node.attribute("sourcename"))
                .filter(|value| !value.trim().is_empty())
                .map(ToString::to_string),
            parent_hint: node
                .attribute("publisher")
                .or_else(|| node.attribute("parent"))
                .filter(|value| !value.trim().is_empty())
                .map(ToString::to_string),
        });
    }
    sources
}

fn build_source_record(
    source_name: &str,
    source_key: &str,
    display_name: Option<&str>,
    parent_hint: Option<&str>,
    external_namespace: &str,
    publishers: &mut Vec<PublisherRecord>,
    publishers_by_name: &mut IndexMap<String, CanonicalId>,
) -> SourceRecord {
    let title = display_name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(source_key)
        .to_string();
    let mut hints = vec![
        source_name.to_string(),
        source_key.to_string(),
        title.clone(),
    ];
    if let Some(parent_hint) = parent_hint.filter(|value| !value.trim().is_empty()) {
        hints.push(parent_hint.to_string());
    }
    let game_systems = infer_game_systems(&hints);

    let publisher_name = parent_hint
        .filter(|value| looks_like_publisher_name(value))
        .map(ToString::to_string);
    let mut publisher_ids = Vec::new();
    if let Some(name) = publisher_name.clone() {
        let publisher_id = if let Some(existing) = publishers_by_name.get(&name) {
            *existing
        } else {
            let id = deterministic_id(PUBLISHER_NAMESPACE, &format!("herolab:publisher:{name}"));
            publishers.push(PublisherRecord {
                id,
                name: name.clone(),
                external_ids: vec![ExternalId {
                    format: FormatId::Herolab,
                    namespace: Some("publisher".to_string()),
                    value: name.clone(),
                }],
            });
            publishers_by_name.insert(name, id);
            id
        };
        publisher_ids.push(publisher_id);
    }

    SourceRecord {
        id: deterministic_id(
            SOURCE_NAMESPACE,
            &format!("herolab:{source_name}:{source_key}:{title}"),
        ),
        title,
        publisher: publisher_name,
        publisher_ids,
        edition: None,
        license: None,
        game_systems,
        external_ids: vec![ExternalId {
            format: FormatId::Herolab,
            namespace: Some(external_namespace.to_string()),
            value: source_key.to_string(),
        }],
    }
}

fn looks_like_publisher_name(value: &str) -> bool {
    value.chars().any(|ch| ch.is_ascii_whitespace()) || value.contains('.')
}

fn scope_entity_type_key(key: &str, game_system: Option<&str>) -> String {
    let Some(game_system) = game_system else {
        return key.to_string();
    };
    let suffix = key.strip_prefix("herolab.").unwrap_or(key);
    format!("herolab.{game_system}.{suffix}")
}

fn scope_entity_type_name(name: &str, game_system: Option<&str>) -> String {
    let Some(game_system) = game_system else {
        return name.to_string();
    };
    format!("{name} ({})", game_system_display_name(game_system))
}

fn scope_external_entity_type_value(external_value: &str, game_system: Option<&str>) -> String {
    let Some(game_system) = game_system else {
        return external_value.to_string();
    };
    if let Some(rest) = external_value.strip_prefix("herolab:") {
        format!("herolab:{game_system}:{rest}")
    } else {
        external_value.to_string()
    }
}

fn slugify_compset(compset: &str) -> String {
    let mut slug = String::new();
    for (index, ch) in compset.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 && !slug.ends_with('_') {
                slug.push('_');
            }
            slug.push(ch.to_ascii_lowercase());
        } else if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.ends_with('_') {
            slug.push('_');
        }
    }

    if slug.is_empty() {
        "thing".to_string()
    } else {
        slug.trim_matches('_').to_string()
    }
}

fn entity_type_fields_for_key(key: &str) -> Vec<FieldDef> {
    let mut fields = common_entity_fields();

    if key.ends_with(".spell") {
        fields.extend([
            text_field("range", "Range"),
            text_field("duration", "Duration"),
            text_field("target", "Target"),
            text_field("saving_throw", "Saving Throw"),
            text_field("components", "Components"),
            text_field("spell_level", "Spell Level"),
            text_field("spell_school", "Spell School"),
            text_field("cast_time", "Cast Time"),
            list_text_field("spell_classes", "Spell Classes"),
        ]);
    } else if key.ends_with(".race")
        || key.ends_with(".subrace")
        || key.ends_with(".race_spec")
        || key.ends_with(".race_custom")
    {
        fields.extend([
            text_field("speed", "Speed"),
            text_field("challenge_rating", "Challenge Rating"),
            text_field("race_type", "Race Type"),
            text_field("race_size", "Race Size"),
            list_text_field("alignments", "Alignments"),
            list_text_field("proficient_skills", "Proficient Skills"),
        ]);
    } else if key.ends_with(".class") || key.ends_with(".class_level") {
        fields.push(text_field("hit_dice", "Hit Dice"));
        fields.push(list_text_field("class_skills", "Class Skills"));
    } else if key.ends_with(".weapon") || key.ends_with(".gear") || key.ends_with(".wondrous_item")
    {
        fields.extend([
            text_field("gear_type", "Gear Type"),
            text_field("item_rarity", "Item Rarity"),
            list_text_field("helper_flags", "Helper Flags"),
            list_text_field("usages", "Usage Tags"),
            list_text_field("charges_per_use", "Charges Per Use"),
            text_field("action_type", "Action Type"),
            text_field("feature_type", "Feature Type"),
            text_field("recharge", "Recharge"),
        ]);
    } else if key.ends_with(".feat")
        || key.ends_with(".class_special")
        || key.ends_with(".ability")
        || key.ends_with(".trait")
    {
        fields.extend([
            text_field("action_type", "Action Type"),
            text_field("feature_type", "Feature Type"),
            text_field("recharge", "Recharge"),
            list_text_field("usages", "Usage Tags"),
            list_text_field("charges_per_use", "Charges Per Use"),
        ]);
    }

    fields
}

fn infer_game_systems(hints: &[String]) -> Vec<String> {
    let mut game_systems = Vec::new();
    for hint in hints {
        let normalized = hint.trim().to_ascii_lowercase();
        let inferred = if normalized.contains("pathfinder 2")
            || normalized.contains("pathfinder2")
            || normalized.contains("pf2")
            || normalized.contains("2e")
        {
            Some("pathfinder2")
        } else if normalized.contains("pathfinder") {
            Some("pathfinder")
        } else if normalized.contains("starfinder 2")
            || normalized.contains("starfinder2")
            || normalized.contains("sf2")
        {
            Some("starfinder2")
        } else if normalized.contains("starfinder") {
            Some("starfinder")
        } else if normalized.contains("hl_dd_5e")
            || normalized.starts_with("5e")
            || normalized.contains(" 5e")
            || normalized.contains("d&d")
            || normalized.contains("dnd")
            || normalized.contains("5th edition")
        {
            Some("dnd5e")
        } else {
            None
        };

        if let Some(system) = inferred {
            if !game_systems
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(system))
            {
                game_systems.push(system.to_string());
            }
        }
    }
    game_systems
}

fn game_system_display_name(game_system: &str) -> &'static str {
    match game_system {
        "dnd5e" => "D&D 5e",
        "pathfinder" => "Pathfinder",
        "pathfinder2" => "Pathfinder 2e",
        "starfinder" => "Starfinder",
        "starfinder2" => "Starfinder 2e",
        _ => "Unknown System",
    }
}

fn common_entity_fields() -> Vec<FieldDef> {
    vec![
        text_field("thing_id", "HeroLab Thing ID"),
        text_field("thing_compset", "HeroLab Compset"),
        text_field("summary", "Summary"),
        text_field("description", "Description"),
        text_field("uniqueness", "Uniqueness"),
        list_text_field("tags", "Tags"),
        list_text_field("autotags", "Auto Tags"),
        list_text_field("assignvals", "Assigned Values"),
    ]
}

fn text_field(key: &str, name: &str) -> FieldDef {
    FieldDef {
        key: key.to_string(),
        name: name.to_string(),
        field_type: FieldType::Text,
        cardinality: FieldCardinality::One,
        required: false,
        description: None,
    }
}

fn list_text_field(key: &str, name: &str) -> FieldDef {
    FieldDef {
        key: key.to_string(),
        name: name.to_string(),
        field_type: FieldType::List(Box::new(FieldType::Text)),
        cardinality: FieldCardinality::Many,
        required: false,
        description: None,
    }
}

fn canonical_field_alias(field: &str) -> Option<&'static str> {
    match field {
        "sRange" => Some("range"),
        "sDuration" => Some("duration"),
        "sTarget" => Some("target"),
        "sSave" => Some("saving_throw"),
        "sCompDesc" => Some("components"),
        "rSpeed" => Some("speed"),
        "rCR" => Some("challenge_rating"),
        "rHitDice" => Some("hit_dice"),
        _ => None,
    }
}

fn derive_canonical_attributes_from_tags(
    attributes: &mut IndexMap<String, serde_json::Value>,
    tags: &[serde_json::Value],
) {
    let mut spell_classes = Vec::new();
    let mut components = Vec::new();
    let mut helper_flags = Vec::new();
    let mut usages = Vec::new();
    let mut charges_per_use = attributes
        .get("charges_per_use")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    for tag in tags {
        let Some(group) = tag.get("group").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(value) = tag.get("tag").and_then(|value| value.as_str()) else {
            continue;
        };

        match group {
            "sClass" => spell_classes.push(serde_json::Value::String(value.to_string())),
            "sComp" => components.push(serde_json::Value::String(value.to_string())),
            "sLevel" => {
                attributes.insert(
                    "spell_level".to_string(),
                    serde_json::Value::String(value.to_string()),
                );
            }
            "sSchool" => {
                attributes.insert(
                    "spell_school".to_string(),
                    serde_json::Value::String(value.to_string()),
                );
            }
            "sCastTime" => {
                attributes.insert(
                    "cast_time".to_string(),
                    serde_json::Value::String(value.to_string()),
                );
            }
            "GearType" | "gType" => {
                attributes.insert(
                    "gear_type".to_string(),
                    serde_json::Value::String(value.to_string()),
                );
            }
            "ItemRarity" => {
                attributes.insert(
                    "item_rarity".to_string(),
                    serde_json::Value::String(value.to_string()),
                );
            }
            "Usage" => usages.push(serde_json::Value::String(value.to_string())),
            "ChargeUse" => charges_per_use.push(serde_json::Value::String(value.to_string())),
            "Helper" => helper_flags.push(serde_json::Value::String(value.to_string())),
            "abAction" => {
                attributes.insert(
                    "action_type".to_string(),
                    serde_json::Value::String(value.to_string()),
                );
            }
            "FeatureTyp" => {
                attributes.insert(
                    "feature_type".to_string(),
                    serde_json::Value::String(value.to_string()),
                );
            }
            "Recharge" => {
                attributes.insert(
                    "recharge".to_string(),
                    serde_json::Value::String(value.to_string()),
                );
            }
            "RaceType" => {
                attributes.insert(
                    "race_type".to_string(),
                    serde_json::Value::String(value.to_string()),
                );
            }
            "RaceSize" => {
                attributes.insert(
                    "race_size".to_string(),
                    serde_json::Value::String(value.to_string()),
                );
            }
            "Alignment" => push_unique_array_value(attributes, "alignments", value),
            "ProfSkill" => push_unique_array_value(attributes, "proficient_skills", value),
            "ClassSkill" => push_unique_array_value(attributes, "class_skills", value),
            _ => {}
        }
    }

    if !spell_classes.is_empty() {
        attributes.insert(
            "spell_classes".to_string(),
            serde_json::Value::Array(spell_classes),
        );
    }
    if !components.is_empty() {
        attributes.insert(
            "component_tags".to_string(),
            serde_json::Value::Array(components.clone()),
        );
        if !attributes.contains_key("components") {
            attributes.insert(
                "components".to_string(),
                serde_json::Value::Array(components),
            );
        }
    }
    if !helper_flags.is_empty() {
        attributes.insert(
            "helper_flags".to_string(),
            serde_json::Value::Array(helper_flags),
        );
    }
    if !usages.is_empty() {
        attributes.insert("usages".to_string(), serde_json::Value::Array(usages));
    }
    if !charges_per_use.is_empty() {
        dedupe_json_values(&mut charges_per_use);
        attributes.insert(
            "charges_per_use".to_string(),
            serde_json::Value::Array(charges_per_use),
        );
    }
}

fn derive_canonical_attributes_from_autotags(
    attributes: &mut IndexMap<String, serde_json::Value>,
    tags: &[serde_json::Value],
) {
    let mut helper_flags = attributes
        .get("helper_flags")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let mut usages = attributes
        .get("usages")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let mut charges_per_use = attributes
        .get("charges_per_use")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    for tag in tags {
        let Some(group) = tag.get("group").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(value) = tag.get("tag").and_then(|value| value.as_str()) else {
            continue;
        };

        match group {
            "Helper" => helper_flags.push(serde_json::Value::String(value.to_string())),
            "Usage" => usages.push(serde_json::Value::String(value.to_string())),
            "ChargeUse" => charges_per_use.push(serde_json::Value::String(value.to_string())),
            "Recharge" => {
                attributes.insert(
                    "recharge".to_string(),
                    serde_json::Value::String(value.to_string()),
                );
            }
            _ => {}
        }
    }

    if !helper_flags.is_empty() {
        dedupe_json_values(&mut helper_flags);
        attributes.insert(
            "helper_flags".to_string(),
            serde_json::Value::Array(helper_flags),
        );
    }
    if !usages.is_empty() {
        dedupe_json_values(&mut usages);
        attributes.insert("usages".to_string(), serde_json::Value::Array(usages));
    }
    if !charges_per_use.is_empty() {
        dedupe_json_values(&mut charges_per_use);
        attributes.insert(
            "charges_per_use".to_string(),
            serde_json::Value::Array(charges_per_use),
        );
    }
}

fn push_unique_array_value(
    attributes: &mut IndexMap<String, serde_json::Value>,
    key: &str,
    value: &str,
) {
    let entry = attributes
        .entry(key.to_string())
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    let Some(items) = entry.as_array_mut() else {
        return;
    };
    let candidate = serde_json::Value::String(value.to_string());
    if !items.iter().any(|item| item == &candidate) {
        items.push(candidate);
    }
}

fn dedupe_json_values(values: &mut Vec<serde_json::Value>) {
    let mut deduped = Vec::with_capacity(values.len());
    for value in values.drain(..) {
        if !deduped.iter().any(|existing| existing == &value) {
            deduped.push(value);
        }
    }
    *values = deduped;
}

fn build_script_program(node: &roxmltree::Node<'_, '_>) -> Option<ScriptProgram> {
    let source = script_text(node)?;
    Some(ScriptProgram {
        source: Some(source.clone()),
        statements: vec![ScriptStatement::Opaque(source)],
    })
}

fn script_text(node: &roxmltree::Node<'_, '_>) -> Option<String> {
    node.text()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
}

fn escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn write_tag_records(
    xml: &mut String,
    value: Option<&serde_json::Value>,
    tag_name: &str,
) -> Result<(), HerolabError> {
    let Some(records) = value.and_then(|value| value.as_array()) else {
        return Ok(());
    };

    for record in records {
        let Some(group) = record.get("group").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(tag) = record.get("tag").and_then(|value| value.as_str()) else {
            continue;
        };

        write!(
            xml,
            r#"    <{tag_name} group="{}" tag="{}""#,
            escape_attr(group),
            escape_attr(tag)
        )
        .map_err(|e| HerolabError::Unparse(format!("write {tag_name} start: {e}")))?;
        if let Some(name) = record.get("name").and_then(|value| value.as_str()) {
            write!(xml, r#" name="{}""#, escape_attr(name))
                .map_err(|e| HerolabError::Unparse(format!("write {tag_name} name: {e}")))?;
        }
        if let Some(abbrev) = record.get("abbrev").and_then(|value| value.as_str()) {
            write!(xml, r#" abbrev="{}""#, escape_attr(abbrev))
                .map_err(|e| HerolabError::Unparse(format!("write {tag_name} abbrev: {e}")))?;
        }
        writeln!(xml, " />")
            .map_err(|e| HerolabError::Unparse(format!("write {tag_name} end: {e}")))?;
    }

    Ok(())
}

fn write_assignval_records(
    xml: &mut String,
    value: Option<&serde_json::Value>,
) -> Result<(), HerolabError> {
    let Some(records) = value.and_then(|value| value.as_array()) else {
        return Ok(());
    };

    for record in records {
        let Some(field) = record.get("field").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(assign_value) = record.get("value").and_then(|value| value.as_str()) else {
            continue;
        };

        write!(
            xml,
            r#"    <assignval field="{}" value="{}""#,
            escape_attr(field),
            escape_attr(assign_value)
        )
        .map_err(|e| HerolabError::Unparse(format!("write assignval start: {e}")))?;
        if let Some(behavior) = record.get("behavior").and_then(|value| value.as_str()) {
            write!(xml, r#" behavior="{}""#, escape_attr(behavior))
                .map_err(|e| HerolabError::Unparse(format!("write assignval behavior: {e}")))?;
        }
        writeln!(xml, " />")
            .map_err(|e| HerolabError::Unparse(format!("write assignval end: {e}")))?;
    }

    Ok(())
}

fn collect_tag_records(node: &roxmltree::Node<'_, '_>, tag_name: &str) -> Vec<serde_json::Value> {
    node.children()
        .filter(|child| child.is_element() && child.tag_name().name() == tag_name)
        .filter_map(|tag| {
            let group = tag.attribute("group")?;
            let value = tag.attribute("tag")?;

            let mut record = serde_json::Map::new();
            record.insert(
                "group".to_string(),
                serde_json::Value::String(group.to_string()),
            );
            record.insert(
                "tag".to_string(),
                serde_json::Value::String(value.to_string()),
            );
            if let Some(name) = tag.attribute("name") {
                record.insert(
                    "name".to_string(),
                    serde_json::Value::String(name.to_string()),
                );
            }
            if let Some(abbrev) = tag.attribute("abbrev") {
                record.insert(
                    "abbrev".to_string(),
                    serde_json::Value::String(abbrev.to_string()),
                );
            }

            Some(serde_json::Value::Object(record))
        })
        .collect()
}

fn collect_assignval_records(node: &roxmltree::Node<'_, '_>) -> Vec<serde_json::Value> {
    node.children()
        .filter(|child| child.is_element() && child.tag_name().name() == "assignval")
        .filter_map(|assignval| {
            let field = assignval.attribute("field")?;
            let value = assignval.attribute("value")?;

            let mut record = serde_json::Map::new();
            record.insert(
                "field".to_string(),
                serde_json::Value::String(field.to_string()),
            );
            record.insert(
                "value".to_string(),
                serde_json::Value::String(value.to_string()),
            );
            if let Some(behavior) = assignval.attribute("behavior") {
                record.insert(
                    "behavior".to_string(),
                    serde_json::Value::String(behavior.to_string()),
                );
            }

            Some(serde_json::Value::Object(record))
        })
        .collect()
}
