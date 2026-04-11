use artisan_herolab::HerolabLoader;

#[test]
fn parse_user_catalog_extracts_privileged_records() {
    let input = r#"
<document signature="Hero Lab Data">
  <thing id="tRaceHuman" name="Human" compset="Race">
    <usesource id="srcCore" name="Pathfinder Core Rulebook" parent="Paizo Inc." />
  </thing>
</document>
"#;

    let parsed =
        HerolabLoader::parse_user_catalog(input, "sample.user").expect("parse user catalog");

    assert_eq!(parsed.publishers.len(), 1);
    assert_eq!(parsed.publishers[0].name, "Paizo Inc.");

    assert_eq!(parsed.sources.len(), 1);
    assert_eq!(parsed.sources[0].title, "Pathfinder Core Rulebook");
    assert_eq!(parsed.sources[0].publisher.as_deref(), Some("Paizo Inc."));

    assert_eq!(parsed.entity_types.len(), 1);
    assert_eq!(parsed.entities.len(), 1);
    assert_eq!(parsed.entities[0].name, "Human");

    assert_eq!(parsed.citations.len(), 1);
    assert_eq!(parsed.entities[0].citations.len(), 1);
    assert_eq!(parsed.entities[0].citations[0], parsed.citations[0].id);
}

#[test]
fn parse_user_catalog_falls_back_to_filename_source() {
    let input = r#"
<document signature="Hero Lab Data">
  <thing id="tSimple" name="Simple" />
</document>
"#;

    let parsed =
        HerolabLoader::parse_user_catalog(input, "fallback.user").expect("parse user catalog");

    assert_eq!(parsed.publishers.len(), 0);
    assert_eq!(parsed.sources.len(), 1);
    assert_eq!(parsed.sources[0].title, "fallback.user");
}
