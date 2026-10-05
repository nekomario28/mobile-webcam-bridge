use super::*;
use crate::platform::windows::wide;
use std::ptr;
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::*,
};
const PATH: &str = "Software\\nekomario28\\Mobile Webcam";
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
fn read_value(root: HKEY, path: &str, name: &str) -> crate::Result<Option<String>> {
    let mut kind = 0;
    let mut size = 0;
    let code = unsafe {
        RegGetValueW(
            root,
            wide(path).as_ptr(),
            wide(name).as_ptr(),
            RRF_RT_REG_SZ | RRF_RT_REG_DWORD,
            &mut kind,
            ptr::null_mut(),
            &mut size,
        )
    };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if code != ERROR_SUCCESS {
        return Err(std::io::Error::from_raw_os_error(code as i32).into());
    }
    if size > 16 * 1024 {
        return Err("registry setting exceeds limit".into());
    }
    let mut data = vec![0u16; (size as usize).div_ceil(2)];
    let code = unsafe {
        RegGetValueW(
            root,
            wide(path).as_ptr(),
            wide(name).as_ptr(),
            RRF_RT_REG_SZ | RRF_RT_REG_DWORD,
            &mut kind,
            data.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if code != ERROR_SUCCESS {
        return Err(std::io::Error::from_raw_os_error(code as i32).into());
    }
    if kind == REG_DWORD {
        if size != 4 {
            return Err("invalid registry integer".into());
        }
        return Ok(Some(
            ((data[0] as u32) | ((data[1] as u32) << 16)).to_string(),
        ));
    }
    data.truncate(size as usize / 2);
    if data.last() == Some(&0) {
        data.pop();
    }
    Ok(Some(String::from_utf16(&data)?))
}
pub fn read() -> crate::Result<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    for key in Settings::default().values().keys() {
        for (root, path) in [
            (HKEY_CURRENT_USER, PATH),
            (HKEY_CURRENT_USER, "Software\\nekomario28"),
            (HKEY_LOCAL_MACHINE, PATH),
            (HKEY_LOCAL_MACHINE, "Software\\nekomario28"),
        ] {
            if let Some(value) = read_value(root, path, key)? {
                values.insert(key.clone(), value);
                break;
            }
        }
    }
    Ok(values)
}
pub fn save(values: &BTreeMap<String, String>) -> crate::Result<()> {
    Settings::from_values(&read()?)?;
    let mut raw = ptr::null_mut();
    let code = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(PATH).as_ptr(),
            0,
            ptr::null(),
            0,
            KEY_SET_VALUE | KEY_QUERY_VALUE,
            ptr::null(),
            &mut raw,
            ptr::null_mut(),
        )
    };
    if code != ERROR_SUCCESS {
        return Err(std::io::Error::from_raw_os_error(code as i32).into());
    }
    let key = Key(raw);
    for (name, value) in values {
        let text = wide(value);
        let number = if name == "rotation" {
            Some(value.parse::<u32>()?)
        } else {
            None
        };
        let (kind, data, size) = if let Some(ref number) = number {
            (REG_DWORD, (number as *const u32).cast(), 4)
        } else {
            (REG_SZ, text.as_ptr().cast(), (text.len() * 2) as u32)
        };
        let code = unsafe { RegSetValueExW(key.0, wide(name).as_ptr(), 0, kind, data, size) };
        if code != ERROR_SUCCESS {
            return Err(std::io::Error::from_raw_os_error(code as i32).into());
        }
    }
    Ok(())
}
