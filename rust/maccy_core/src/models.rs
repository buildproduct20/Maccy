use serde::{Deserialize, Serialize};

pub const TEXT: &str = "public.utf8-plain-text";
pub const HTML: &str = "public.html";
pub const RTF: &str = "public.rtf";
pub const FILE_URL: &str = "public.file-url";
pub const IMAGE_TYPES: &[&str] = &["public.png", "public.tiff", "public.jpeg", "public.heic"];
pub const SUPPORTED_TYPES: &[&str] = &[
    TEXT,
    HTML,
    RTF,
    FILE_URL,
    "public.png",
    "public.tiff",
    "public.jpeg",
    "public.heic",
];
pub const MAX_PAYLOAD_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_REPRESENTATIONS: usize = 128;
pub const PAGE_CAP: usize = 50;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Representation {
    pub item_index: u32,
    pub uti: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ClipboardPayload {
    pub representations: Vec<Representation>,
    pub advertised_types: Vec<String>,
    pub source_app: Option<String>,
    /// Readable native-format fallback, never a replacement for original bytes.
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Text,
    Rich,
    Image,
    Files,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ItemSummary {
    pub id: i64,
    pub kind: ItemKind,
    pub preview: String,
    pub source_app: Option<String>,
    pub created_at: i64,
    pub last_copied_at: i64,
    pub copy_count: u64,
    pub recency_order: i64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Cursor {
    pub last_copied_at: i64,
    pub recency_order: i64,
    pub id: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Page {
    pub items: Vec<ItemSummary>,
    pub next_cursor: Option<Cursor>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct CleanupReport {
    pub deleted_items: usize,
    pub deleted_blobs: usize,
}
