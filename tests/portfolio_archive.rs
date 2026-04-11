use std::io::{Cursor, Write};

use artisan_herolab::{AssetKind, HerolabLoader};
use zip::write::SimpleFileOptions;

fn build_test_archive() -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default();

        zip.start_file("index.xml", options)
            .expect("start index.xml");
        zip.write_all(
            br#"<document signature='Portfolio Index'><thing id='tRaceHuman' name='Human'><usesource id='srcCore' name='Pathfinder Core Rulebook' parent='Paizo Inc.'/></thing></document>"#,
        )
        .expect("write index.xml");

        zip.start_file("herolab/lead1.xml", options)
            .expect("start lead xml");
        zip.write_all(br#"<document signature='Hero Lab Lead'></document>"#)
            .expect("write lead xml");

        zip.start_file("statblocks_text/hero.txt", options)
            .expect("start txt");
        zip.write_all(b"Hero stat block").expect("write txt");

        zip.start_file("images/portrait.png", options)
            .expect("start image");
        zip.write_all(&[0_u8, 1, 2, 3, 4]).expect("write image");

        zip.finish().expect("finish zip");
    }
    cursor.into_inner()
}

#[test]
fn inspect_portfolio_archive_extracts_assets_and_catalog() {
    let bytes = build_test_archive();

    let manifest = HerolabLoader::inspect_portfolio_archive(&bytes).expect("inspect archive");

    assert_eq!(manifest.assets.len(), 4);
    assert!(
        manifest
            .assets
            .iter()
            .any(|a| a.path == "images/portrait.png" && a.kind == AssetKind::Image)
    );
    assert!(
        manifest
            .assets
            .iter()
            .any(|a| a.path == "index.xml" && a.kind == AssetKind::Xml)
    );

    assert_eq!(manifest.catalog.publishers.len(), 1);
    assert_eq!(manifest.catalog.sources.len(), 1);
    assert_eq!(manifest.catalog.entities.len(), 1);
}
