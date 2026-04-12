use std::{
    fs, io,
    path::{Path, PathBuf},
};

use artisan_herolab::{HerolabLoader, ParsedCatalog};
use serde_json::{Value, json};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/herolab/input")
}

#[test]
fn semantic_roundtrip_single_fixture_file() {
    let file = fixture_root().join("sample_small_realistic.user");
    assert!(file.exists(), "expected fixture file at {}", file.display());
    assert_semantic_roundtrip_file(&file).expect("single fixture roundtrip should succeed");
}

#[test]
fn semantic_roundtrip_all_fixture_files() {
    let root = fixture_root();
    assert!(
        root.exists(),
        "fixture root does not exist: {}",
        root.display()
    );

    let files = collect_all_fixture_files(&root).expect("collect fixture files");
    assert!(
        !files.is_empty(),
        "no fixture files found under {}",
        root.display()
    );

    for file in &files {
        assert_semantic_roundtrip_file(file).unwrap_or_else(|err| {
            panic!("semantic roundtrip failed for {}: {err}", file.display())
        });
    }
}

fn assert_semantic_roundtrip_file(path: &Path) -> io::Result<()> {
    let text = fs::read_to_string(path)?;
    let source_name = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("fixture.user");
    let first = HerolabLoader::parse_user_catalog(&text, source_name)
        .map_err(|e| io::Error::other(format!("parse first pass: {e}")))?;
    let generated = HerolabLoader::unparse_user_catalog(&first)
        .map_err(|e| io::Error::other(format!("unparse catalog: {e}")))?;
    let second = HerolabLoader::parse_user_catalog(&generated, source_name)
        .map_err(|e| io::Error::other(format!("parse second pass: {e}")))?;

    let before = semantic_snapshot(&first);
    let after = semantic_snapshot(&second);
    assert_eq!(
        before,
        after,
        "semantic roundtrip mismatch: {}",
        path.display()
    );
    Ok(())
}

fn collect_all_fixture_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    collect_all_files_recursive(root, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_all_files_recursive(path: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    if path.is_file() {
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("user"))
        {
            out.push(path.to_path_buf());
        }
        return Ok(());
    }

    if !path.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        collect_all_files_recursive(&entry.path(), out)?;
    }

    Ok(())
}

fn semantic_snapshot(catalog: &ParsedCatalog) -> Value {
    let citation_by_id: std::collections::HashMap<_, _> = catalog
        .citations
        .iter()
        .map(|citation| (citation.id.0.to_string(), citation))
        .collect();
    let source_by_id: std::collections::HashMap<_, _> = catalog
        .sources
        .iter()
        .map(|source| (source.id.0.to_string(), source))
        .collect();

    let mut types: Vec<Value> = catalog
        .entity_types
        .iter()
        .map(|entity_type| {
            let mut field_keys: Vec<_> = entity_type
                .fields
                .iter()
                .map(|field| field.key.clone())
                .collect();
            field_keys.sort();
            json!({
                "key": entity_type.key,
                "name": entity_type.name,
                "fields": field_keys,
            })
        })
        .collect();
    types.sort_by(|a, b| a["key"].as_str().cmp(&b["key"].as_str()));

    let mut publishers: Vec<Value> = catalog
        .publishers
        .iter()
        .map(|publisher| json!({ "name": publisher.name }))
        .collect();
    publishers.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));

    let mut sources: Vec<Value> = catalog
        .sources
        .iter()
        .map(|source| {
            let mut game_systems = source.game_systems.clone();
            game_systems.sort();
            json!({
                "title": source.title,
                "publisher": source.publisher,
                "game_systems": game_systems,
            })
        })
        .collect();
    sources.sort_by(|a, b| a["title"].as_str().cmp(&b["title"].as_str()));

    let mut entities: Vec<Value> = catalog
        .entities
        .iter()
        .map(|entity| {
            let mut attrs = serde_json::Map::new();
            let mut keys: Vec<_> = entity.attributes.keys().cloned().collect();
            keys.sort();
            for key in keys {
                attrs.insert(key.clone(), entity.attributes.get(&key).cloned().unwrap());
            }

            let mut rule_hooks: Vec<Value> = entity
                .rule_hooks
                .iter()
                .map(|hook| {
                    json!({
                        "phase": hook.phase,
                        "priority": hook.priority,
                        "index": hook.index,
                        "script": hook.script.as_ref().and_then(|script| script.source.clone()),
                    })
                })
                .collect();
            rule_hooks.sort_by(|a, b| {
                a["phase"]
                    .as_str()
                    .cmp(&b["phase"].as_str())
                    .then_with(|| a["priority"].as_i64().cmp(&b["priority"].as_i64()))
                    .then_with(|| a["script"].as_str().cmp(&b["script"].as_str()))
            });

            let mut prereqs: Vec<Value> = entity
                .prerequisites
                .iter()
                .map(|prereq| {
                    json!({
                        "kind": prereq.kind,
                        "expression": prereq.expression,
                    })
                })
                .collect();
            prereqs.sort_by(|a, b| {
                a["kind"]
                    .as_str()
                    .cmp(&b["kind"].as_str())
                    .then_with(|| a["expression"].as_str().cmp(&b["expression"].as_str()))
            });

            let mut external_ids: Vec<Value> = entity
                .external_ids
                .iter()
                .map(|id| {
                    json!({
                        "namespace": id.namespace,
                        "value": id.value,
                    })
                })
                .collect();
            external_ids.sort_by(|a, b| {
                a["namespace"]
                    .as_str()
                    .cmp(&b["namespace"].as_str())
                    .then_with(|| a["value"].as_str().cmp(&b["value"].as_str()))
            });

            let mut source_titles: Vec<String> = entity
                .citations
                .iter()
                .filter_map(|citation_id| citation_by_id.get(&citation_id.0.to_string()))
                .filter_map(|citation| source_by_id.get(&citation.source.0.to_string()))
                .map(|source| source.title.clone())
                .collect();
            source_titles.sort();

            json!({
                "name": entity.name,
                "entity_type": entity.entity_type.0.to_string(),
                "attributes": Value::Object(attrs),
                "rule_hooks": rule_hooks,
                "prerequisites": prereqs,
                "external_ids": external_ids,
                "sources": source_titles,
            })
        })
        .collect();
    entities.sort_by(|a, b| {
        a["name"]
            .as_str()
            .cmp(&b["name"].as_str())
            .then_with(|| a["entity_type"].as_str().cmp(&b["entity_type"].as_str()))
    });

    json!({
        "publishers": publishers,
        "sources": sources,
        "entity_types": types,
        "entities": entities,
    })
}
