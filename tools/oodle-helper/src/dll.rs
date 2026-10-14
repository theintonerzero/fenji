//! The one place this workspace writes `unsafe` FFI, into the client's own oo2core_6_win64.dll:
//! OodleLZ_Decompress with the arguments the client itself passes (FUN_14024E060; M2 plan F8), and
//! OodleLZ_Compress with the arguments ooz passes (kraken.cpp:4375), for fenji-oodle's fixtures.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::path::Path;
use std::ptr::{null, null_mut};

use libloading::Library;

/// 14 arguments, as at the client's call site: input (the block after its 4-byte raw length), its
/// length, output, raw length, then the constants 1, 0, 0, null, 0, null, null, null, 0, 3.
type Decompress = unsafe extern "C" fn(
    *const u8,
    isize,
    *mut u8,
    isize,
    i32,
    i32,
    i32,
    *mut u8,
    isize,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    isize,
    i32,
) -> isize;

/// ooz's OodLZ_CompressFunc (kraken.cpp:4288): compressor, input, its length, output, level, then
/// options, dictionary, unused, scratch and its size, all zero (kraken.cpp:4375). The result, the
/// compressed length, is pointer-sized like `Decompress`'s; ooz's `int` reads only its low half.
type Compress = unsafe extern "C" fn(
    compressor: i32,
    src: *const u8,
    src_len: usize,
    dst: *mut u8,
    level: i32,
    options: *const c_void,
    dictionary: usize,
    unused: usize,
    scratch: *mut c_void,
    scratch_size: usize,
) -> isize;

pub struct Oodle {
    _lib: Library,
    decompress: Decompress,
    /// `None` if the DLL doesn't export `OodleLZ_Compress`.
    compress: Option<Compress>,
}

impl Oodle {
    pub fn load(path: &Path) -> Result<Self, String> {
        // SAFETY: loading runs the DLL's initialisation. This helper exists to load exactly this
        // DLL from the owner's client copy, and it runs only inside the sandbox (run-oodle-helper.sh).
        let lib = unsafe { Library::new(path) }.map_err(|e| format!("{}: {e}", path.display()))?;
        // SAFETY: the symbol's type is the one the client's own call site uses (F8).
        let decompress = unsafe { lib.get::<Decompress>(b"OodleLZ_Decompress\0") }
            .map(|s| *s)
            .map_err(|e| format!("OodleLZ_Decompress: {e}"))?;
        // SAFETY: the symbol's type is the one ooz declares for it (kraken.cpp:4288).
        let compress = unsafe { lib.get::<Compress>(b"OodleLZ_Compress\0") }
            .ok()
            .map(|s| *s);
        Ok(Self {
            _lib: lib,
            decompress,
            compress,
        })
    }

    /// Whether the DLL exports `OodleLZ_Compress`.
    pub fn can_compress(&self) -> bool {
        self.compress.is_some()
    }

    /// Decodes `input`, an Oodle stream without the block's length prefix
    /// ([`crate::spool::oodle_stream`]). The raw bytes, or `None` if the DLL doesn't return
    /// exactly `raw_len` bytes.
    pub fn decompress(&self, input: &[u8], raw_len: usize) -> Option<Vec<u8>> {
        let mut out = vec![0u8; raw_len];
        // SAFETY: both buffers are valid for the lengths passed; the library outlives the call.
        let n = unsafe {
            (self.decompress)(
                input.as_ptr(),
                input.len() as isize,
                out.as_mut_ptr(),
                raw_len as isize,
                1,
                0,
                0,
                null_mut(),
                0,
                null_mut(),
                null_mut(),
                null_mut(),
                0,
                3,
            )
        };
        (n == raw_len as isize).then_some(out)
    }

    /// `input` compressed with `compressor` at `level`, or None if the DLL refused.
    pub fn compress(&self, compressor: i32, level: i32, input: &[u8]) -> Option<Vec<u8>> {
        let f = self.compress?;
        // Oodle's worst case is the raw length plus a few hundred bytes per 256 KiB: this is ample.
        let mut out = vec![0u8; input.len() + input.len() / 8 + 65_536];
        // SAFETY: the arguments ooz passes (kraken.cpp:4375); `out` outlasts the call and is larger
        // than any output Oodle writes for this input.
        let n = unsafe {
            f(
                compressor,
                input.as_ptr(),
                input.len(),
                out.as_mut_ptr(),
                level,
                null(),
                0,
                0,
                null_mut(),
                0,
            )
        };
        let n = usize::try_from(n)
            .ok()
            .filter(|&n| n > 0 && n <= out.len())?;
        out.truncate(n);
        Some(out)
    }
}
