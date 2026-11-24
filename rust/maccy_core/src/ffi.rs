use crate::native_callbacks::{Process, Release};
use crate::worker::Worker;
use crate::{CoreError, Result};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

static WORKERS: OnceLock<Mutex<HashMap<u64, Arc<Worker>>>> = OnceLock::new();
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn workers() -> &'static Mutex<HashMap<u64, Arc<Worker>>> {
    WORKERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn boundary(f: impl FnOnce() -> Result<()>) -> i32 {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => match error {
            CoreError::InvalidInput => 1,
            CoreError::Storage => 2,
            CoreError::MissingContent => 3,
            CoreError::Unsupported => 4,
            CoreError::Busy => 5,
            CoreError::Closed => 6,
        },
        Err(_) => 255,
    }
}

#[repr(C)]
pub struct Buffer {
    pub data: *mut u8,
    pub length: usize,
}

unsafe fn bytes<'a>(data: *const u8, length: usize, max: usize) -> Result<&'a [u8]> {
    if length == 0 {
        return Ok(&[]);
    }
    if data.is_null() || length > max {
        return Err(CoreError::InvalidInput);
    }
    Ok(unsafe { std::slice::from_raw_parts(data, length) })
}

#[unsafe(no_mangle)]
pub extern "C" fn ml_register_native(process: Option<Process>, release: Option<Release>) -> i32 {
    boundary(|| {
        crate::native_callbacks::register(
            process.ok_or(CoreError::InvalidInput)?,
            release.ok_or(CoreError::InvalidInput)?,
        )
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ml_open(
    path: *const u8,
    length: usize,
    board: *const u8,
    board_length: usize,
    handle: *mut u64,
) -> i32 {
    boundary(|| {
        if handle.is_null() {
            return Err(CoreError::InvalidInput);
        }
        unsafe { handle.write(0) };
        let path = std::str::from_utf8(unsafe { bytes(path, length, 4096)? })
            .map_err(|_| CoreError::InvalidInput)?;
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(CoreError::InvalidInput);
        }
        let board = std::str::from_utf8(unsafe { bytes(board, board_length, 255)? })
            .map_err(|_| CoreError::InvalidInput)?;
        if !board.is_empty() && !board.starts_with("com.buildproduct20.maccylite.test.") {
            return Err(CoreError::InvalidInput);
        }
        let id = NEXT
            .try_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |n| n.checked_add(1),
            )
            .map_err(|_| CoreError::Busy)?;
        let worker = Arc::new(Worker::open(
            path,
            (!board.is_empty()).then(|| board.to_owned()),
        )?);
        workers()
            .lock()
            .map_err(|_| CoreError::Closed)?
            .insert(id, worker);
        unsafe { handle.write(id) };
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ml_request(
    handle: u64,
    data: *const u8,
    length: usize,
    response: *mut Buffer,
) -> i32 {
    boundary(|| {
        if response.is_null() {
            return Err(CoreError::InvalidInput);
        }
        unsafe {
            response.write(Buffer {
                data: std::ptr::null_mut(),
                length: 0,
            })
        };
        let command = serde_json::from_slice(unsafe { bytes(data, length, 3 * 1024 * 1024)? })?;
        let worker = workers()
            .lock()
            .map_err(|_| CoreError::Closed)?
            .get(&handle)
            .cloned()
            .ok_or(CoreError::Closed)?;
        let result = worker.run(command)?;
        let mut bytes = serde_json::to_vec(&result)?.into_boxed_slice();
        let buffer = Buffer {
            data: bytes.as_mut_ptr(),
            length: bytes.len(),
        };
        std::mem::forget(bytes);
        unsafe { response.write(buffer) };
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ml_free_buffer(data: *mut u8, length: usize) {
    if !data.is_null() {
        unsafe {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                data, length,
            )))
        };
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ml_close(handle: u64) -> i32 {
    boundary(|| {
        let worker = workers()
            .lock()
            .map_err(|_| CoreError::Closed)?
            .remove(&handle)
            .ok_or(CoreError::Closed)?;
        drop(worker);
        Ok(())
    })
}
