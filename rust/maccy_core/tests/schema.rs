use maccy_core::CoreError;
use maccy_core::models::{ClipboardPayload, Representation, TEXT};
use maccy_core::repository::Store;
use maccy_core::settings::Settings;
use rusqlite::Connection;

fn text(value: &str) -> ClipboardPayload {
    ClipboardPayload {
        representations: vec![Representation {
            item_index: 0,
            uti: TEXT.into(),
            data: value.as_bytes().to_vec(),
        }],
        ..Default::default()
    }
}

#[test]
fn v1_private_store_migrates_without_losing_original_bytes() {
    let root = tempfile::tempdir().unwrap();
    let mut store = Store::open(root.path(), None, 10).unwrap();
    let first = store.ingest(text("first"), 10).unwrap().unwrap();
    let second = store.ingest(text("second"), 10).unwrap().unwrap();
    drop(store);
    let db = Connection::open(root.path().join("history.sqlite3")).unwrap();
    db.execute_batch("DROP INDEX idx_items_recency; ALTER TABLE items DROP COLUMN recency_order; PRAGMA user_version=1;").unwrap();
    drop(db);
    let mut store = Store::open(root.path(), None, 10).unwrap();
    assert_eq!(store.count().unwrap(), 2);
    assert_eq!(
        store.payload(first).unwrap().representations[0].data,
        b"first"
    );
    assert_eq!(store.page("", None, 50).unwrap().items[0].id, second);
    assert_eq!(store.ingest(text("first"), 10).unwrap(), Some(first));
    assert_eq!(store.page("", None, 50).unwrap().items[0].id, first);
}

#[test]
fn capacity_evicts_oldest_copy_not_oldest_identity_at_same_timestamp() {
    let root = tempfile::tempdir().unwrap();
    let mut store = Store::open(
        root.path(),
        Some(Settings {
            max_items: 2,
            ..Default::default()
        }),
        10,
    )
    .unwrap();
    let first = store.ingest(text("first"), 10).unwrap().unwrap();
    let second = store.ingest(text("second"), 10).unwrap().unwrap();
    store.ingest(text("first"), 10).unwrap();
    let third = store.ingest(text("third"), 10).unwrap().unwrap();
    assert!(store.payload(first).is_ok());
    assert!(matches!(
        store.payload(second),
        Err(CoreError::MissingContent)
    ));
    let page = store.page("", None, 50).unwrap();
    assert_eq!(page.items[0].id, third);
    assert!(page.items.iter().all(|item| item.last_copied_at == 10));
    assert_eq!(store.enforce(10 + 7 * 86_400).unwrap().deleted_items, 0);
    assert_eq!(store.enforce(11 + 7 * 86_400).unwrap().deleted_items, 2);
}
