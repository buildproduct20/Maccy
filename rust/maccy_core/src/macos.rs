use crate::models::*;
use crate::settings::Settings;
use crate::{CoreError, Result};
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting, NSWorkspace};
use objc2_foundation::{NSArray, NSData, NSString};
use std::collections::BTreeMap;

pub struct Pasteboard {
    board: Retained<NSPasteboard>,
    named: bool,
}

impl Pasteboard {
    /// Construct and use on the same Rust owner thread. Named boards are test-only.
    pub fn open(name: Option<&str>) -> Result<Self> {
        if name.is_some_and(|s| s.is_empty() || s.len() > 255) {
            return Err(CoreError::InvalidInput);
        }
        let board = match name {
            Some(name) => NSPasteboard::pasteboardWithName(&NSString::from_str(name)),
            None => NSPasteboard::generalPasteboard(),
        };
        Ok(Self {
            board,
            named: name.is_some(),
        })
    }

    pub fn generation(&self) -> i64 {
        self.board.changeCount() as i64
    }

    pub fn capture(&self, settings: &Settings) -> Result<(i64, Option<ClipboardPayload>)> {
        for _ in 0..2 {
            let before = self.generation();
            let payload = autoreleasepool(|_| self.read_snapshot(settings));
            if before == self.generation() {
                return payload.map(|payload| (before, payload));
            }
        }
        Err(CoreError::Busy)
    }

    fn read_snapshot(&self, settings: &Settings) -> Result<Option<ClipboardPayload>> {
        // AppKit does not expose a verified writer for the general pasteboard.
        // The frontmost app is useful metadata, not a privacy authorization.
        if settings.paused
            || (!self.named && (settings.allowlist_apps || !settings.ignored_apps.is_empty()))
        {
            return Ok(None);
        }
        let Some(items) = self.board.pasteboardItems() else {
            return Ok(None);
        };
        if items.len() > MAX_REPRESENTATIONS {
            return Err(CoreError::InvalidInput);
        }
        let source_app = if self.named {
            Some("com.buildproduct20.maccylite.fixture".into())
        } else {
            NSWorkspace::sharedWorkspace()
                .frontmostApplication()
                .and_then(|app| app.bundleIdentifier())
                .map(|id| id.to_string())
        };
        let mut payload = ClipboardPayload {
            source_app,
            ..Default::default()
        };
        if let Some(types) = self.board.types() {
            if types.len() > 256 {
                return Err(CoreError::InvalidInput);
            }
            payload
                .advertised_types
                .extend(types.to_vec().into_iter().map(|t| t.to_string()));
        }
        let items = items.to_vec();
        for item in &items {
            let types = item.types();
            if types.len() > 256 {
                return Err(CoreError::InvalidInput);
            }
            payload
                .advertised_types
                .extend(types.to_vec().into_iter().map(|t| t.to_string()));
        }
        payload.advertised_types.sort();
        payload.advertised_types.dedup();
        if payload.advertised_types.len() > 256 {
            return Err(CoreError::InvalidInput);
        }
        // App/type/pause exclusion precedes payload reads. Pattern exclusion needs
        // actual text and runs after bounded extraction, before any persistence.
        let mut early = settings.clone();
        early.ignored_patterns.clear();
        if early.ignores(&payload)? {
            return Ok(None);
        }
        let mut total = 0usize;
        for (index, item) in items.iter().enumerate() {
            for kind in item.types().to_vec() {
                let uti = kind.to_string();
                let enabled = settings.enabled_types.contains(&uti);
                let inspect_text = uti == TEXT && !settings.ignored_patterns.is_empty();
                if !enabled && !inspect_text {
                    continue;
                }
                let Some(data) = item.dataForType(&kind) else {
                    if inspect_text {
                        return Ok(None);
                    }
                    continue;
                };
                total = total
                    .checked_add(data.len())
                    .ok_or(CoreError::InvalidInput)?;
                if total > MAX_PAYLOAD_BYTES || payload.representations.len() >= MAX_REPRESENTATIONS
                {
                    return Err(CoreError::InvalidInput);
                }
                let bytes = data.to_vec();
                if inspect_text
                    && settings.ignores_text(
                        std::str::from_utf8(&bytes).map_err(|_| CoreError::InvalidInput)?,
                    )?
                {
                    return Ok(None);
                }
                if enabled {
                    payload.representations.push(Representation {
                        item_index: index as u32,
                        uti,
                        data: bytes,
                    });
                }
            }
        }
        if payload.representations.is_empty() {
            return Ok(None);
        }
        payload.text = payload
            .representations
            .iter()
            .filter(|r| r.uti == TEXT)
            .filter_map(|r| std::str::from_utf8(&r.data).ok())
            .collect::<Vec<_>>()
            .join("\n");
        if payload.text.is_empty() {
            if let Some(rich) = payload.representations.iter().find(|r| r.uti == RTF) {
                payload.text = crate::native_callbacks::rtf_text(&rich.data)?;
            } else if let Some(html) = payload.representations.iter().find(|r| r.uti == HTML) {
                payload.text = crate::formats::html_text(&html.data)?;
            }
        }
        if settings.ignores(&payload)? {
            return Ok(None);
        }
        for image in payload
            .representations
            .iter()
            .filter(|r| IMAGE_TYPES.contains(&r.uti.as_str()))
        {
            crate::native_callbacks::image_info(&image.data)?;
        }
        Ok(Some(payload))
    }

    /// All bytes are materialized/validated before modifying the pasteboard.
    pub fn write(&self, payload: &ClipboardPayload) -> Result<i64> {
        autoreleasepool(|_| {
            if payload.representations.is_empty()
                || payload.representations.len() > MAX_REPRESENTATIONS
            {
                return Err(CoreError::InvalidInput);
            }
            let mut groups: BTreeMap<u32, Retained<NSPasteboardItem>> = BTreeMap::new();
            let mut total = 0usize;
            for rep in &payload.representations {
                total = total
                    .checked_add(rep.data.len())
                    .ok_or(CoreError::InvalidInput)?;
                if total > MAX_PAYLOAD_BYTES || !SUPPORTED_TYPES.contains(&rep.uti.as_str()) {
                    return Err(CoreError::InvalidInput);
                }
                let item = groups.entry(rep.item_index).or_default();
                if !item.setData_forType(
                    &NSData::with_bytes(&rep.data),
                    &NSString::from_str(&rep.uti),
                ) {
                    return Err(CoreError::InvalidInput);
                }
            }
            let objects: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = groups
                .into_values()
                .map(ProtocolObject::from_retained)
                .collect();
            let array = NSArray::from_retained_slice(&objects);
            self.board.clearContents();
            if !self.board.writeObjects(&array) {
                return Err(CoreError::Unsupported);
            }
            Ok(self.generation())
        })
    }
}
