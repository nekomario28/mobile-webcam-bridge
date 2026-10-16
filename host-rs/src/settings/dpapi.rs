//! Same-user, same-machine protection for the Wi-Fi bearer token.
//! Raw Win32 DPAPI FFI keeps the Windows release build free of extra crates.
use std::{ffi::c_void, io, ptr};

#[repr(C)]
struct DataBlob {
    len: u32,
    bytes: *mut u8,
}

#[link(name = "crypt32")]
unsafe extern "system" {
    fn CryptProtectData(
        input: *const DataBlob,
        description: *const u16,
        entropy: *const DataBlob,
        reserved: *const c_void,
        prompt: *const c_void,
        flags: u32,
        output: *mut DataBlob,
    ) -> i32;
    fn CryptUnprotectData(
        input: *const DataBlob,
        description: *mut *mut u16,
        entropy: *const DataBlob,
        reserved: *const c_void,
        prompt: *const c_void,
        flags: u32,
        output: *mut DataBlob,
    ) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LocalFree(ptr: *mut c_void) -> *mut c_void;
}

const NO_UI: u32 = 1;

fn decode_hex(value: &str) -> crate::Result<Vec<u8>> {
    if value.len() > 16 * 1024 || !value.len().is_multiple_of(2) {
        return Err("invalid protected Wi-Fi key".into());
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digits = std::str::from_utf8(pair)?;
            Ok(u8::from_str_radix(digits, 16)?)
        })
        .collect()
}

fn encode_hex(data: &[u8]) -> String {
    data.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn dpapi(input: &[u8], encrypt: bool) -> crate::Result<Vec<u8>> {
    let in_blob = DataBlob {
        len: input.len().try_into()?,
        bytes: input.as_ptr().cast_mut(),
    };
    let mut output = DataBlob {
        len: 0,
        bytes: ptr::null_mut(),
    };
    // User-scope DPAPI: NEVER set CRYPTPROTECT_LOCAL_MACHINE here.
    let success = unsafe {
        if encrypt {
            CryptProtectData(
                &in_blob,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                NO_UI,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &in_blob,
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                NO_UI,
                &mut output,
            )
        }
    };
    if success == 0 {
        return Err(io::Error::last_os_error().into());
    }
    if output.bytes.is_null() {
        return Err("Windows DPAPI returned no output".into());
    }
    let data = unsafe { std::slice::from_raw_parts(output.bytes, output.len as usize) }.to_vec();
    // Clear the native buffer before returning it to LocalFree.
    if !encrypt {
        unsafe { ptr::write_bytes(output.bytes, 0, output.len as usize) };
    }
    unsafe { LocalFree(output.bytes.cast()) };
    Ok(data)
}

pub fn protect(secret: &str) -> crate::Result<String> {
    Ok(encode_hex(&dpapi(secret.as_bytes(), true)?))
}

pub fn unprotect(ciphertext: &str) -> crate::Result<String> {
    let plaintext = dpapi(&decode_hex(ciphertext)?, false)?;
    Ok(String::from_utf8(plaintext)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_keeps_plaintext_out_of_registry_value() {
        let secret = "0123456789abcdef0123456789abcdef";
        let protected = protect(secret).unwrap();
        assert!(!protected.contains(secret));
        assert_eq!(unprotect(&protected).unwrap(), secret);
        assert!(unprotect("garbled").is_err());
    }
}
