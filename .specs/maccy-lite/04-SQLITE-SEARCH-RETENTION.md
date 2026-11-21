# SQLite model, search, retention and cleanup

## Authoritative invariants
- Rust SQLite is the **only** item history repository; no shadow SwiftData DB, no full-history in-memory array.
- Default `max_history_items=200`; bounded valid user input; all kinds count equally.
- Default `retention_days=7`; age uses `last_copied_at` on ALL kinds, including images and file references.
- On unique insertion when full, newest item retained, oldest evicted; count never remains > max after a successful operation.
- Duplicate capture refreshes `last_copied_at`/count and recency without evicting an unrelated item.
- Older than cutoff is `last_copied_at < now-7 days`; use explicit boundary tests.
- No pinned/favorite exemption exists.
- Search all **retained** items, regardless of screen page.

## Schema proposal — refine as needed based on FFI / actual formats
```sql
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
CREATE TABLE items (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL CHECK(kind IN ('text','rich','image','files')),
  digest BLOB NOT NULL UNIQUE,
  preview TEXT NOT NULL,
  search_text TEXT NOT NULL DEFAULT '',
  source_app TEXT,
  created_at INTEGER NOT NULL,
  last_copied_at INTEGER NOT NULL,
  copy_count INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE representations (
  item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  uti TEXT NOT NULL,
  content_inline BLOB,
  blob_digest BLOB,
  PRIMARY KEY(item_id, ordinal),
  CHECK((content_inline IS NOT NULL) != (blob_digest IS NOT NULL))
);
CREATE TABLE blobs (
  digest BLOB PRIMARY KEY,
  relative_path TEXT NOT NULL UNIQUE,
  size_bytes INTEGER NOT NULL,
  ref_count INTEGER NOT NULL CHECK(ref_count >= 0)
);
CREATE INDEX idx_items_recency ON items(last_copied_at DESC, id DESC);
CREATE INDEX idx_items_expiry ON items(last_copied_at);
CREATE INDEX idx_representations_blob ON representations(blob_digest);
-- FTS5 external-content or triggers may be used; ensure update and delete
-- consistency and test on macOS deployment target.
```
The above is a starting design. Prefer actual foreign keys + derived reference accounting if refcount becomes error-prone. Make schema migrations versioned. Use binding for SQL values and explicit WAL checkpoint policy. Do not simply enable FTS5 blindly: verify SQLite build supports trigram tokenizer; provide a compatible fallback.

## Search contracts
- `list(limit <= configured_page_cap, before_key=(last_copied_at,id)?)` uses keyset pagination and stable order.
- `search(query,limit,cursor)` uses FTS5 trigram for suitable substring queries with escaped syntax; for 1–2 Unicode scalar queries use correctly escaped LIKE fallback; query/cancel asynchronously with stale-result protection.
- `preview` contains bounded text (suggest 200–400 scalars) and metadata. Full representations are read only for restore.
- Query all retained rows; **no** artificial 50-row history cap that breaks old record search. UI page size 50 is rendering-only.
- Never return full image data in list results.

## Atomic insertion and eviction
1. Normalize/validate/filter incoming copy and compute canonical content digest before DB mutation.
2. Stage new blob bytes using uniquely named temp files in the app's private directory; fsync/rename as required by documented durability target.
3. Begin `IMMEDIATE` transaction, upsert by digest. On existing item, verify supported semantic equality, refresh recency/count; on unique item insert metadata + representations.
4. Enforce `last_copied_at < cutoff` AND count overage. Use indexed ordered deletion, removing as many rows as needed. Ensure newest newly inserted row survives when max>=1; protect against clock skew by sane timestamp policy.
5. Record unreferenced blob cleanup intents (or derive orphans), update references with transactional semantics.
6. Commit DB first. Only then unlink unreferenced files. Failed unlinks leave orphan files for startup reconciliation; never delete files still referenced by a committed row.
7. On DB failure roll back. Preserve old records and previously referenced blobs, retain clean-up-able staged blobs; don't claim capture was persisted.
8. After transaction send coalesced UI change notification. Avoid an unbounded task per captured item.

In words: user suggestion "when 200 is full and copy a new unique item, delete one old and insert one new" is *exactly* the desired logical behavior. Implement using an **insert-then-evict transaction** to avoid losing history on failed capture. Identical refresh does not increase count.

## Expiry schedule
- Startup: reconcile missing/orphan staged blobs, then enforce age and max count before presenting history.
- On each successful capture: enforce both policies.
- Periodic: e.g. every 1 hour while app running, to remove expired items even with no new captures.
- Settings change: enforce immediately (but only within app-owned DB). Reject invalid values without changing active settings.
- Cleanup should use indexed DB operations and batch unlink; no excessive `VACUUM` on every record. WAL and cache memory bounded; optional incremental vacuum/checkpoint based on measured disk fragmentation.
- Multiple app startups/cleanups are idempotent; no deletion of source files referenced by clipboard file URL entries.

## Crash and race correctness
Single DB owner / transaction queue; serialize capture, retention and delete. Use unique temporary blob files and startup orphan sweeper with grace period to avoid deleting in-flight writes. Treat malformed image, missing blob, locked SQLite, crash before/after commit, repeated duplicate, clock moved backward/forward, and OS shutdown as explicit cases.

## Required automated assertions
`D01` 200 distinct insert count=200; `D02` 201st oldest removed/newest present; `D03` duplicate at cap count stays 200 and recency updated; `D04` duplicate content with richer formats doesn't silently regress; `D05` age 7d minus epsilon retained, plus epsilon expired; `D06` image/text/file same expiry; `D07` startup cleanup; `D08` periodic cleanup; `D09` settings decreased cap; `D10` rollback on failed blob/DB write; `D11` no orphan/leaked image refs; `D12` 10k indexed search; `D13` Korean/emoji and 1–2 glyph search; `D14` stable page traversal; `D15` restart persistence and recovery; `D16` concurrency serialized.
