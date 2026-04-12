use artisan_herolab::HerolabLoader;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut per_file = false;
    let mut roots = Vec::new();

    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "--per-file" => per_file = true,
            _ => roots.push(arg.clone()),
        }
    }

    if roots.is_empty() {
        eprintln!("Usage: semantic_inventory [--per-file] <herolab_path1> [herolab_path2] ...");
        eprintln!(
            "Example: semantic_inventory --per-file /path/to/HL-Pack-Pathfinder /path/to/HL_DD_5e_Colab"
        );
        std::process::exit(1);
    }

    let mut state = SemanticReport::default();
    let mut progress = Progress::default();
    eprintln!("semantic_inventory: scanning {} root(s)", roots.len());
    for root_arg in &roots {
        scan_semantic_path(Path::new(root_arg), &mut state, &mut progress)?;
    }

    let mut report = String::new();
    writeln!(report, "=== HeroLab Semantic Inventory Summary ===").unwrap();
    writeln!(report, "Files scanned: {}", state.files_scanned).unwrap();
    writeln!(report, "User documents parsed: {}", state.user_documents).unwrap();
    writeln!(
        report,
        "Portfolio archives parsed: {}",
        state.portfolio_archives
    )
    .unwrap();
    writeln!(
        report,
        "Files with UTF-8 decoding issues fixed: {}",
        state.lossy_decodes
    )
    .unwrap();
    writeln!(
        report,
        "Files with parse failures: {}",
        state.failures.len()
    )
    .unwrap();
    writeln!(report).unwrap();

    writeln!(report, "=== Catalog Coverage Summary ===").unwrap();
    writeln!(report, "Publishers: {}", state.publishers).unwrap();
    writeln!(report, "Sources: {}", state.sources).unwrap();
    writeln!(report, "Citations: {}", state.citations).unwrap();
    writeln!(report, "Entity types: {}", state.entity_types).unwrap();
    writeln!(report, "Entities: {}", state.entities).unwrap();
    writeln!(
        report,
        "Entities with attributes: {}",
        state.entities_with_attributes
    )
    .unwrap();
    writeln!(
        report,
        "Entities with citations: {}",
        state.entities_with_citations
    )
    .unwrap();
    writeln!(
        report,
        "Entities with rule hooks: {}",
        state.entities_with_rule_hooks
    )
    .unwrap();
    writeln!(
        report,
        "Entities with prerequisites: {}",
        state.entities_with_prerequisites
    )
    .unwrap();
    writeln!(
        report,
        "Entities with effects: {}",
        state.entities_with_effects
    )
    .unwrap();
    writeln!(report).unwrap();

    writeln!(report, "=== Portfolio Graph Coverage Summary ===").unwrap();
    writeln!(report, "Graph nodes: {}", state.graph_nodes).unwrap();
    writeln!(report, "Graph edges: {}", state.graph_edges).unwrap();
    writeln!(report, "Graph notes: {}", state.graph_notes).unwrap();
    writeln!(report, "Archive assets: {}", state.archive_assets).unwrap();
    writeln!(report).unwrap();

    write_sorted_counts(
        &mut report,
        "=== Entity Attribute Keys ===",
        &state.attribute_key_counts,
    );
    write_sorted_counts(
        &mut report,
        "=== Rule Hook Phases ===",
        &state.rule_phase_counts,
    );
    write_sorted_counts(
        &mut report,
        "=== Prerequisite Kinds ===",
        &state.prerequisite_kind_counts,
    );
    write_sorted_counts(
        &mut report,
        "=== Archive Asset Kinds ===",
        &state.asset_kind_counts,
    );
    write_sorted_counts(
        &mut report,
        "=== Entity Names By Frequency ===",
        &state.entity_name_counts,
    );

    if per_file {
        writeln!(report, "=== Per-File Results ===").unwrap();
        if state.file_reports.is_empty() {
            writeln!(report, "none").unwrap();
        } else {
            for file in &state.file_reports {
                writeln!(
                    report,
                    "{} | kind={} | entities={} | attrs={} | rules={} | prereqs={} | citations={} | graph_nodes={} | assets={}",
                    file.path.display(),
                    file.kind,
                    file.entities,
                    file.attributes,
                    file.rule_hooks,
                    file.prerequisites,
                    file.citations,
                    file.graph_nodes,
                    file.assets
                )
                .unwrap();
            }
        }
        writeln!(report).unwrap();
    }

    let output_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("SEMANTIC_INVENTORY.txt");
    fs::write(&output_path, report)?;
    eprintln!(
        "semantic_inventory: wrote {} (entities={}, archives={}, failures={})",
        output_path.display(),
        state.entities,
        state.portfolio_archives,
        state.failures.len()
    );

    Ok(())
}

#[derive(Default)]
struct SemanticReport {
    files_scanned: usize,
    user_documents: usize,
    portfolio_archives: usize,
    lossy_decodes: usize,
    publishers: usize,
    sources: usize,
    citations: usize,
    entity_types: usize,
    entities: usize,
    entities_with_attributes: usize,
    entities_with_citations: usize,
    entities_with_rule_hooks: usize,
    entities_with_prerequisites: usize,
    entities_with_effects: usize,
    graph_nodes: usize,
    graph_edges: usize,
    graph_notes: usize,
    archive_assets: usize,
    attribute_key_counts: HashMap<String, usize>,
    rule_phase_counts: HashMap<String, usize>,
    prerequisite_kind_counts: HashMap<String, usize>,
    asset_kind_counts: HashMap<String, usize>,
    entity_name_counts: HashMap<String, usize>,
    file_reports: Vec<FileReport>,
    failures: Vec<String>,
}

struct FileReport {
    path: PathBuf,
    kind: &'static str,
    entities: usize,
    attributes: usize,
    rule_hooks: usize,
    prerequisites: usize,
    citations: usize,
    graph_nodes: usize,
    assets: usize,
}

#[derive(Default)]
struct Progress {
    files_seen: usize,
}

fn scan_semantic_path(
    path: &Path,
    state: &mut SemanticReport,
    progress: &mut Progress,
) -> io::Result<()> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            scan_semantic_path(&entry.path(), state, progress)?;
        }
        return Ok(());
    }

    if !path.is_file() {
        return Ok(());
    }

    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default();

    match extension.as_str() {
        "user" | "xml" | "1st" | "dat" => scan_semantic_xml(path, state, progress)?,
        "por" | "stock" => scan_semantic_archive(path, state, progress)?,
        _ => {}
    }

    Ok(())
}

fn scan_semantic_xml(
    path: &Path,
    state: &mut SemanticReport,
    progress: &mut Progress,
) -> io::Result<()> {
    state.files_scanned += 1;
    note_progress("semantic_inventory", path, state, progress);
    let (text, was_lossy) = read_text_file_lossy(path)?;
    if was_lossy {
        state.lossy_decodes += 1;
    }
    let doc = match roxmltree::Document::parse(&text) {
        Ok(doc) => doc,
        Err(err) => {
            state
                .failures
                .push(format!("{} :: xml parse failed :: {}", path.display(), err));
            return Ok(());
        }
    };

    let signature = doc
        .root_element()
        .attribute("signature")
        .unwrap_or_default();
    if signature != "Hero Lab Data" {
        return Ok(());
    }

    let source_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("input.user");
    match HerolabLoader::parse_user_catalog(&text, source_name) {
        Ok(parsed) => {
            state.user_documents += 1;
            let summary = tally_catalog(&parsed, state);
            state.file_reports.push(FileReport {
                path: path.to_path_buf(),
                kind: "user",
                entities: summary.entities,
                attributes: summary.attributes,
                rule_hooks: summary.rule_hooks,
                prerequisites: summary.prerequisites,
                citations: summary.citations,
                graph_nodes: 0,
                assets: 0,
            });
        }
        Err(err) => state.failures.push(format!(
            "{} :: user parse failed :: {}",
            path.display(),
            err
        )),
    }

    Ok(())
}

fn read_text_file_lossy(path: &Path) -> io::Result<(String, bool)> {
    let bytes = fs::read(path)?;
    Ok(decode_text_lossy(&bytes))
}

fn decode_text_lossy(bytes: &[u8]) -> (String, bool) {
    let content = String::from_utf8_lossy(bytes);
    let was_lossy = content.contains('\u{FFFD}');
    (content.to_string(), was_lossy)
}

fn scan_semantic_archive(
    path: &Path,
    state: &mut SemanticReport,
    progress: &mut Progress,
) -> io::Result<()> {
    state.files_scanned += 1;
    note_progress("semantic_inventory", path, state, progress);
    let bytes = fs::read(path)?;

    match HerolabLoader::inspect_portfolio_archive(&bytes) {
        Ok(manifest) => {
            state.portfolio_archives += 1;
            state.archive_assets += manifest.assets.len();
            for asset in &manifest.assets {
                *state
                    .asset_kind_counts
                    .entry(format!("{:?}", asset.kind))
                    .or_default() += 1;
            }

            let summary = tally_catalog(&manifest.catalog, state);

            let graph_summary = match HerolabLoader::parse_portfolio_graph(&bytes) {
                Ok(graph) => {
                    state.graph_nodes += graph.nodes.len();
                    state.graph_edges += graph.edges.len();
                    state.graph_notes += graph.metadata.notes.len();
                    graph.nodes.len()
                }
                Err(err) => {
                    state.failures.push(format!(
                        "{} :: portfolio graph parse failed :: {}",
                        path.display(),
                        err
                    ));
                    0
                }
            };

            state.file_reports.push(FileReport {
                path: path.to_path_buf(),
                kind: "archive",
                entities: summary.entities,
                attributes: summary.attributes,
                rule_hooks: summary.rule_hooks,
                prerequisites: summary.prerequisites,
                citations: summary.citations,
                graph_nodes: graph_summary,
                assets: manifest.assets.len(),
            });
        }
        Err(err) => state.failures.push(format!(
            "{} :: archive parse failed :: {}",
            path.display(),
            err
        )),
    }

    Ok(())
}

fn note_progress(label: &str, path: &Path, state: &SemanticReport, progress: &mut Progress) {
    progress.files_seen += 1;
    if progress.files_seen == 1 || progress.files_seen.is_multiple_of(100) {
        eprintln!(
            "{label}: processed {} file(s); current={}; entities={}; users={}; archives={}; failures={}",
            progress.files_seen,
            path.display(),
            state.entities,
            state.user_documents,
            state.portfolio_archives,
            state.failures.len()
        );
    }
}

struct CatalogSummary {
    entities: usize,
    attributes: usize,
    rule_hooks: usize,
    prerequisites: usize,
    citations: usize,
}

fn tally_catalog(
    parsed: &artisan_herolab::ParsedCatalog,
    state: &mut SemanticReport,
) -> CatalogSummary {
    state.publishers += parsed.publishers.len();
    state.sources += parsed.sources.len();
    state.citations += parsed.citations.len();
    state.entity_types += parsed.entity_types.len();
    state.entities += parsed.entities.len();

    let mut attributes = 0usize;
    let mut rule_hooks = 0usize;
    let mut prerequisites = 0usize;
    let mut citations = 0usize;

    for entity in &parsed.entities {
        if !entity.attributes.is_empty() {
            state.entities_with_attributes += 1;
        }
        if !entity.citations.is_empty() {
            state.entities_with_citations += 1;
        }
        if !entity.rule_hooks.is_empty() {
            state.entities_with_rule_hooks += 1;
        }
        if !entity.prerequisites.is_empty() {
            state.entities_with_prerequisites += 1;
        }
        if !entity.effects.is_empty() {
            state.entities_with_effects += 1;
        }

        attributes += entity.attributes.len();
        rule_hooks += entity.rule_hooks.len();
        prerequisites += entity.prerequisites.len();
        citations += entity.citations.len();

        for key in entity.attributes.keys() {
            *state.attribute_key_counts.entry(key.clone()).or_default() += 1;
        }
        for rule_hook in &entity.rule_hooks {
            let phase = rule_hook.phase.as_deref().unwrap_or("<none>");
            *state
                .rule_phase_counts
                .entry(phase.to_string())
                .or_default() += 1;
        }
        for prereq in &entity.prerequisites {
            *state
                .prerequisite_kind_counts
                .entry(prereq.kind.clone())
                .or_default() += 1;
        }
        *state
            .entity_name_counts
            .entry(entity.name.clone())
            .or_default() += 1;
    }

    CatalogSummary {
        entities: parsed.entities.len(),
        attributes,
        rule_hooks,
        prerequisites,
        citations,
    }
}

fn write_sorted_counts(report: &mut String, title: &str, counts: &HashMap<String, usize>) {
    let mut items: Vec<_> = counts.iter().collect();
    items.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));

    writeln!(report, "{title}").unwrap();
    if items.is_empty() {
        writeln!(report, "none").unwrap();
    } else {
        for (key, count) in items {
            writeln!(report, "{:6} | {}", count, key).unwrap();
        }
    }
    writeln!(report).unwrap();
}
