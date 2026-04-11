use std::io::{Cursor, Read};

use artisan_core::{
    CanonicalId, CharacterGraph, Entity, EntityType,
    domain::{
        CitationRecord, PublisherRecord, SourceRecord, SubjectRef, VerificationState,
        citation::CitationLocator, entity::CompletenessState,
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

#[derive(Debug, Clone, Default)]
pub struct ParsedCatalog {
    pub publishers: Vec<PublisherRecord>,
    pub sources: Vec<SourceRecord>,
    pub citations: Vec<CitationRecord>,
    pub entity_types: Vec<EntityType>,
    pub entities: Vec<Entity>,
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

        let has_semantic_nodes = doc
            .descendants()
            .any(|n| n.is_element() && matches!(n.tag_name().name(), "thing" | "usesource"));
        if !has_semantic_nodes {
            return Ok(ParsedCatalog::default());
        }

        let entity_type_id = deterministic_id(ENTITY_TYPE_NAMESPACE, "herolab.user.thing");
        let entity_type = EntityType {
            id: entity_type_id,
            key: "herolab.user.thing".to_string(),
            name: "HeroLab Thing".to_string(),
            parent: None,
            fields: Vec::new(),
            relationships: Vec::new(),
            descriptive_fields: IndexMap::new(),
            mechanical_fields: IndexMap::new(),
            external_ids: vec![ExternalId {
                format: FormatId::Herolab,
                namespace: Some("entity_type".to_string()),
                value: "herolab:user:thing".to_string(),
            }],
            provenance: None,
        };

        let mut source_title: Option<String> = None;
        let mut publisher_name: Option<String> = None;
        let mut source_external_ids = vec![ExternalId {
            format: FormatId::Herolab,
            namespace: Some("source".to_string()),
            value: source_name.to_string(),
        }];

        for node in doc.descendants().filter(|n| n.is_element()) {
            let tag = node.tag_name().name();
            if tag != "usesource" {
                continue;
            }

            if source_title.is_none() {
                source_title = node
                    .attribute("name")
                    .or_else(|| node.attribute("sourcename"))
                    .map(ToString::to_string);
            }

            if publisher_name.is_none() {
                publisher_name = node
                    .attribute("parent")
                    .or_else(|| node.attribute("publisher"))
                    .map(ToString::to_string);
            }

            if let Some(id) = node.attribute("id") {
                source_external_ids.push(ExternalId {
                    format: FormatId::Herolab,
                    namespace: Some("source_id".to_string()),
                    value: id.to_string(),
                });
            }
        }

        let source_title = source_title.unwrap_or_else(|| source_name.to_string());
        let source_id = deterministic_id(
            SOURCE_NAMESPACE,
            &format!("herolab:{source_name}:{source_title}"),
        );

        let mut publishers = Vec::new();
        let mut publisher_ids = Vec::new();
        if let Some(name) = publisher_name.filter(|n| !n.trim().is_empty()) {
            let publisher_id =
                deterministic_id(PUBLISHER_NAMESPACE, &format!("herolab:publisher:{name}"));
            publishers.push(PublisherRecord {
                id: publisher_id,
                name: name.clone(),
                external_ids: vec![ExternalId {
                    format: FormatId::Herolab,
                    namespace: Some("publisher".to_string()),
                    value: name,
                }],
            });
            publisher_ids.push(publisher_id);
        }

        let source = SourceRecord {
            id: source_id,
            title: source_title,
            publisher: publishers.first().map(|p| p.name.clone()),
            publisher_ids,
            edition: None,
            license: None,
            game_systems: Vec::new(),
            external_ids: source_external_ids,
        };

        let mut entities = Vec::new();
        let mut citations = Vec::new();

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

            let citation_id = deterministic_id(CITATION_NAMESPACE, &format!("{stable_key}:source"));
            citations.push(CitationRecord {
                id: citation_id,
                subject: SubjectRef::Entity(entity_id),
                source: source_id,
                locators: vec![CitationLocator {
                    kind: "thing".to_string(),
                    value: name.clone(),
                    canonical: true,
                }],
                verification: VerificationState::Unverified,
                external_ids: vec![ExternalId {
                    format: FormatId::Herolab,
                    namespace: Some("citation".to_string()),
                    value: stable_key.clone(),
                }],
            });

            entities.push(Entity {
                id: entity_id,
                entity_type: entity_type_id,
                name,
                attributes,
                effects: Vec::new(),
                prerequisites: Vec::new(),
                rule_hooks: Vec::new(),
                citations: vec![citation_id],
                external_ids: vec![ExternalId {
                    format: FormatId::Herolab,
                    namespace: Some("thing".to_string()),
                    value: stable_key,
                }],
                completeness: CompletenessState::Descriptive,
                provenance: None,
            });
        }

        Ok(ParsedCatalog {
            publishers,
            sources: vec![source],
            citations,
            entity_types: vec![entity_type],
            entities,
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
        Ok(String::new())
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
