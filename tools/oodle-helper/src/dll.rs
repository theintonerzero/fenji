//! The one place this workspace calls foreign code: OodleLZ_Decompress in the client's own
//! oo2core_6_win64.dll, with the arguments the client itself passes (FUN_14024E060; M2 plan F8).
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::path::Path;
use std::ptr::null_mut;

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

pub struct Oodle {
    _lib: Library,
    decompress: Decompress,
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
        Ok(Self {
            _lib: lib,
            decompress,
        })
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
}
