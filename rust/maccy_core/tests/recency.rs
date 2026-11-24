use maccy_core::models::{ClipboardPayload, Representation, TEXT};
use maccy_core::repository::Store;

fn copy(text: &str) -> ClipboardPayload {
    ClipboardPayload {
        representations: vec![Representation {
            item_index: 0,
            uti: TEXT.into(),
            data: text.as_bytes().to_vec(),
        }],
        ..Default::default()
    }
}

#[test]
fn duplicate_refresh_reorders_even_when_clock_has_not_advanced() {
    let root = tempfile::tempdir().unwrap();
    let mut store = Store::open(root.path(), None, 10).unwrap();
    let first = store.ingest(copy("first"), 10).unwrap().unwrap();
    let second = store.ingest(copy("second"), 10).unwrap().unwrap();
    assert_eq!(store.page("", None, 50).unwrap().items[0].id, second);
    assert_eq!(store.ingest(copy("first"), 10).unwrap(), Some(first));
    let items = store.page("", None, 50).unwrap().items;
    assert_eq!(items[0].id, first);
    assert_eq!(items[0].last_copied_at, 10);
    assert_eq!(items[0].copy_count, 2);
    assert_eq!(items[1].id, second);
    assert_eq!(store.count().unwrap(), 2);
    drop(store);
    let store = Store::open(root.path(), None, 10).unwrap();
    assert_eq!(store.page("", None, 50).unwrap().items[0].id, first);
}
