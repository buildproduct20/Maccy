use maccy_core::CoreError;
use maccy_core::import::{ImportResult, import_settings};
use maccy_core::models::{SUPPORTED_TYPES, TEXT};
use maccy_core::repository::Store;
use maccy_core::settings::{SAFETY_TYPES, Settings};
use plist::{Dictionary, Value};
use serde_json::json;

fn array(values: &[&str]) -> Value {
    Value::Array(values.iter().map(|v| Value::String((*v).into())).collect())
}

fn encode(value: Value, binary: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    if binary {
        value.to_writer_binary(&mut bytes).unwrap();
    } else {
        value.to_writer_xml(&mut bytes).unwrap();
    }
    bytes
}

fn fixture(entries: &[(&str, Value)], binary: bool) -> Vec<u8> {
    let mut values = Dictionary::new();
    for (key, value) in entries {
        values.insert((*key).into(), value.clone());
    }
    encode(Value::Dictionary(values), binary)
}

fn has(keys: &[String], key: &str) -> bool {
    keys.iter().any(|v| v == key)
}

#[test]
fn xml_and_binary_map_only_compatible_settings() {
    for binary in [false, true] {
        let bytes = fixture(
            &[
                ("enabledPasteboardTypes", array(&[TEXT])),
                ("ignoredApps", array(&["com.example.editor"])),
                ("ignoreAllAppsExceptListed", Value::Boolean(true)),
                ("ignoreRegexp", array(&["^private", "secret$"])),
                ("ignoredPasteboardTypes", array(&["com.example.private"])),
                ("clipboardCheckInterval", Value::Real(0.75)),
                ("searchMode", Value::String("exact".into())),
            ],
            binary,
        );
        assert_eq!(bytes.starts_with(b"bplist00"), binary);
        let current = Settings::default();
        let imported = import_settings(&bytes, &current, false).unwrap();
        assert_eq!(imported.settings.enabled_types, [TEXT]);
        assert_eq!(imported.settings.ignored_apps, ["com.example.editor"]);
        assert!(imported.settings.allowlist_apps);
        assert_eq!(imported.settings.ignored_patterns, ["^private", "secret$"]);
        assert_eq!(imported.settings.ignored_types, ["com.example.private"]);
        assert_eq!(imported.settings.poll_interval_ms, 750);
        assert_eq!(imported.settings.max_items, 200);
        assert_eq!(imported.settings.retention_days, 7);
        assert_eq!(imported.report.imported_keys.len(), 7);
        assert!(imported.report.skipped_keys.is_empty());
        assert!(imported.report.invalid_keys.is_empty());
        assert_eq!(imported.report.invalid_entries, 0);
        assert!(!imported.report.already_imported);
        assert_eq!(
            imported.settings.import_digest,
            Some(blake3::hash(&bytes).to_hex().to_string())
        );
        assert_eq!(current, Settings::default());
        let serialized = serde_json::to_vec(&imported).unwrap();
        assert_eq!(
            serde_json::from_slice::<ImportResult>(&serialized).unwrap(),
            imported
        );
    }
}

#[test]
fn seconds_are_clamped_and_rounded_to_safe_milliseconds() {
    for (value, expected) in [
        (Value::Real(0.0), 100),
        (Value::Real(0.001), 100),
        (Value::Real(0.5006), 501),
        (Value::Integer(2.into()), 2000),
        (Value::Real(1000.0), 5000),
        (Value::Real(f64::MAX), 5000),
    ] {
        let bytes = fixture(&[("clipboardCheckInterval", value)], true);
        let imported = import_settings(&bytes, &Settings::default(), false).unwrap();
        assert_eq!(imported.settings.poll_interval_ms, expected);
        assert_eq!(imported.report.imported_keys, ["clipboardCheckInterval"]);
    }
}

#[test]
fn invalid_intervals_are_not_imported() {
    for value in [
        Value::Real(-1.0),
        Value::Real(f64::NAN),
        Value::Real(f64::INFINITY),
        Value::Boolean(true),
        Value::String("0.5".into()),
    ] {
        let bytes = fixture(&[("clipboardCheckInterval", value)], true);
        let imported = import_settings(&bytes, &Settings::default(), false).unwrap();
        assert_eq!(imported.settings, Settings::default());
        assert_eq!(imported.report.invalid_keys, ["clipboardCheckInterval"]);
        assert_eq!(imported.report.invalid_entries, 0);
    }
}

#[test]
fn malformed_oversized_and_non_dictionary_inputs_are_sanitized_errors() {
    let current = Settings::default();
    for bytes in [
        Vec::new(),
        b"not a plist: /synthetic/private/source".to_vec(),
        b"<?xml version=\"1.0\"?><plist><dict><key>broken</key>".to_vec(),
        b"bplist00truncated".to_vec(),
        b"{ clipboardCheckInterval = 1; }".to_vec(),
        encode(Value::Array(vec![]), false),
        encode(Value::String("not a dictionary".into()), true),
        vec![b'x'; 2 * 1024 * 1024 + 1],
    ] {
        let error = import_settings(&bytes, &current, false).unwrap_err();
        assert_eq!(error, CoreError::InvalidInput);
        assert_eq!(error.to_string(), "invalid_input");
        assert_eq!(current, Settings::default());
    }
}

#[test]
fn removed_history_size_pin_and_unknown_keys_are_explicitly_skipped() {
    let keys = [
        "historySize",
        "size",
        "max_items",
        "retention_days",
        "history",
        "storagePath",
        "pinTo",
        "pinOrder",
        "showSpecialSymbols",
        "clearOnQuit",
        "clearSystemClipboard",
        "pasteByDefault",
        "removeFormattingByDefault",
        "popupPosition",
        "windowSize",
        "menuIcon",
        "showFooter",
        "showTitle",
        "KeyboardShortcuts_popup",
        "lastReviewRequestedAt",
        "numberOfUsages",
        "migrations",
        "sortBy",
        "checkForUpdates",
        "showInStatusBar",
        "futureKey",
    ];
    let entries: Vec<_> = keys
        .iter()
        .map(|key| {
            (
                *key,
                Value::String("/synthetic/nonexistent/old-history.sqlite".into()),
            )
        })
        .collect();
    let imported = import_settings(&fixture(&entries, false), &Settings::default(), false).unwrap();
    assert_eq!(imported.settings, Settings::default());
    assert_eq!(imported.report.skipped_keys.len(), keys.len());
    for key in keys {
        assert!(has(&imported.report.skipped_keys, key));
    }
    assert!(imported.report.imported_keys.is_empty());
    assert!(imported.report.invalid_keys.is_empty());
    assert!(
        !serde_json::to_string(&imported.report)
            .unwrap()
            .contains("/synthetic")
    );
}

#[test]
fn user_retention_values_and_pause_are_never_imported_even_with_overwrite() {
    let current = Settings {
        max_items: 321,
        retention_days: 12,
        paused: true,
        ..Settings::default()
    };
    let bytes = fixture(
        &[
            ("max_items", Value::Integer(999.into())),
            ("retention_days", Value::Integer(90.into())),
            ("paused", Value::Boolean(false)),
            ("clipboardCheckInterval", Value::Real(0.8)),
        ],
        true,
    );
    let imported = import_settings(&bytes, &current, true).unwrap();
    assert_eq!(imported.settings.max_items, 321);
    assert_eq!(imported.settings.retention_days, 12);
    assert!(imported.settings.paused);
    assert_eq!(imported.settings.poll_interval_ms, 800);
    assert_eq!(imported.report.skipped_keys.len(), 3);
}

#[test]
fn mandatory_privacy_markers_cannot_be_removed_by_empty_source_list() {
    let bytes = fixture(&[("ignoredPasteboardTypes", array(&[]))], false);
    let imported = import_settings(&bytes, &Settings::default(), false).unwrap();
    assert!(imported.settings.ignored_types.is_empty());
    for marker in SAFETY_TYPES {
        for advertised in [false, true] {
            let payload = serde_json::from_value(json!({
                "text": "synthetic public text",
                "source_app": "com.example.synthetic",
                "advertised_types": if advertised { vec![*marker] } else { vec![] },
                "representations": if advertised { json!([]) } else {
                    json!([{"item_index": 0, "uti": marker, "data": [1, 2, 3]}])
                }
            }))
            .unwrap();
            assert!(imported.settings.ignores(&payload).unwrap(), "{marker}");
        }
    }
}

#[test]
fn invalid_regex_and_type_entries_do_not_discard_valid_neighbors() {
    let long = "x".repeat(513);
    let bytes = fixture(
        &[
            (
                "ignoreRegexp",
                Value::Array(vec![
                    Value::String("^valid$".into()),
                    Value::String("[".into()),
                    Value::String("(?<=secret)text".into()),
                    Value::String("(secret)\\1".into()),
                    Value::String(long),
                    Value::Boolean(false),
                    Value::String("other".into()),
                ]),
            ),
            (
                "enabledPasteboardTypes",
                Value::Array(vec![
                    Value::String(TEXT.into()),
                    Value::String("unsupported.uti".into()),
                    Value::Integer(42.into()),
                ]),
            ),
            (
                "ignoredPasteboardTypes",
                Value::Array(vec![
                    Value::String("com.example.secret".into()),
                    Value::String("".into()),
                    Value::String("bad\nvalue".into()),
                    Value::String("x".repeat(256)),
                    Value::Boolean(false),
                ]),
            ),
        ],
        false,
    );
    let imported = import_settings(&bytes, &Settings::default(), false).unwrap();
    assert_eq!(imported.settings.ignored_patterns, ["^valid$", "other"]);
    assert_eq!(imported.settings.enabled_types, [TEXT]);
    assert_eq!(imported.settings.ignored_types, ["com.example.secret"]);
    assert_eq!(imported.report.imported_keys.len(), 3);
    assert_eq!(imported.report.invalid_keys.len(), 3);
    assert_eq!(imported.report.invalid_entries, 11);
    let report = serde_json::to_string(&imported.report).unwrap();
    assert!(!report.contains("secret"));
    assert!(!report.contains("unsupported.uti"));
}

#[test]
fn wrong_field_types_preserve_settings_and_do_not_record_success() {
    let bytes = fixture(
        &[
            ("enabledPasteboardTypes", Value::String(TEXT.into())),
            ("ignoredApps", Value::Boolean(false)),
            ("ignoreAllAppsExceptListed", Value::Integer(1.into())),
            ("ignoreRegexp", Value::String("private".into())),
            (
                "ignoredPasteboardTypes",
                Value::Dictionary(Dictionary::new()),
            ),
            ("clipboardCheckInterval", array(&["0.5"])),
            ("searchMode", Value::Boolean(false)),
        ],
        true,
    );
    let imported = import_settings(&bytes, &Settings::default(), false).unwrap();
    assert_eq!(imported.settings, Settings::default());
    assert_eq!(imported.report.invalid_keys.len(), 7);
    assert_eq!(imported.report.invalid_entries, 0);
    assert!(imported.report.imported_keys.is_empty());
}

#[test]
fn array_bounds_are_enforced_and_duplicates_are_not_repeated() {
    let apps = (0..258)
        .map(|i| Value::String(format!("com.example.app{i}")))
        .collect();
    let types = (0..130)
        .map(|i| Value::String(format!("com.example.type{i}")))
        .collect();
    let patterns = (0..34)
        .map(|i| Value::String(format!("^pattern{i}$")))
        .collect();
    let mut enabled: Vec<_> = SUPPORTED_TYPES
        .iter()
        .map(|s| Value::String((*s).into()))
        .collect();
    enabled.push(Value::String(TEXT.into()));
    let bytes = fixture(
        &[
            ("ignoredApps", Value::Array(apps)),
            ("ignoredPasteboardTypes", Value::Array(types)),
            ("ignoreRegexp", Value::Array(patterns)),
            ("enabledPasteboardTypes", Value::Array(enabled)),
        ],
        true,
    );
    let imported = import_settings(&bytes, &Settings::default(), false).unwrap();
    assert_eq!(imported.settings.ignored_apps.len(), 256);
    assert_eq!(imported.settings.ignored_types.len(), 128);
    assert_eq!(imported.settings.ignored_patterns.len(), 32);
    assert_eq!(imported.settings.enabled_types.len(), SUPPORTED_TYPES.len());
    assert_eq!(imported.report.invalid_entries, 7);
    imported.settings.validate().unwrap();
    let duplicate = fixture(
        &[(
            "ignoredApps",
            array(&["com.example.app", "com.example.app"]),
        )],
        false,
    );
    let imported = import_settings(&duplicate, &Settings::default(), false).unwrap();
    assert_eq!(imported.settings.ignored_apps, ["com.example.app"]);
    assert_eq!(imported.report.invalid_entries, 0);
}

#[test]
fn app_validation_and_allowlist_dependency_are_safe() {
    for apps in [
        None,
        Some(array(&[])),
        Some(array(&["bad/app", "bad\napp", ""])),
    ] {
        let mut entries = vec![("ignoreAllAppsExceptListed", Value::Boolean(true))];
        if let Some(apps) = apps {
            entries.push(("ignoredApps", apps));
        }
        let imported =
            import_settings(&fixture(&entries, false), &Settings::default(), false).unwrap();
        assert!(!imported.settings.allowlist_apps);
        assert!(has(
            &imported.report.invalid_keys,
            "ignoreAllAppsExceptListed"
        ));
        assert!(
            imported.settings.import_digest.is_none()
                || has(&imported.report.imported_keys, "ignoredApps")
        );
    }
    let bytes = fixture(
        &[
            ("ignoreAllAppsExceptListed", Value::Boolean(true)),
            ("ignoredApps", array(&["com.example.valid", "bad/app"])),
        ],
        true,
    );
    let imported = import_settings(&bytes, &Settings::default(), false).unwrap();
    assert!(imported.settings.allowlist_apps);
    assert_eq!(imported.settings.ignored_apps, ["com.example.valid"]);
    assert_eq!(imported.report.invalid_entries, 1);
    assert_eq!(imported.report.invalid_keys, ["ignoredApps"]);

    let current = Settings {
        ignored_apps: vec!["com.example.current".into()],
        ..Settings::default()
    };
    let imported = import_settings(&bytes, &current, false).unwrap();
    assert_eq!(imported.settings, current);
    assert!(has(&imported.report.skipped_keys, "ignoredApps"));
    assert!(has(
        &imported.report.skipped_keys,
        "ignoreAllAppsExceptListed"
    ));
}

#[test]
fn unsupported_search_modes_are_not_mapped_to_literal_search() {
    for mode in ["fuzzy", "regexp", "mixed", "unknown"] {
        let bytes = fixture(&[("searchMode", Value::String(mode.into()))], false);
        let imported = import_settings(&bytes, &Settings::default(), false).unwrap();
        assert_eq!(imported.settings, Settings::default());
        assert_eq!(imported.report.skipped_keys, ["searchMode"]);
        assert!(imported.report.imported_keys.is_empty());
    }
    let bytes = fixture(&[("searchMode", Value::String("exact".into()))], false);
    let imported = import_settings(&bytes, &Settings::default(), false).unwrap();
    assert_eq!(imported.report.imported_keys, ["searchMode"]);
    assert!(imported.settings.import_digest.is_some());
}

#[test]
fn same_digest_is_a_noop_and_explicit_overwrite_reimports() {
    let bytes = fixture(&[("clipboardCheckInterval", Value::Real(0.9))], false);
    let first = import_settings(&bytes, &Settings::default(), false).unwrap();
    let mut changed = first.settings.clone();
    changed.poll_interval_ms = 1700;
    let second = import_settings(&bytes, &changed, false).unwrap();
    assert!(second.report.already_imported);
    assert_eq!(second.settings, changed);
    assert!(second.report.imported_keys.is_empty());
    assert_eq!(second.report.skipped_keys, ["clipboardCheckInterval"]);
    let explicit = import_settings(&bytes, &changed, true).unwrap();
    assert!(!explicit.report.already_imported);
    assert_eq!(explicit.settings.poll_interval_ms, 900);
    assert_eq!(
        explicit.settings.import_digest,
        first.settings.import_digest
    );
}

#[test]
fn every_changed_supported_preference_is_protected_unless_overwritten() {
    let current = Settings {
        enabled_types: Vec::new(),
        ignored_apps: vec!["com.example.current".into()],
        allowlist_apps: true,
        ignored_patterns: vec!["current".into()],
        ignored_types: vec!["com.example.currentType".into()],
        poll_interval_ms: 1500,
        ..Settings::default()
    };
    let bytes = fixture(
        &[
            ("enabledPasteboardTypes", array(&[TEXT])),
            ("ignoredApps", array(&["com.example.legacy"])),
            ("ignoreAllAppsExceptListed", Value::Boolean(false)),
            ("ignoreRegexp", array(&["legacy"])),
            ("ignoredPasteboardTypes", array(&[])),
            ("clipboardCheckInterval", Value::Real(0.2)),
        ],
        false,
    );
    let protected = import_settings(&bytes, &current, false).unwrap();
    assert_eq!(protected.settings, current);
    assert_eq!(protected.report.skipped_keys.len(), 6);
    assert!(protected.report.imported_keys.is_empty());
    let explicit = import_settings(&bytes, &current, true).unwrap();
    assert_eq!(explicit.report.imported_keys.len(), 6);
    assert_eq!(explicit.settings.enabled_types, [TEXT]);
    assert_eq!(explicit.settings.ignored_apps, ["com.example.legacy"]);
    assert!(!explicit.settings.allowlist_apps);
    assert_eq!(explicit.settings.ignored_patterns, ["legacy"]);
    assert!(explicit.settings.ignored_types.is_empty());
    assert_eq!(explicit.settings.poll_interval_ms, 200);
}

#[test]
fn unchanged_fields_can_import_while_changed_fields_stay_protected() {
    let current = Settings {
        poll_interval_ms: 1800,
        ..Settings::default()
    };
    let bytes = fixture(
        &[
            ("clipboardCheckInterval", Value::Real(0.2)),
            ("ignoredApps", array(&["com.example.legacy"])),
        ],
        false,
    );
    let imported = import_settings(&bytes, &current, false).unwrap();
    assert_eq!(imported.settings.poll_interval_ms, 1800);
    assert_eq!(imported.settings.ignored_apps, ["com.example.legacy"]);
    assert_eq!(imported.report.imported_keys, ["ignoredApps"]);
    assert_eq!(imported.report.skipped_keys, ["clipboardCheckInterval"]);
}

#[test]
fn validated_temporary_settings_apply_atomically_to_only_a_synthetic_store() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("new-store");
    let mut store = Store::open(&root, None, 100).unwrap();
    let original = store.settings().clone();
    let bytes = fixture(
        &[
            ("ignoredApps", array(&["com.example.synthetic"])),
            ("clipboardCheckInterval", Value::Real(0.8)),
        ],
        true,
    );
    let imported = import_settings(&bytes, store.settings(), false).unwrap();
    assert_eq!(store.settings(), &original);
    assert_eq!(store.count().unwrap(), 0);
    assert_eq!(
        import_settings(b"malformed", store.settings(), false),
        Err(CoreError::InvalidInput)
    );
    assert_eq!(store.settings(), &original);
    store
        .update_settings(imported.settings.clone(), 100)
        .unwrap();
    assert_eq!(store.settings(), &imported.settings);
    let mut invalid = imported.settings.clone();
    invalid.poll_interval_ms = 0;
    assert_eq!(
        store.update_settings(invalid, 100),
        Err(CoreError::InvalidInput)
    );
    assert_eq!(store.settings(), &imported.settings);
    drop(store);
    let reopened = Store::open(&root, None, 101).unwrap();
    assert_eq!(reopened.settings(), &imported.settings);
    assert_eq!(reopened.count().unwrap(), 0);
    assert!(
        import_settings(&bytes, reopened.settings(), false)
            .unwrap()
            .report
            .already_imported
    );
}

#[test]
fn importer_has_only_a_bytes_entry_point_and_no_source_path_parameter() {
    let importer: fn(&[u8], &Settings, bool) -> maccy_core::Result<ImportResult> = import_settings;
    let bytes = fixture(
        &[(
            "storagePath",
            Value::String("/synthetic/nonexistent/history.sqlite".into()),
        )],
        true,
    );
    let result = importer(&bytes, &Settings::default(), false).unwrap();
    assert_eq!(result.settings, Settings::default());
    assert_eq!(result.report.skipped_keys, ["storagePath"]);
}
