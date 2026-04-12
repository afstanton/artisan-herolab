use artisan_herolab::HerolabLoader;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::io::Read as _;
use std::path::{Path, PathBuf};

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: token_inventory <herolab_path1> [herolab_path2] ...");
        eprintln!("Example: token_inventory /path/to/HL-Pack-Pathfinder /path/to/HL_DD_5e_Colab");
        std::process::exit(1);
    }

    let mut report_state = InventoryReport::default();
    let mut progress = Progress::default();

    eprintln!("token_inventory: scanning {} root(s)", args.len() - 1);
    for root_arg in &args[1..] {
        scan_path(Path::new(root_arg), &mut report_state, &mut progress)?;
    }

    let mut report = String::new();
    writeln!(report, "=== HeroLab Token Inventory Summary ===").unwrap();
    writeln!(report, "Files scanned: {}", report_state.files_scanned).unwrap();
    writeln!(
        report,
        "XML documents parsed: {}",
        report_state.xml_documents
    )
    .unwrap();
    writeln!(
        report,
        "Archive files parsed: {}",
        report_state.archive_files
    )
    .unwrap();
    writeln!(
        report,
        "Archive assets seen: {}",
        report_state.archive_assets
    )
    .unwrap();
    writeln!(
        report,
        "Files with UTF-8 decoding issues fixed: {}",
        report_state.lossy_decodes
    )
    .unwrap();
    writeln!(
        report,
        "Distinct document signatures: {}",
        report_state.signature_counts.len()
    )
    .unwrap();
    writeln!(
        report,
        "Distinct XML tags: {}",
        report_state.tag_counts.len()
    )
    .unwrap();
    writeln!(
        report,
        "Distinct XML tag@attribute pairs: {}",
        report_state.attribute_counts.len()
    )
    .unwrap();
    writeln!(
        report,
        "Distinct thing compsets: {}",
        report_state.compset_counts.len()
    )
    .unwrap();
    writeln!(
        report,
        "Distinct script-bearing tags: {}",
        report_state.script_tag_counts.len()
    )
    .unwrap();
    writeln!(report, "Parse failures: {}", report_state.failures.len()).unwrap();
    writeln!(report).unwrap();

    write_sorted_counts(
        &mut report,
        "=== Document Signatures ===",
        &report_state.signature_counts,
    );
    write_sorted_counts(&mut report, "=== XML Tags ===", &report_state.tag_counts);
    write_sorted_counts(
        &mut report,
        "=== XML Tag@Attribute Pairs ===",
        &report_state.attribute_counts,
    );
    write_sorted_counts(
        &mut report,
        "=== Thing Compsets ===",
        &report_state.compset_counts,
    );
    write_sorted_counts(
        &mut report,
        "=== Script-Bearing Tags ===",
        &report_state.script_tag_counts,
    );

    let output_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("TOKEN_INVENTORY.txt");
    fs::write(&output_path, report)?;
    eprintln!(
        "token_inventory: wrote {} (xml_docs={}, archives={}, failures={})",
        output_path.display(),
        report_state.xml_documents,
        report_state.archive_files,
        report_state.failures.len()
    );

    Ok(())
}

#[derive(Default)]
struct InventoryReport {
    files_scanned: usize,
    xml_documents: usize,
    archive_files: usize,
    archive_assets: usize,
    lossy_decodes: usize,
    signature_counts: HashMap<String, usize>,
    tag_counts: HashMap<String, usize>,
    attribute_counts: HashMap<String, usize>,
    compset_counts: HashMap<String, usize>,
    script_tag_counts: HashMap<String, usize>,
    failures: Vec<String>,
}

#[derive(Default)]
struct Progress {
    files_seen: usize,
}

fn scan_path(path: &Path, report: &mut InventoryReport, progress: &mut Progress) -> io::Result<()> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            scan_path(&entry.path(), report, progress)?;
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
        "user" | "xml" | "1st" | "dat" => scan_xml_file(path, report, progress)?,
        "por" | "stock" => scan_archive_file(path, report, progress)?,
        _ => {}
    }

    Ok(())
}

fn scan_xml_file(
    path: &Path,
    report: &mut InventoryReport,
    progress: &mut Progress,
) -> io::Result<()> {
    report.files_scanned += 1;
    note_progress("token_inventory", path, report, progress);
    let (text, was_lossy) = read_text_file_lossy(path)?;
    if was_lossy {
        report.lossy_decodes += 1;
    }
    scan_xml_document(&text, path, report);
    Ok(())
}

fn scan_archive_file(
    path: &Path,
    report: &mut InventoryReport,
    progress: &mut Progress,
) -> io::Result<()> {
    report.files_scanned += 1;
    note_progress("token_inventory", path, report, progress);
    let bytes = fs::read(path)?;
    report.archive_files += 1;

    match HerolabLoader::inspect_portfolio_archive(&bytes) {
        Ok(manifest) => {
            report.archive_assets += manifest.assets.len();
        }
        Err(err) => report.failures.push(format!(
            "{} :: archive parse failed :: {err}",
            path.display()
        )),
    }

    let cursor = std::io::Cursor::new(bytes);
    match zip::ZipArchive::new(cursor) {
        Ok(mut archive) => {
            for index in 0..archive.len() {
                let mut entry = match archive.by_index(index) {
                    Ok(entry) => entry,
                    Err(err) => {
                        report.failures.push(format!(
                            "{} :: zip entry {} unreadable :: {}",
                            path.display(),
                            index,
                            err
                        ));
                        continue;
                    }
                };

                let entry_path = PathBuf::from(entry.name());
                let is_xml = entry_path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| {
                        ext.eq_ignore_ascii_case("xml")
                            || ext.eq_ignore_ascii_case("user")
                            || ext.eq_ignore_ascii_case("1st")
                            || ext.eq_ignore_ascii_case("dat")
                    })
                    .unwrap_or(false);
                if !is_xml {
                    continue;
                }

                let mut bytes = Vec::new();
                match entry.read_to_end(&mut bytes) {
                    Ok(_) => {
                        let (text, was_lossy) = decode_text_lossy(&bytes);
                        if was_lossy {
                            report.lossy_decodes += 1;
                        }
                        scan_xml_document(&text, &path.join(entry.name()), report)
                    }
                    Err(err) => report.failures.push(format!(
                        "{} :: {} :: xml member unreadable :: {}",
                        path.display(),
                        entry.name(),
                        err
                    )),
                }
            }
        }
        Err(err) => {
            report
                .failures
                .push(format!("{} :: zip open failed :: {}", path.display(), err))
        }
    }

    Ok(())
}

fn scan_xml_document(text: &str, path: &Path, report: &mut InventoryReport) {
    match roxmltree::Document::parse(text) {
        Ok(doc) => {
            report.xml_documents += 1;

            if let Some(root) = doc.root_element().attribute("signature") {
                *report.signature_counts.entry(root.to_string()).or_default() += 1;
            }

            for node in doc.descendants().filter(|node| node.is_element()) {
                let tag = node.tag_name().name().to_string();
                *report.tag_counts.entry(tag.clone()).or_default() += 1;

                if is_script_tag(&tag) {
                    *report.script_tag_counts.entry(tag.clone()).or_default() += 1;
                }

                if tag == "thing" {
                    if let Some(compset) = node.attribute("compset") {
                        *report
                            .compset_counts
                            .entry(compset.to_string())
                            .or_default() += 1;
                    }
                }

                for attr in node.attributes() {
                    *report
                        .attribute_counts
                        .entry(format!("{tag}@{}", attr.name()))
                        .or_default() += 1;
                }
            }
        }
        Err(err) => {
            report
                .failures
                .push(format!("{} :: xml parse failed :: {}", path.display(), err))
        }
    }
}

fn is_script_tag(tag: &str) -> bool {
    matches!(
        tag,
        "eval" | "evalrule" | "procedure" | "exprreq" | "validate" | "bootstrap"
    )
}

fn note_progress(label: &str, path: &Path, report: &InventoryReport, progress: &mut Progress) {
    progress.files_seen += 1;
    if progress.files_seen == 1 || progress.files_seen.is_multiple_of(100) {
        eprintln!(
            "{label}: processed {} file(s); current={}; xml_docs={}; archives={}; failures={}",
            progress.files_seen,
            path.display(),
            report.xml_documents,
            report.archive_files,
            report.failures.len()
        );
    }
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
