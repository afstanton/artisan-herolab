use artisan_core::id::FormatId;
use artisan_herolab::HerolabLoader;
use serde_json::Value;

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
    assert_eq!(
        parsed.sources[0].game_systems,
        vec!["pathfinder".to_string()]
    );

    assert_eq!(parsed.entity_types.len(), 1);
    assert_eq!(parsed.entities.len(), 1);
    assert_eq!(parsed.entities[0].name, "Human");
    assert!(
        parsed.entity_types[0]
            .external_ids
            .iter()
            .any(|id| id.format == FormatId::Herolab
                && id.namespace.as_deref() == Some("entity_type")
                && id.value == "herolab:pathfinder:compset:Race")
    );
    assert_eq!(parsed.entity_types[0].key, "herolab.pathfinder.race");
    assert!(
        parsed.entities[0]
            .external_ids
            .iter()
            .any(|id| id.format == FormatId::Herolab
                && id.namespace.as_deref() == Some("thing_id")
                && id.value == "tRaceHuman")
    );
    assert!(
        parsed.entities[0]
            .external_ids
            .iter()
            .any(|id| id.format == FormatId::Herolab
                && id.namespace.as_deref() == Some("compset")
                && id.value == "Race")
    );

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
    assert_eq!(parsed.entities[0].citations.len(), 1);
}

#[test]
fn parse_user_catalog_preserves_fieldvals_and_eval_blocks() {
    let input = include_str!("fixtures/herolab/input/sample_small_realistic.user");

    let parsed = HerolabLoader::parse_user_catalog(input, "sample_small_realistic.user")
        .expect("parse realistic user catalog");

    assert_eq!(parsed.entities.len(), 2);

    let human = parsed
        .entities
        .iter()
        .find(|entity| entity.name == "Human")
        .expect("human entity");
    assert_eq!(
        human.attributes.get("field:Summary"),
        Some(&Value::String("Baseline human ancestry.".to_string()))
    );
    assert_eq!(
        human.attributes.get("thing_compset"),
        Some(&Value::String("Race".to_string()))
    );

    let wizard = parsed
        .entities
        .iter()
        .find(|entity| entity.name == "Wizard")
        .expect("wizard entity");
    assert_eq!(wizard.rule_hooks.len(), 1);

    let rule_hook = &wizard.rule_hooks[0];
    assert_eq!(rule_hook.phase.as_deref(), Some("Initialize"));
    assert_eq!(rule_hook.priority, Some(1000));

    let script = rule_hook.script.as_ref().expect("script program");
    assert_eq!(
        script.source.as_deref(),
        Some("doneif (field[abValue].value = 0)")
    );
    assert_eq!(
        wizard.attributes.get("thing_id"),
        Some(&Value::String("cWizard".to_string()))
    );
}

#[test]
fn parse_user_catalog_promotes_core_thing_metadata_into_canonical_attributes() {
    let input = r#"
<document signature="Hero Lab Data">
  <thing
    id="fPowerAtk"
    name="Power Attack"
    compset="Feat"
    summary="Trade accuracy for damage."
    description="You can take a penalty on attacks for extra damage."
    uniqueness="useronce"
  />
</document>
"#;

    let parsed =
        HerolabLoader::parse_user_catalog(input, "metadata.user").expect("parse metadata user");

    let feat = parsed
        .entities
        .iter()
        .find(|entity| entity.name == "Power Attack")
        .unwrap();
    assert_eq!(
        feat.attributes.get("summary"),
        Some(&Value::String("Trade accuracy for damage.".to_string()))
    );
    assert_eq!(
        feat.attributes.get("description"),
        Some(&Value::String(
            "You can take a penalty on attacks for extra damage.".to_string()
        ))
    );
    assert_eq!(
        feat.attributes.get("uniqueness"),
        Some(&Value::String("useronce".to_string()))
    );
}

#[test]
fn parse_user_catalog_structures_tag_autotag_and_assignval_children() {
    let input = r#"
<document signature="Hero Lab Data">
  <thing id="wMoonSickle" name="Moon Sickle" compset="Weapon">
    <tag group="GearType" tag="gtWondrous" name="Wondrous Item" abbrev="Wondrous"/>
    <tag group="Helper" tag="NeedAttune"/>
    <autotag group="Usage" tag="Day"/>
    <autotag group="Helper" tag="ItemSpell"/>
    <assignval field="sNameMod" value="save DC 13"/>
    <assignval field="trkMax" value="1" behavior="replace"/>
  </thing>
</document>
"#;

    let parsed =
        HerolabLoader::parse_user_catalog(input, "tags.user").expect("parse tag-rich user");
    let item = parsed
        .entities
        .iter()
        .find(|entity| entity.name == "Moon Sickle")
        .expect("weapon entity");

    assert_eq!(
        item.attributes.get("assigned:sNameMod"),
        Some(&Value::String("save DC 13".to_string()))
    );
    assert_eq!(
        item.attributes.get("assigned:trkMax"),
        Some(&Value::String("1".to_string()))
    );

    let tags = item
        .attributes
        .get("tags")
        .and_then(Value::as_array)
        .expect("tags array");
    assert!(tags.iter().any(|tag| {
        tag.get("group").and_then(Value::as_str) == Some("GearType")
            && tag.get("tag").and_then(Value::as_str) == Some("gtWondrous")
            && tag.get("name").and_then(Value::as_str) == Some("Wondrous Item")
            && tag.get("abbrev").and_then(Value::as_str) == Some("Wondrous")
    }));
    assert!(tags.iter().any(|tag| {
        tag.get("group").and_then(Value::as_str) == Some("Helper")
            && tag.get("tag").and_then(Value::as_str) == Some("NeedAttune")
    }));

    let autotags = item
        .attributes
        .get("autotags")
        .and_then(Value::as_array)
        .expect("autotags array");
    assert!(autotags.iter().any(|tag| {
        tag.get("group").and_then(Value::as_str) == Some("Usage")
            && tag.get("tag").and_then(Value::as_str) == Some("Day")
    }));
    assert!(autotags.iter().any(|tag| {
        tag.get("group").and_then(Value::as_str) == Some("Helper")
            && tag.get("tag").and_then(Value::as_str) == Some("ItemSpell")
    }));

    let assignvals = item
        .attributes
        .get("assignvals")
        .and_then(Value::as_array)
        .expect("assignvals array");
    assert!(assignvals.iter().any(|assign| {
        assign.get("field").and_then(Value::as_str) == Some("sNameMod")
            && assign.get("value").and_then(Value::as_str) == Some("save DC 13")
    }));
    assert!(assignvals.iter().any(|assign| {
        assign.get("field").and_then(Value::as_str) == Some("trkMax")
            && assign.get("value").and_then(Value::as_str) == Some("1")
            && assign.get("behavior").and_then(Value::as_str) == Some("replace")
    }));
}

#[test]
fn parse_user_catalog_adds_canonical_aliases_for_common_fieldvals() {
    let input = r#"
<document signature="Hero Lab Data">
  <thing id="spStorm" name="Storm Cage" compset="Spell">
    <fieldval field="sRange" value="150 feet" />
    <fieldval field="sDuration" value="Concentration, up to 1 minute" />
    <fieldval field="sTarget" value="A point you can see" />
    <fieldval field="sSave" value="Strength negates" />
    <fieldval field="sCompDesc" value="V, S" />
  </thing>
</document>
"#;

    let parsed = HerolabLoader::parse_user_catalog(input, "spell_alias.user")
        .expect("parse spell alias user");
    let spell = parsed
        .entities
        .iter()
        .find(|entity| entity.name == "Storm Cage")
        .expect("spell entity");

    assert_eq!(
        spell.attributes.get("range"),
        Some(&Value::String("150 feet".to_string()))
    );
    assert_eq!(
        spell.attributes.get("duration"),
        Some(&Value::String("Concentration, up to 1 minute".to_string()))
    );
    assert_eq!(
        spell.attributes.get("target"),
        Some(&Value::String("A point you can see".to_string()))
    );
    assert_eq!(
        spell.attributes.get("saving_throw"),
        Some(&Value::String("Strength negates".to_string()))
    );
    assert_eq!(
        spell.attributes.get("components"),
        Some(&Value::String("V, S".to_string()))
    );
}

#[test]
fn parse_user_catalog_derives_canonical_spell_semantics_from_tags() {
    let input = r#"
<document signature="Hero Lab Data">
  <thing id="spBorrowed" name="Borrowed Knowledge" compset="Spell">
    <usesource source="5eTCoE"/>
    <tag group="sClass" tag="cHelpBrd"/>
    <tag group="sClass" tag="cHelpWiz"/>
    <tag group="sLevel" tag="2"/>
    <tag group="sCastTime" tag="Action1"/>
    <tag group="sSchool" tag="Divination"/>
    <tag group="sComp" tag="V"/>
    <tag group="sComp" tag="S"/>
    <tag group="sComp" tag="M"/>
  </thing>
</document>
"#;

    let parsed =
        HerolabLoader::parse_user_catalog(input, "spell_tags.user").expect("parse spell tags user");
    let spell = parsed
        .entities
        .iter()
        .find(|entity| entity.name == "Borrowed Knowledge")
        .expect("spell entity");

    assert_eq!(
        spell.attributes.get("spell_level"),
        Some(&Value::String("2".to_string()))
    );
    assert_eq!(
        spell.attributes.get("cast_time"),
        Some(&Value::String("Action1".to_string()))
    );
    assert_eq!(
        spell.attributes.get("spell_school"),
        Some(&Value::String("Divination".to_string()))
    );
    assert_eq!(
        spell.attributes.get("spell_classes"),
        Some(&Value::Array(vec![
            Value::String("cHelpBrd".to_string()),
            Value::String("cHelpWiz".to_string()),
        ]))
    );
    assert_eq!(
        spell.attributes.get("components"),
        Some(&Value::Array(vec![
            Value::String("V".to_string()),
            Value::String("S".to_string()),
            Value::String("M".to_string()),
        ]))
    );
    let spell_type = parsed
        .entity_types
        .iter()
        .find(|entity_type| entity_type.id == spell.entity_type)
        .expect("scoped spell entity type");
    assert_eq!(spell_type.key, "herolab.dnd5e.spell");
}

#[test]
fn parse_user_catalog_derives_canonical_item_semantics_from_tags() {
    let input = r#"
<document signature="Hero Lab Data">
  <thing id="itBook" name="Sinister Spellbook" compset="Gear">
    <tag group="GearType" tag="gtWondrous"/>
    <tag group="ItemRarity" tag="Rare"/>
    <tag group="Helper" tag="ShowSpec"/>
    <tag group="Helper" tag="NeedAttune"/>
    <tag group="Usage" tag="Day"/>
    <tag group="ChargeUse" tag="1"/>
    <autotag group="Usage" tag="ShortRest"/>
    <autotag group="Helper" tag="ItemSpell"/>
    <autotag group="ChargeUse" tag="2"/>
    <autotag group="Recharge" tag="6"/>
  </thing>
</document>
"#;

    let parsed =
        HerolabLoader::parse_user_catalog(input, "item_tags.user").expect("parse item tags user");
    let item = parsed
        .entities
        .iter()
        .find(|entity| entity.name == "Sinister Spellbook")
        .expect("item entity");

    assert_eq!(
        item.attributes.get("gear_type"),
        Some(&Value::String("gtWondrous".to_string()))
    );
    assert_eq!(
        item.attributes.get("item_rarity"),
        Some(&Value::String("Rare".to_string()))
    );
    assert_eq!(
        item.attributes.get("helper_flags"),
        Some(&Value::Array(vec![
            Value::String("ShowSpec".to_string()),
            Value::String("NeedAttune".to_string()),
            Value::String("ItemSpell".to_string()),
        ]))
    );
    assert_eq!(
        item.attributes.get("usages"),
        Some(&Value::Array(vec![
            Value::String("Day".to_string()),
            Value::String("ShortRest".to_string()),
        ]))
    );
    assert_eq!(
        item.attributes.get("charges_per_use"),
        Some(&Value::Array(vec![
            Value::String("1".to_string()),
            Value::String("2".to_string()),
        ]))
    );
    assert_eq!(
        item.attributes.get("recharge"),
        Some(&Value::String("6".to_string()))
    );
}

#[test]
fn parse_user_catalog_derives_action_usage_and_recharge_semantics_from_tags() {
    let input = r#"
<document signature="Hero Lab Data">
  <thing id="abBlink" name="Blink Step" compset="Ability">
    <tag group="FeatureTyp" tag="Action"/>
    <tag group="abAction" tag="Bonus"/>
    <tag group="Usage" tag="LongRest"/>
    <tag group="Recharge" tag="5"/>
  </thing>
</document>
"#;

    let parsed = HerolabLoader::parse_user_catalog(input, "action_tags.user")
        .expect("parse action tags user");
    let ability = parsed
        .entities
        .iter()
        .find(|entity| entity.name == "Blink Step")
        .expect("ability entity");

    assert_eq!(
        ability.attributes.get("feature_type"),
        Some(&Value::String("Action".to_string()))
    );
    assert_eq!(
        ability.attributes.get("action_type"),
        Some(&Value::String("Bonus".to_string()))
    );
    assert_eq!(
        ability.attributes.get("recharge"),
        Some(&Value::String("5".to_string()))
    );
    assert_eq!(
        ability.attributes.get("usages"),
        Some(&Value::Array(vec![Value::String("LongRest".to_string())]))
    );
}

#[test]
fn parse_user_catalog_derives_race_and_skill_classification_semantics_from_tags() {
    let input = r#"
<document signature="Hero Lab Data">
  <thing id="rGoblin" name="Goblin" compset="Race">
    <tag group="RaceType" tag="NPC"/>
    <tag group="RaceSize" tag="Small11" name="Small" abbrev="Small"/>
    <tag group="Alignment" tag="Chaotic"/>
    <tag group="Alignment" tag="Evil"/>
    <tag group="ProfSkill" tag="skStealth"/>
    <tag group="ProfSkill" tag="skPercep"/>
  </thing>
  <thing id="cRogue" name="Rogue" compset="Class">
    <tag group="ClassSkill" tag="skStealth"/>
    <tag group="ClassSkill" tag="skAcrobat"/>
  </thing>
</document>
"#;

    let parsed = HerolabLoader::parse_user_catalog(input, "race_class_tags.user")
        .expect("parse race/class tags user");
    let race = parsed
        .entities
        .iter()
        .find(|entity| entity.name == "Goblin")
        .expect("race entity");
    let class = parsed
        .entities
        .iter()
        .find(|entity| entity.name == "Rogue")
        .expect("class entity");

    assert_eq!(
        race.attributes.get("race_type"),
        Some(&Value::String("NPC".to_string()))
    );
    assert_eq!(
        race.attributes.get("race_size"),
        Some(&Value::String("Small11".to_string()))
    );
    assert_eq!(
        race.attributes.get("alignments"),
        Some(&Value::Array(vec![
            Value::String("Chaotic".to_string()),
            Value::String("Evil".to_string()),
        ]))
    );
    assert_eq!(
        race.attributes.get("proficient_skills"),
        Some(&Value::Array(vec![
            Value::String("skStealth".to_string()),
            Value::String("skPercep".to_string()),
        ]))
    );
    assert_eq!(
        class.attributes.get("class_skills"),
        Some(&Value::Array(vec![
            Value::String("skStealth".to_string()),
            Value::String("skAcrobat".to_string()),
        ]))
    );
}

#[test]
fn parse_user_catalog_infers_entity_types_from_matrix_compsets() {
    let input = r#"
<document signature="Hero Lab Data">
  <thing id="rHuman" name="Human" compset="Race">
    <usesource id="srcCore" name="Core Book" parent="Test Publisher" />
  </thing>
  <thing id="fAlert" name="Alertness" compset="Feat">
    <exprreq><![CDATA[hero.tagis[Helper.Ready]]]></exprreq>
  </thing>
  <thing id="spMagic" name="Magic Missile" compset="Spell">
    <fieldval field="sRange" value="120 feet" />
  </thing>
  <thing id="cWizard" name="Wizard" compset="Class">
    <eval phase="Initialize" priority="1000"><![CDATA[
      doneif (field[abValue].value = 0)
    ]]></eval>
  </thing>
  <thing id="wSword" name="Longsword" compset="Weapon" />
  <thing id="tCelestial" name="Celestial" compset="Template" />
</document>
"#;

    let parsed =
        HerolabLoader::parse_user_catalog(input, "matrix.user").expect("parse matrix catalog");

    let entity_type_keys: std::collections::BTreeSet<_> = parsed
        .entity_types
        .iter()
        .map(|entity_type| entity_type.key.as_str())
        .collect();
    assert_eq!(
        entity_type_keys,
        std::collections::BTreeSet::from([
            "herolab.class",
            "herolab.feat",
            "herolab.race",
            "herolab.spell",
            "herolab.template",
            "herolab.weapon",
        ])
    );

    let entity_type_by_id: std::collections::HashMap<_, _> = parsed
        .entity_types
        .iter()
        .map(|entity_type| (entity_type.id.0.to_string(), entity_type.key.as_str()))
        .collect();

    let names_to_expected_keys = [
        ("Human", "herolab.race"),
        ("Alertness", "herolab.feat"),
        ("Magic Missile", "herolab.spell"),
        ("Wizard", "herolab.class"),
        ("Longsword", "herolab.weapon"),
        ("Celestial", "herolab.template"),
    ];

    for (name, expected_key) in names_to_expected_keys {
        let entity = parsed
            .entities
            .iter()
            .find(|entity| entity.name == name)
            .unwrap_or_else(|| panic!("missing entity {name}"));
        assert_eq!(
            entity_type_by_id.get(&entity.entity_type.0.to_string()),
            Some(&expected_key),
            "wrong inferred entity type for {name}"
        );
    }

    let spell_type = parsed
        .entity_types
        .iter()
        .find(|entity_type| entity_type.key == "herolab.spell")
        .expect("spell entity type");
    let spell_field_keys: std::collections::BTreeSet<_> = spell_type
        .fields
        .iter()
        .map(|field| field.key.as_str())
        .collect();
    assert!(spell_field_keys.contains("range"));
    assert!(spell_field_keys.contains("duration"));
    assert!(spell_field_keys.contains("target"));
    assert!(spell_field_keys.contains("saving_throw"));
    assert!(spell_field_keys.contains("spell_level"));
    assert!(spell_field_keys.contains("spell_school"));
    assert!(spell_field_keys.contains("spell_classes"));

    let weapon_type = parsed
        .entity_types
        .iter()
        .find(|entity_type| entity_type.key == "herolab.weapon")
        .expect("weapon entity type");
    let weapon_field_keys: std::collections::BTreeSet<_> = weapon_type
        .fields
        .iter()
        .map(|field| field.key.as_str())
        .collect();
    assert!(weapon_field_keys.contains("gear_type"));
    assert!(weapon_field_keys.contains("item_rarity"));
    assert!(weapon_field_keys.contains("helper_flags"));
    assert!(weapon_field_keys.contains("usages"));
    assert!(weapon_field_keys.contains("charges_per_use"));
    assert!(weapon_field_keys.contains("action_type"));
    assert!(weapon_field_keys.contains("feature_type"));
    assert!(weapon_field_keys.contains("recharge"));

    let race_type = parsed
        .entity_types
        .iter()
        .find(|entity_type| entity_type.key == "herolab.race")
        .expect("race entity type");
    let race_field_keys: std::collections::BTreeSet<_> = race_type
        .fields
        .iter()
        .map(|field| field.key.as_str())
        .collect();
    assert!(race_field_keys.contains("race_type"));
    assert!(race_field_keys.contains("race_size"));
    assert!(race_field_keys.contains("alignments"));
    assert!(race_field_keys.contains("proficient_skills"));

    let feat_type = parsed
        .entity_types
        .iter()
        .find(|entity_type| entity_type.key == "herolab.feat")
        .expect("feat entity type");
    let feat_field_keys: std::collections::BTreeSet<_> = feat_type
        .fields
        .iter()
        .map(|field| field.key.as_str())
        .collect();
    assert!(feat_field_keys.contains("charges_per_use"));
}

#[test]
fn parse_user_catalog_falls_back_to_slugged_entity_type_for_unknown_compset() {
    let input = r#"
<document signature="Hero Lab Data">
  <thing id="xMystery" name="Mystery Thing" compset="ArcaneWidget" />
</document>
"#;

    let parsed =
        HerolabLoader::parse_user_catalog(input, "unknown.user").expect("parse unknown compset");

    assert_eq!(parsed.entity_types.len(), 1);
    assert_eq!(parsed.entity_types[0].key, "herolab.arcane_widget");
    assert_eq!(parsed.entity_types[0].name, "HeroLab ArcaneWidget");
    assert_eq!(parsed.entities.len(), 1);
    assert_eq!(parsed.entities[0].entity_type, parsed.entity_types[0].id);
}

#[test]
fn parse_user_catalog_scopes_entity_types_by_game_system_when_source_hints_match() {
    let input = r#"
<document signature="Hero Lab Data">
  <thing id="spBlade" name="Blade of Disaster" compset="Spell">
    <usesource source="5eTCoE"/>
  </thing>
</document>
"#;

    let parsed =
        HerolabLoader::parse_user_catalog(input, "COM_5ePack_TCoE - Spells.user").expect("parse");

    assert_eq!(parsed.sources.len(), 1);
    assert_eq!(parsed.sources[0].game_systems, vec!["dnd5e".to_string()]);
    assert_eq!(parsed.entity_types.len(), 1);
    assert_eq!(parsed.entity_types[0].key, "herolab.dnd5e.spell");
    assert_eq!(parsed.entity_types[0].name, "HeroLab Spell (D&D 5e)");
    assert!(
        parsed.entity_types[0]
            .external_ids
            .iter()
            .any(|id| id.namespace.as_deref() == Some("entity_type")
                && id.value == "herolab:dnd5e:compset:Spell")
    );
}

#[test]
fn parse_user_catalog_preserves_multiple_usesources_as_distinct_citations() {
    let input = r#"
<document signature="Hero Lab Data">
  <thing id="spBlade" name="Blade of Disaster" compset="Spell">
    <usesource source="5eTCoE" name="Tasha's Cauldron of Everything" />
    <usesource source="p5eIDRotFP" name="Icewind Dale: Rime of the Frostmaiden" />
  </thing>
</document>
"#;

    let parsed = HerolabLoader::parse_user_catalog(input, "COM_5ePack_TCoE - Spells.user")
        .expect("parse multi-source user");

    assert_eq!(parsed.sources.len(), 2);
    let source_titles: std::collections::BTreeSet<_> = parsed
        .sources
        .iter()
        .map(|source| source.title.as_str())
        .collect();
    assert_eq!(
        source_titles,
        std::collections::BTreeSet::from([
            "Icewind Dale: Rime of the Frostmaiden",
            "Tasha's Cauldron of Everything",
        ])
    );
    assert_eq!(parsed.entities.len(), 1);
    assert_eq!(parsed.entities[0].citations.len(), 2);
}

#[test]
fn parse_user_catalog_extracts_source_records_from_source_definition_files() {
    let input = r#"
<document signature="Hero Lab Structure">
  <source
    id="pSKR"
    name="Sean K Reynolds Games"
    selectable="no"
    parent="p3PPPack"
    sortorder="10"
    description="Parent Source for Sean K Reynolds Games products.">
  </source>
  <source
    id="srcRobmanc"
    name="SKR - Robomancer"
    selectable="yes"
    parent="pSKR"
    sortorder="10"
    description="Robomancer product.">
  </source>
</document>
"#;

    let parsed = HerolabLoader::parse_user_catalog(input, "COM_Source_210 - SKR Games.1st")
        .expect("parse source definitions");

    assert!(parsed.entities.is_empty());
    assert_eq!(parsed.sources.len(), 2);
    let source_ids: std::collections::BTreeSet<_> = parsed
        .sources
        .iter()
        .flat_map(|source| source.external_ids.iter())
        .filter(|id| id.namespace.as_deref() == Some("source_id"))
        .map(|id| id.value.as_str())
        .collect();
    assert_eq!(
        source_ids,
        std::collections::BTreeSet::from(["pSKR", "srcRobmanc"])
    );
}
