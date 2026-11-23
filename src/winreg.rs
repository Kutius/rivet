use anyhow::{Result, bail};

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

/// Read a string (`REG_SZ` or `REG_EXPAND_SZ`) value from `HKEY_CURRENT_USER`
/// without expanding environment variables. Returns `Ok(None)` if the key or
/// value does not exist.
#[cfg(windows)]
pub fn get_hkcu_string(subkey: &str, name: &str) -> Result<Option<String>> {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW,
    };

    const ERROR_MORE_DATA: u32 = 234;
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
    let key_wide = wide(subkey);
    let name_wide = wide(name);
    let mut kind = 0u32;
    let mut size = 0u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key_wide.as_ptr(),
            name_wide.as_ptr(),
            flags,
            &mut kind,
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if status != ERROR_SUCCESS && status != ERROR_MORE_DATA {
        bail!("reading registry value '{subkey}\\{name}' failed with Windows error {status}");
    }
    let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
    let mut actual = size;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key_wide.as_ptr(),
            name_wide.as_ptr(),
            flags,
            &mut kind,
            buffer.as_mut_ptr().cast(),
            &mut actual,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if status != ERROR_SUCCESS {
        bail!("reading registry value '{subkey}\\{name}' failed with Windows error {status}");
    }
    let value = String::from_utf16_lossy(&buffer[..(actual as usize / 2)]);
    Ok(Some(value.trim_end_matches('\0').to_string()))
}

/// Write a `REG_SZ` (or `REG_EXPAND_SZ` when `expand` is true) value under
/// `HKEY_CURRENT_USER`, creating the subkey if necessary.
#[cfg(windows)]
pub fn set_hkcu_string(subkey: &str, name: &str, value: &str, expand: bool) -> Result<()> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ, RegCloseKey, RegCreateKeyExW,
        RegSetValueExW,
    };

    let key_wide = wide(subkey);
    let name_wide = wide(name);
    let value_wide = wide(value);
    let mut handle = std::ptr::null_mut();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key_wide.as_ptr(),
            0,
            std::ptr::null(),
            0,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut handle,
            std::ptr::null_mut(),
        )
    };
    if status != ERROR_SUCCESS {
        bail!("opening registry key '{subkey}' failed with Windows error {status}");
    }
    let kind = if expand { REG_EXPAND_SZ } else { REG_SZ };
    let status = unsafe {
        RegSetValueExW(
            handle,
            name_wide.as_ptr(),
            0,
            kind,
            value_wide.as_ptr().cast(),
            (value_wide.len() * 2) as u32,
        )
    };
    unsafe {
        RegCloseKey(handle);
    }
    if status != ERROR_SUCCESS {
        bail!("writing registry value '{subkey}\\{name}' failed with Windows error {status}");
    }
    Ok(())
}

/// Delete a value under `HKEY_CURRENT_USER`. Succeeds if the key or value is
/// already absent.
#[cfg(windows)]
pub fn delete_hkcu_value(subkey: &str, name: &str) -> Result<()> {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, KEY_SET_VALUE, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
    };

    let key_wide = wide(subkey);
    let name_wide = wide(name);
    let mut handle = std::ptr::null_mut();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            key_wide.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut handle,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    if status != ERROR_SUCCESS {
        bail!("opening registry key '{subkey}' failed with Windows error {status}");
    }
    let status = unsafe { RegDeleteValueW(handle, name_wide.as_ptr()) };
    unsafe {
        RegCloseKey(handle);
    }
    if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
        bail!("deleting registry value '{subkey}\\{name}' failed with Windows error {status}");
    }
    Ok(())
}

#[cfg(windows)]
pub fn broadcast_setting_change(area: &str) -> Result<()> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };

    let area_wide = wide(area);
    let mut result = 0usize;
    let ok = unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            area_wide.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            5000,
            &mut result,
        )
    };
    if ok == 0 {
        bail!("Windows did not acknowledge WM_SETTINGCHANGE");
    }
    Ok(())
}

#[cfg(windows)]
pub fn broadcast_font_change() -> Result<()> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_FONTCHANGE,
    };

    let mut result = 0usize;
    let ok = unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_FONTCHANGE,
            0,
            0,
            SMTO_ABORTIFHUNG,
            5000,
            &mut result,
        )
    };
    if ok == 0 {
        bail!("Windows did not acknowledge WM_FONTCHANGE");
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn get_hkcu_string(_subkey: &str, _name: &str) -> Result<Option<String>> {
    Ok(None)
}

#[cfg(not(windows))]
pub fn set_hkcu_string(_subkey: &str, _name: &str, _value: &str, _expand: bool) -> Result<()> {
    bail!("Windows registry access is only supported on Windows")
}

#[cfg(not(windows))]
pub fn delete_hkcu_value(_subkey: &str, _name: &str) -> Result<()> {
    Ok(())
}

#[cfg(not(windows))]
pub fn broadcast_setting_change(_area: &str) -> Result<()> {
    Ok(())
}

#[cfg(not(windows))]
pub fn broadcast_font_change() -> Result<()> {
    Ok(())
}
