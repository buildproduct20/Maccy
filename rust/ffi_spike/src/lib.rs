//! Bounded native interoperability prototype, not the production history engine.
use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

const INVALID_HANDLE: i32 = 1;
const INVALID_BUFFER: i32 = 2;
const INVALID_PAYLOAD: i32 = 3;
const BUFFER_TOO_SMALL: i32 = 4;
const NATIVE_FAILURE: i32 = 5;
const PANIC: i32 = 255;
const MAX_PAYLOAD: usize = 1024 * 1024;
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);
static HANDLES: OnceLock<Mutex<HashSet<u64>>> = OnceLock::new();

fn handles() -> &'static Mutex<HashSet<u64>> {
    HANDLES.get_or_init(|| Mutex::new(HashSet::new()))
}

fn boundary(f: impl FnOnce() -> i32) -> i32 {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(PANIC)
}

#[unsafe(no_mangle)]
pub extern "C" fn ml_spike_open() -> u64 {
    catch_unwind(|| {
        let id = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        if id == 0 {
            return 0;
        }
        handles()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id);
        id
    })
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn ml_spike_close(handle: u64) -> i32 {
    boundary(|| {
        if handles()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&handle)
        {
            0
        } else {
            INVALID_HANDLE
        }
    })
}

/// Validate and echo a UTF-8 JSON string array into caller-owned storage.
///
/// # Safety
/// `input` must identify `length` readable bytes when length is nonzero.
/// `written` must be valid writable storage for one usize. A nonzero capacity
/// requires `output` to identify that many writable bytes, not overlapping input
/// or written. Buffers remain caller-owned and valid throughout this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ml_spike_roundtrip(
    handle: u64,
    input: *const u8,
    length: usize,
    output: *mut u8,
    capacity: usize,
    written: *mut usize,
) -> i32 {
    boundary(|| {
        if !handles()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&handle)
        {
            return INVALID_HANDLE;
        }
        if written.is_null() || input.is_null() || length > MAX_PAYLOAD {
            return INVALID_BUFFER;
        }
        // The C ABI contract guarantees the non-null input covers length bytes.
        let bytes = unsafe { std::slice::from_raw_parts(input, length) };
        if serde_json::from_slice::<Vec<String>>(bytes).is_err() {
            return INVALID_PAYLOAD;
        }
        // The caller owns written and, once capacity is sufficient, output.
        unsafe { written.write(length) };
        if capacity < length {
            return BUFFER_TOO_SMALL;
        }
        if output.is_null() {
            return INVALID_BUFFER;
        }
        unsafe { std::ptr::copy_nonoverlapping(input, output, length) };
        0
    })
}

#[cfg(target_os = "macos")]
fn named_pasteboard_roundtrip() -> i32 {
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::NSString;

    autoreleasepool(|_| {
        let pasteboard = NSPasteboard::pasteboardWithUniqueName();
        let kind = NSString::from_str("public.utf8-plain-text");
        let fixture = NSString::from_str("Maccy Lite named-pasteboard fixture");
        pasteboard.clearContents();
        let stored = pasteboard.setString_forType(&fixture, &kind);
        let restored = pasteboard.stringForType(&kind);
        // AppKit declares this as oneway void; objc2 0.3.2 skips its binding.
        // The SDK confirms a zero-argument void method on this owned named board.
        let _: () = unsafe { objc2::msg_send![&*pasteboard, releaseGlobally] };
        if stored && restored.as_deref() == Some(fixture.as_ref()) {
            0
        } else {
            NATIVE_FAILURE
        }
    })
}

/// Exercise a disposable named pasteboard; never access the general pasteboard.
#[unsafe(no_mangle)]
pub extern "C" fn ml_spike_named_pasteboard() -> i32 {
    boundary(|| {
        #[cfg(target_os = "macos")]
        {
            let caller_thread = named_pasteboard_roundtrip();
            if caller_thread != 0 {
                return caller_thread;
            }
            std::thread::spawn(named_pasteboard_roundtrip)
                .join()
                .unwrap_or(PANIC)
        }
        #[cfg(not(target_os = "macos"))]
        {
            NATIVE_FAILURE
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn array_roundtrip_uses_caller_owned_buffer() {
        let handle = ml_spike_open();
        let payload = r#"["text","line\nnext","🚀"]"#.as_bytes();
        let mut written = 0;
        let mut output = vec![0; payload.len()];
        assert_eq!(
            unsafe {
                ml_spike_roundtrip(
                    handle,
                    payload.as_ptr(),
                    payload.len(),
                    output.as_mut_ptr(),
                    output.len(),
                    &mut written,
                )
            },
            0
        );
        assert_eq!(written, payload.len());
        assert_eq!(output, payload);
        assert_eq!(ml_spike_close(handle), 0);
        assert_eq!(ml_spike_close(handle), INVALID_HANDLE);
    }

    #[test]
    fn short_buffer_and_invalid_payload_return_typed_errors() {
        let handle = ml_spike_open();
        let mut written = 0;
        assert_eq!(
            unsafe {
                ml_spike_roundtrip(
                    handle,
                    b"[]".as_ptr(),
                    2,
                    std::ptr::null_mut(),
                    0,
                    &mut written,
                )
            },
            BUFFER_TOO_SMALL
        );
        assert_eq!(written, 2);
        assert_eq!(
            unsafe {
                ml_spike_roundtrip(
                    handle,
                    b"{}".as_ptr(),
                    2,
                    std::ptr::null_mut(),
                    0,
                    &mut written,
                )
            },
            INVALID_PAYLOAD
        );
        assert_eq!(ml_spike_close(handle), 0);
    }

    #[test]
    fn stale_handles_and_null_buffers_are_rejected() {
        let handle = ml_spike_open();
        let mut written = 0;
        assert_eq!(
            unsafe {
                ml_spike_roundtrip(
                    handle,
                    std::ptr::null(),
                    1,
                    std::ptr::null_mut(),
                    0,
                    &mut written,
                )
            },
            INVALID_BUFFER
        );
        assert_eq!(ml_spike_close(handle), 0);
        assert_eq!(
            unsafe {
                ml_spike_roundtrip(
                    handle,
                    b"[]".as_ptr(),
                    2,
                    std::ptr::null_mut(),
                    0,
                    &mut written,
                )
            },
            INVALID_HANDLE
        );
    }

    #[test]
    fn panic_does_not_unwind_through_boundary() {
        assert_eq!(boundary(|| panic!("synthetic spike failure")), PANIC);
    }
}
