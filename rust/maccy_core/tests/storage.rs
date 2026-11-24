use maccy_core::CoreError;
use maccy_core::models::*;
use maccy_core::repository::Store;
use maccy_core::settings::{SAFETY_TYPES, Settings};
use rusqlite::{Connection, params};
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tempfile::TempDir;

const WEEK: i64 = 7 * 86_400;

fn rep(index: u32, uti: &str, bytes: &[u8]) -> Representation {
    Representation {
        item_index: index,
        uti: uti.into(),
        data: bytes.to_vec(),
    }
}
fn text(value: &str) -> ClipboardPayload {
    ClipboardPayload {
        representations: vec![rep(0, TEXT, value.as_bytes())],
        ..Default::default()
    }
}
fn image(value: u8) -> ClipboardPayload {
    // Synthetic opaque bytes: this repository does not decode or validate image pixels.
    ClipboardPayload {
        representations: vec![rep(0, "public.png", &[value, 2, 3, 4])],
        ..Default::default()
    }
}
fn files() -> ClipboardPayload {
    ClipboardPayload {
        representations: vec![
            rep(7, FILE_URL, b"file:///not-a-real-source/b.txt"),
            rep(2, FILE_URL, b"file:///not-a-real-source/a.txt"),
        ],
        ..Default::default()
    }
}
fn open() -> (TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path(), None, 0).unwrap();
    (dir, store)
}
fn insert(store: &mut Store, value: &str, now: i64) -> i64 {
    store.ingest(text(value), now).unwrap().unwrap()
}
fn blobs(root: &Path) -> Vec<PathBuf> {
    fs::read_dir(root.join("blobs"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect()
}
fn blob(root: &Path, bytes: &[u8]) -> PathBuf {
    root.join("blobs")
        .join(format!("{}.blob", blake3::hash(bytes).to_hex()))
}
fn ids(store: &Store) -> Vec<i64> {
    let mut cursor = None;
    let mut all = Vec::new();
    loop {
        let page = store.page("", cursor, 50).unwrap();
        all.extend(page.items.into_iter().map(|r| r.id));
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    all
}
fn db(root: &Path) -> Connection {
    Connection::open(root.join("history.sqlite3")).unwrap()
}

#[test]
fn default_200_then_201_evicts_oldest_and_keeps_newest() {
    let (_dir, mut store) = open();
    let mut inserted = Vec::new();
    for i in 0..200 {
        inserted.push(insert(&mut store, &format!("entry-{i}"), i));
    }
    assert_eq!(store.settings().max_items, 200);
    assert_eq!(store.count().unwrap(), 200);
    assert!(store.payload(inserted[0]).is_ok());
    let newest = insert(&mut store, "entry-200", 200);
    assert_eq!(store.count().unwrap(), 200);
    assert!(matches!(
        store.payload(inserted[0]),
        Err(CoreError::MissingContent)
    ));
    assert_eq!(ids(&store).first(), Some(&newest));
    assert_eq!(ids(&store).last(), Some(&inserted[1]));
}

#[test]
fn duplicate_at_cap_refreshes_count_recency_without_unrelated_eviction() {
    let (_dir, mut store) = open();
    for i in 0..200 {
        insert(&mut store, &format!("entry-{i}"), i);
    }
    let before: HashSet<_> = ids(&store).into_iter().collect();
    let original = store.page("entry-0", None, 50).unwrap().items[0].id;
    assert_eq!(insert(&mut store, "entry-0", 500), original);
    assert_eq!(store.count().unwrap(), 200);
    assert_eq!(before, ids(&store).into_iter().collect());
    let first = &store.page("", None, 50).unwrap().items[0];
    assert_eq!(first.id, original);
    assert_eq!(first.copy_count, 2);
    assert_eq!(first.created_at, 0);
    assert_eq!(first.last_copied_at, 500);
}

#[test]
fn richer_distinct_canonical_order_metadata_ignored_and_raw_bytes_exact() {
    let (_dir, mut store) = open();
    let raw = " \r\nHello\u{fffc} 세계\r\n ";
    let plain = insert(&mut store, raw, 0);
    let mut rich = text(raw);
    rich.text = "native fallback".into();
    rich.representations.push(rep(0, HTML, b"<b>Hello</b>"));
    rich.representations.push(rep(0, RTF, b"{\\rtf1 Hello}"));
    let rich_id = store.ingest(rich.clone(), 1).unwrap().unwrap();
    assert_ne!(plain, rich_id);
    rich.representations.reverse();
    rich.source_app = Some("synthetic.other".into());
    rich.advertised_types = vec!["synthetic.unknown-metadata".into()];
    assert_eq!(store.ingest(rich, 2).unwrap(), Some(rich_id));
    let restored = store.payload(rich_id).unwrap();
    assert_eq!(restored.representations.len(), 3);
    assert_eq!(
        restored
            .representations
            .iter()
            .find(|r| r.uti == TEXT)
            .unwrap()
            .data,
        raw.as_bytes()
    );
    assert_eq!(restored.text, "native fallback");
    assert!(store.payload(plain).unwrap().representations == text(raw).representations);
    assert_eq!(store.count().unwrap(), 2);
}

#[test]
fn preview_is_300_scalars_without_object_character_payload_untouched() {
    let (_dir, mut store) = open();
    let raw = format!("\u{fffc}{}\r\n", "한😀".repeat(180));
    let id = insert(&mut store, &raw, 0);
    let preview = &store.page("", None, 50).unwrap().items[0].preview;
    assert_eq!(preview.chars().count(), 300);
    assert!(!preview.contains('\u{fffc}'));
    assert_eq!(
        store.payload(id).unwrap().representations[0].data,
        raw.as_bytes()
    );
    assert!(store.ingest(text(" \r\n\t"), 1).unwrap().is_none());
}

#[test]
fn seven_day_before_exact_after_applies_to_all_kinds() {
    for payload in [text("old text"), image(1), files()] {
        let (_dir, mut store) = open();
        let id = store.ingest(payload, 0).unwrap().unwrap();
        assert_eq!(store.enforce(WEEK - 1).unwrap().deleted_items, 0);
        assert!(store.payload(id).is_ok());
        assert_eq!(store.enforce(WEEK).unwrap().deleted_items, 0);
        assert!(store.payload(id).is_ok());
        assert_eq!(store.enforce(WEEK + 1).unwrap().deleted_items, 1);
        assert_eq!(store.count().unwrap(), 0);
        assert_eq!(store.enforce(WEEK + 1).unwrap().deleted_items, 0);
    }
}

#[test]
fn startup_expiry_settings_persist_and_enforce_immediately() {
    let (dir, mut store) = open();
    insert(&mut store, "old", 0);
    insert(&mut store, "new", WEEK);
    drop(store);
    let mut store = Store::open(dir.path(), None, WEEK + 1).unwrap();
    assert_eq!(store.count().unwrap(), 1);
    assert_eq!(store.page("", None, 50).unwrap().items[0].preview, "new");
    insert(&mut store, "other", WEEK + 2);
    let settings = Settings {
        max_items: 1,
        retention_days: 1,
        ..Settings::default()
    };
    store.update_settings(settings.clone(), WEEK + 2).unwrap();
    assert_eq!(store.count().unwrap(), 1);
    drop(store);
    let store = Store::open(dir.path(), Some(Settings::default()), WEEK + 2).unwrap();
    assert_eq!(store.settings(), &settings);
    assert_eq!(store.page("", None, 50).unwrap().items[0].preview, "other");
    drop(store);
    let store = Store::open(dir.path(), None, WEEK + 2 + 86_401).unwrap();
    assert_eq!(store.count().unwrap(), 0);
}

#[test]
fn settings_invalid_or_failed_retention_are_atomic_in_memory_and_on_disk() {
    let (dir, mut store) = open();
    insert(&mut store, "one", 0);
    insert(&mut store, "two", 1);
    let original = store.settings().clone();
    for settings in [
        Settings {
            max_items: 0,
            ..original.clone()
        },
        Settings {
            ignored_patterns: vec!["[".into()],
            ..original.clone()
        },
        Settings {
            retention_days: 0,
            ..original.clone()
        },
    ] {
        assert_eq!(
            store.update_settings(settings, 1),
            Err(CoreError::InvalidInput)
        );
        assert_eq!(store.settings(), &original);
        assert_eq!(store.count().unwrap(), 2);
    }
    let conn = db(dir.path());
    conn.execute_batch("CREATE TRIGGER reject_retention BEFORE DELETE ON items BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    assert!(
        store
            .update_settings(
                Settings {
                    max_items: 1,
                    ..original.clone()
                },
                1
            )
            .is_err()
    );
    assert_eq!(store.settings(), &original);
    assert_eq!(store.count().unwrap(), 2);
    let serialized: String = conn
        .query_row("SELECT value FROM settings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Settings>(&serialized).unwrap(),
        original
    );
    conn.execute_batch("DROP TRIGGER reject_retention;")
        .unwrap();
    drop(store);
    assert_eq!(
        Store::open(dir.path(), None, 1).unwrap().settings(),
        &original
    );
}

#[test]
fn stable_keyset_pages_cover_all_rows_ties_cap_and_search_beyond_first_50() {
    let (_dir, mut store) = open();
    let oldest = insert(&mut store, "oldest needle", 42);
    for i in 0..129 {
        insert(&mut store, &format!("record-{i}"), 42);
    }
    assert_eq!(store.page("", None, usize::MAX).unwrap().items.len(), 50);
    assert!(store.page("", None, 0).unwrap().items.is_empty());
    let all = ids(&store);
    assert_eq!(all.len(), 130);
    assert_eq!(all.iter().copied().collect::<HashSet<_>>().len(), 130);
    assert!(all.windows(2).all(|v| v[0] > v[1]));
    assert_eq!(all.last(), Some(&oldest));
    assert_eq!(store.page("needle", None, 50).unwrap().items[0].id, oldest);
    let first = store.page("record", None, 11).unwrap();
    let second = store.page("record", first.next_cursor, 11).unwrap();
    assert_eq!(first.items.len(), 11);
    assert_eq!(second.items.len(), 11);
    assert!(
        first
            .items
            .iter()
            .all(|a| second.items.iter().all(|b| a.id != b.id))
    );
}

#[test]
fn unicode_and_fts_syntax_are_literal_substring_not_query_language() {
    let (_dir, mut store) = open();
    let sample = "HELLO 한글검색 안녕 😀🦀 100% _x_ a\\b \"quoted\" OR NOT * [] (x) : - İΣ";
    let id = insert(&mut store, sample, 0);
    insert(&mut store, "unrelated 1000 axb", 1);
    for query in [
        "hello",
        "한",
        "한글",
        "한글검색",
        "😀",
        "😀🦀",
        "%",
        "_",
        "\\",
        "100%",
        "_x_",
        "a\\b",
        "\"quoted\"",
        "OR NOT",
        "* []",
        "(x)",
        "İΣ",
    ] {
        let found = store.page(query, None, 50).unwrap();
        assert_eq!(found.items.len(), 1, "query {query:?}");
        assert_eq!(found.items[0].id, id, "query {query:?}");
    }
    assert!(
        store
            .page("%doesnotexist_", None, 50)
            .unwrap()
            .items
            .is_empty()
    );
    store.delete(id).unwrap();
    assert!(store.page("한글검색", None, 50).unwrap().items.is_empty());
    store.clear().unwrap();
    assert!(store.page("unrelated", None, 50).unwrap().items.is_empty());
}

#[test]
fn metadata_pages_ignore_missing_and_corrupt_originals_restore_is_typed_error() {
    for corrupt in [false, true] {
        let (dir, mut store) = open();
        let payload = image(17);
        let path = blob(dir.path(), &payload.representations[0].data);
        let id = store.ingest(payload, 0).unwrap().unwrap();
        if corrupt {
            fs::write(&path, b"bad!").unwrap();
        } else {
            fs::remove_file(&path).unwrap();
        }
        assert_eq!(store.page("", None, 50).unwrap().items[0].id, id);
        assert!(matches!(store.payload(id), Err(CoreError::MissingContent)));
        drop(store);
        let store = Store::open(dir.path(), None, 1).unwrap();
        assert_eq!(store.page("", None, 50).unwrap().items[0].id, id);
        assert!(matches!(store.payload(id), Err(CoreError::MissingContent)));
    }
}

#[test]
fn shared_blobs_delete_clear_and_restart_accounting() {
    let (dir, mut store) = open();
    let shared = b"shared bytes";
    let a = store
        .ingest(
            ClipboardPayload {
                representations: vec![rep(0, TEXT, shared)],
                ..Default::default()
            },
            0,
        )
        .unwrap()
        .unwrap();
    let b = store
        .ingest(
            ClipboardPayload {
                representations: vec![rep(0, TEXT, shared), rep(0, HTML, b"<b>shared bytes</b>")],
                ..Default::default()
            },
            1,
        )
        .unwrap()
        .unwrap();
    assert_eq!(blobs(dir.path()).len(), 2);
    store.delete(a).unwrap();
    assert_eq!(blobs(dir.path()).len(), 2);
    assert!(store.payload(b).is_ok());
    drop(store);
    let mut store = Store::open(dir.path(), None, 2).unwrap();
    assert!(store.payload(b).is_ok());
    store.clear().unwrap();
    assert_eq!(store.count().unwrap(), 0);
    assert!(blobs(dir.path()).is_empty());
    assert_eq!(
        db(dir.path())
            .query_row("SELECT count(*) FROM blobs", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    store.clear().unwrap();
    drop(store);
    assert_eq!(
        Store::open(dir.path(), None, 3).unwrap().count().unwrap(),
        0
    );
}

#[test]
fn file_urls_preserve_indices_order_and_never_touch_source_files() {
    let source = tempfile::tempdir().unwrap();
    let a = source.path().join("a.txt");
    let b = source.path().join("b.txt");
    fs::write(&a, b"source a").unwrap();
    fs::write(&b, b"source b").unwrap();
    let (_dir, mut store) = open();
    let payload = ClipboardPayload {
        representations: vec![
            rep(12, FILE_URL, format!("file://{}", b.display()).as_bytes()),
            rep(3, FILE_URL, format!("file://{}", a.display()).as_bytes()),
        ],
        ..Default::default()
    };
    let id = store.ingest(payload.clone(), 0).unwrap().unwrap();
    let restored = store.payload(id).unwrap();
    assert_eq!(restored.representations[0].item_index, 3);
    assert_eq!(restored.representations[1].item_index, 12);
    assert_eq!(
        restored.representations[0].data,
        payload.representations[1].data
    );
    assert!(store.page("a.txt", None, 50).unwrap().items.len() == 1);
    store.clear().unwrap();
    assert_eq!(fs::read(a).unwrap(), b"source a");
    assert_eq!(fs::read(b).unwrap(), b"source b");
}

#[test]
fn percent_encoded_file_name_is_searchable_without_changing_the_url() {
    let (_dir, mut store) = open();
    let url = b"file:///synthetic/%ED%95%9C%EA%B8%80%20file.txt";
    let id = store
        .ingest(
            ClipboardPayload {
                representations: vec![rep(0, FILE_URL, url)],
                ..Default::default()
            },
            0,
        )
        .unwrap()
        .unwrap();
    let page = store.page("\u{d55c}\u{ae00} file", None, 50).unwrap();
    assert_eq!(page.items[0].id, id);
    assert_eq!(store.payload(id).unwrap().representations[0].data, url);
}

#[test]
fn sqlite_abort_after_blob_staging_rolls_back_and_preserves_previous_history() {
    let (dir, mut store) = open();
    let settings = Settings {
        max_items: 1,
        ..Settings::default()
    };
    store.update_settings(settings, 0).unwrap();
    let old = insert(&mut store, "committed", 0);
    let conn = db(dir.path());
    conn.execute_batch("CREATE TRIGGER reject_rep BEFORE INSERT ON representations BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    assert!(store.ingest(image(44), 1).is_err());
    assert_eq!(store.count().unwrap(), 1);
    assert_eq!(store.page("", None, 50).unwrap().items[0].id, old);
    assert!(store.payload(old).is_ok());
    assert!(blob(dir.path(), &[44, 2, 3, 4]).exists());
    conn.execute_batch("DROP TRIGGER reject_rep;").unwrap();
    drop(store);
    let mut store = Store::open(dir.path(), None, 1).unwrap();
    assert!(store.payload(old).is_ok());
    assert!(store.ingest(image(44), 2).unwrap().is_some());
    assert_eq!(store.count().unwrap(), 1);
    assert!(matches!(store.payload(old), Err(CoreError::MissingContent)));
}

#[test]
fn sqlite_writer_lock_fails_without_eviction_or_blob_write() {
    let (dir, mut store) = open();
    let old = insert(&mut store, "committed", 0);
    let before = blobs(dir.path());
    let conn = db(dir.path());
    conn.execute_batch("BEGIN IMMEDIATE;").unwrap();
    assert!(store.ingest(image(51), 1).is_err());
    assert_eq!(store.count().unwrap(), 1);
    assert!(store.payload(old).is_ok());
    assert_eq!(blobs(dir.path()), before);
    conn.execute_batch("ROLLBACK;").unwrap();
    assert!(store.ingest(image(51), 1).unwrap().is_some());
}

#[test]
fn failed_blob_write_rolls_back_old_rows_and_backward_clock_keeps_new_unique() {
    let (dir, mut store) = open();
    store
        .update_settings(
            Settings {
                max_items: 1,
                ..Settings::default()
            },
            0,
        )
        .unwrap();
    let old = insert(&mut store, "old", 100);
    fs::rename(dir.path().join("blobs"), dir.path().join("saved-blobs")).unwrap();
    fs::write(dir.path().join("blobs"), b"not a directory").unwrap();
    assert!(store.ingest(image(4), 101).is_err());
    assert_eq!(store.count().unwrap(), 1);
    assert_eq!(store.page("", None, 50).unwrap().items[0].id, old);
    fs::remove_file(dir.path().join("blobs")).unwrap();
    fs::rename(dir.path().join("saved-blobs"), dir.path().join("blobs")).unwrap();
    let new = insert(&mut store, "new with backward clock", 1);
    assert_eq!(store.count().unwrap(), 1);
    assert!(store.payload(new).is_ok());
    assert_eq!(
        store.page("", None, 50).unwrap().items[0].last_copied_at,
        100
    );
    assert!(matches!(store.payload(old), Err(CoreError::MissingContent)));
}

#[test]
fn privacy_rejections_precede_every_blob_write() {
    let (dir, mut store) = open();
    for marker in SAFETY_TYPES {
        let mut payload = image(4);
        payload.advertised_types.push((*marker).into());
        assert!(store.ingest(payload, 0).unwrap().is_none());
        let mut payload = image(4);
        payload.representations.push(rep(0, marker, b"marker"));
        assert!(store.ingest(payload, 0).unwrap().is_none());
    }
    let settings = Settings {
        ignored_apps: vec!["synthetic.excluded".into()],
        ignored_types: vec!["synthetic.secret".into()],
        ignored_patterns: vec!["SECRET".into()],
        ..Settings::default()
    };
    store.update_settings(settings.clone(), 0).unwrap();
    let mut app = image(4);
    app.source_app = Some("synthetic.excluded".into());
    let mut kind = image(4);
    kind.advertised_types = vec!["synthetic.secret".into()];
    let mut native = image(4);
    native.text = "SECRET native".into();
    let mut url = files();
    url.representations[0].data = b"file:///SECRET.txt".to_vec();
    for payload in [app, kind, text("SECRET plain"), native, url] {
        assert!(store.ingest(payload, 0).unwrap().is_none());
    }
    store
        .update_settings(
            Settings {
                paused: true,
                ..settings.clone()
            },
            0,
        )
        .unwrap();
    assert!(store.ingest(image(4), 0).unwrap().is_none());
    store
        .update_settings(
            Settings {
                allowlist_apps: true,
                ..settings
            },
            0,
        )
        .unwrap();
    assert!(store.ingest(image(4), 0).unwrap().is_none());
    assert_eq!(store.count().unwrap(), 0);
    assert!(blobs(dir.path()).is_empty());
}

#[test]
fn unknown_disabled_oversize_malformed_and_ambiguous_rejected_without_blobs() {
    let (dir, mut store) = open();
    let mut malformed = text("valid");
    malformed.representations[0].data = vec![0xff];
    let ambiguous = ClipboardPayload {
        representations: vec![rep(0, TEXT, b"first"), rep(0, TEXT, b"second")],
        ..Default::default()
    };
    let unknown = ClipboardPayload {
        representations: vec![rep(0, "synthetic.unknown", b"bytes")],
        ..Default::default()
    };
    let too_many = ClipboardPayload {
        representations: (0..=MAX_REPRESENTATIONS)
            .map(|i| rep(i as u32, TEXT, b"x"))
            .collect(),
        ..Default::default()
    };
    let oversized = ClipboardPayload {
        representations: vec![rep(0, "public.png", &vec![1; MAX_PAYLOAD_BYTES + 1])],
        ..Default::default()
    };
    let mut empty = image(1);
    empty.representations[0].data.clear();
    let mut bad_url = files();
    bad_url.representations[0].data = b"file:///bad%xx".to_vec();
    let mut remote = files();
    remote.representations[0].data = b"https://example.invalid/a".to_vec();
    for payload in [
        malformed, ambiguous, unknown, too_many, oversized, empty, bad_url, remote,
    ] {
        assert!(store.ingest(payload, 0).unwrap().is_none());
    }
    store
        .update_settings(
            Settings {
                enabled_types: vec![TEXT.into()],
                ..Settings::default()
            },
            0,
        )
        .unwrap();
    assert!(store.ingest(image(1), 0).unwrap().is_none());
    assert!(blobs(dir.path()).is_empty());
    assert_eq!(store.count().unwrap(), 0);
    let mut mixed = text("keep supported");
    mixed
        .representations
        .push(rep(0, "synthetic.unknown", b"private"));
    let id = store.ingest(mixed, 1).unwrap().unwrap();
    assert_eq!(store.payload(id).unwrap().representations.len(), 1);
    assert_eq!(blobs(dir.path()).len(), 1);
}

#[test]
fn conservative_orphan_reconciliation_removes_only_old_derived_files() {
    let (dir, mut store) = open();
    let live = insert(&mut store, "live", 0);
    let orphan = blob(dir.path(), b"abandoned");
    fs::write(&orphan, b"abandoned").unwrap();
    let staged = dir.path().join("blobs/stage-abandoned");
    fs::write(&staged, b"abandoned").unwrap();
    let unrelated = dir.path().join("blobs/not-derived.txt");
    fs::write(&unrelated, b"unrelated").unwrap();
    let recent = blob(dir.path(), b"recent orphan");
    fs::write(&recent, b"recent orphan").unwrap();
    let old = SystemTime::now() - Duration::from_secs(7200);
    for path in [&orphan, &staged, &unrelated, &blob(dir.path(), b"live")] {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old))
            .unwrap();
    }
    drop(store);
    let store = Store::open(dir.path(), None, 1).unwrap();
    assert!(!orphan.exists());
    assert!(!staged.exists());
    assert!(unrelated.exists());
    assert!(recent.exists());
    assert!(store.payload(live).is_ok());
    drop(store);
    assert!(
        Store::open(dir.path(), None, 1)
            .unwrap()
            .payload(live)
            .is_ok()
    );
}

#[test]
fn private_permissions_and_symlink_guards() {
    let (dir, mut store) = open();
    let id = insert(&mut store, "private", 0);
    assert_eq!(
        fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(dir.path().join("history.sqlite3"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(blob(dir.path(), b"private"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let elsewhere = tempfile::tempdir().unwrap();
    let target = elsewhere.path().join("untouched");
    fs::write(&target, b"private").unwrap();
    fs::remove_file(blob(dir.path(), b"private")).unwrap();
    symlink(&target, blob(dir.path(), b"private")).unwrap();
    assert!(matches!(store.payload(id), Err(CoreError::MissingContent)));
    store.clear().unwrap();
    assert_eq!(fs::read(target).unwrap(), b"private");
    let redirected = tempfile::tempdir().unwrap();
    symlink(
        dir.path().join("history.sqlite3"),
        redirected.path().join("history.sqlite3"),
    )
    .unwrap();
    assert!(matches!(
        Store::open(redirected.path(), None, 0),
        Err(CoreError::Storage)
    ));
}

#[test]
fn sqlite_schema_foreign_keys_version_indexes_wal_and_fts_triggers() {
    let (dir, mut store) = open();
    insert(&mut store, "trigger search", 0);
    let conn = db(dir.path());
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        conn.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    let violations: i64 = conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);
    let indexes: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='index' AND name IN ('idx_items_recency','idx_items_expiry','idx_representations_blob')",[],|r|r.get(0)).unwrap();
    assert_eq!(indexes, 3);
    let fts: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='items_fts')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    if fts {
        conn.execute(
            "UPDATE items SET search_text=?1",
            params!["updated searchable"],
        )
        .unwrap();
        assert_eq!(store.page("searchable", None, 50).unwrap().items.len(), 1);
        assert!(store.page("trigger", None, 50).unwrap().items.is_empty());
    }
    store.clear().unwrap();
    assert!(store.page("searchable", None, 50).unwrap().items.is_empty());
}
