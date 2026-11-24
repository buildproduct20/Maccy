use crate::{CoreError, Result};
use std::sync::OnceLock;

#[repr(C)]
pub struct NativeOutput {
    pub data: *mut u8,
    pub length: usize,
    pub width: u32,
    pub height: u32,
}

pub type Process = unsafe extern "C" fn(u32, *const u8, usize, u32, *mut NativeOutput) -> i32;
pub type Release = unsafe extern "C" fn(*mut u8, usize);
struct Callbacks {
    process: Process,
    release: Release,
}
static CALLBACKS: OnceLock<Callbacks> = OnceLock::new();

/// Register process-lifetime native functions, not Swift objects or closures.
pub fn register(process: Process, release: Release) -> Result<()> {
    CALLBACKS
        .set(Callbacks { process, release })
        .map_err(|_| CoreError::Busy)
}

pub struct Converted {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

fn convert(operation: u32, bytes: &[u8], max_pixels: u32) -> Result<Converted> {
    if bytes.len() > crate::models::MAX_PAYLOAD_BYTES {
        return Err(CoreError::InvalidInput);
    }
    let callbacks = CALLBACKS.get().ok_or(CoreError::Unsupported)?;
    let mut output = NativeOutput {
        data: std::ptr::null_mut(),
        length: 0,
        width: 0,
        height: 0,
    };
    // Installed functions are compiled into this app and keep input borrowed only
    // for the call. Their output allocation is released by its matching allocator.
    let code = unsafe {
        (callbacks.process)(
            operation,
            bytes.as_ptr(),
            bytes.len(),
            max_pixels,
            &mut output,
        )
    };
    let result = if code != 0
        || output.length > 2 * 1024 * 1024
        || (output.length > 0 && output.data.is_null())
    {
        Err(CoreError::InvalidInput)
    } else {
        let copied = if output.length == 0 {
            Vec::new()
        } else {
            // A successful native function guarantees this many immutable bytes.
            unsafe { std::slice::from_raw_parts(output.data, output.length) }.to_vec()
        };
        Ok(Converted {
            bytes: copied,
            width: output.width,
            height: output.height,
        })
    };
    if !output.data.is_null() {
        unsafe { (callbacks.release)(output.data, output.length) };
    }
    result
}

pub fn rtf_text(bytes: &[u8]) -> Result<String> {
    String::from_utf8(convert(1, bytes, 0)?.bytes).map_err(|_| CoreError::InvalidInput)
}

pub fn image_info(bytes: &[u8]) -> Result<(u32, u32)> {
    let info = convert(2, bytes, 0)?;
    if info.width == 0
        || info.height == 0
        || info.width > 8192
        || info.height > 8192
        || u64::from(info.width) * u64::from(info.height) > 64_000_000
    {
        return Err(CoreError::InvalidInput);
    }
    Ok((info.width, info.height))
}

pub fn thumbnail(bytes: &[u8], max_pixels: u32) -> Result<Vec<u8>> {
    if !(16..=128).contains(&max_pixels) {
        return Err(CoreError::InvalidInput);
    }
    image_info(bytes)?;
    let image = convert(3, bytes, max_pixels)?;
    if image.width > max_pixels
        || image.height > max_pixels
        || !image.bytes.starts_with(b"\x89PNG\r\n\x1a\n")
    {
        return Err(CoreError::InvalidInput);
    }
    Ok(image.bytes)
}
