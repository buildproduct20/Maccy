use maccy_core::models::{ClipboardPayload, Representation, TEXT};
use maccy_core::repository::Store;
use maccy_core::settings::Settings;
use std::hint::black_box;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let mut store = Store::open(
        dir.path(),
        Some(Settings {
            max_items: 10_000,
            ..Default::default()
        }),
        0,
    )?;
    for i in 0..10_000 {
        let text = format!(
            "synthetic clipboard row {i:05} catalog-{} \u{d55c}\u{ae00} \u{1f680}",
            i % 10
        );
        store.ingest(
            ClipboardPayload {
                representations: vec![Representation {
                    item_index: 0,
                    uti: TEXT.to_owned(),
                    data: text.into_bytes(),
                }],
                ..Default::default()
            },
            i,
        )?;
    }
    assert_eq!(store.count()?, 10_000);
    for (name, query, expected) in [
        ("rare-trigram", "row 03210", 1),
        ("common-trigram", "catalog-1", 50),
        ("short-unicode", "\u{d55c}\u{ae00}", 50),
    ] {
        for _ in 0..10 {
            assert_eq!(store.page(query, None, 50)?.items.len(), expected);
        }
        let mut microseconds = Vec::with_capacity(120);
        for _ in 0..120 {
            let start = Instant::now();
            let page = black_box(store.page(black_box(query), None, 50)?);
            microseconds.push(start.elapsed().as_secs_f64() * 1_000_000.0);
            assert_eq!(page.items.len(), expected);
        }
        microseconds.sort_by(f64::total_cmp);
        println!(
            "{name}: n=120 min={:.1}us median={:.1}us p95={:.1}us max={:.1}us",
            microseconds[0],
            (microseconds[59] + microseconds[60]) / 2.0,
            microseconds[113],
            microseconds[119]
        );
    }
    Ok(())
}
