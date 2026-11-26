//! Small Win32 helpers: process image path, product name, icon → PNG data URL, foreground process.

use base64::Engine;
use std::ffi::c_void;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetForegroundWindow, GetIconInfo, GetWindowThreadProcessId, PrivateExtractIconsW, HICON, ICONINFO,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Convert a COM-allocated PWSTR into a String and free it.
pub fn take_pwstr(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = unsafe { p.to_string().unwrap_or_default() };
    unsafe { CoTaskMemFree(Some(p.0 as *const c_void)) };
    s
}

/// Full path of a process' executable, if we are allowed to query it.
pub fn process_path(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// Process id owning the foreground window.
pub fn foreground_pid() -> Option<u32> {
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        (pid != 0).then_some(pid)
    }
}

/// "FileDescription" from the executable's version resource, e.g. "Google Chrome".
pub fn product_name(path: &str) -> Option<String> {
    unsafe {
        let wpath = wide(path);
        let size = GetFileVersionInfoSizeW(PCWSTR(wpath.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(wpath.as_ptr()), None, size, data.as_mut_ptr() as *mut c_void).ok()?;

        // first language/codepage pair
        let mut ptr: *mut c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let q = wide("\\VarFileInfo\\Translation");
        let mut candidates = Vec::new();
        if VerQueryValueW(data.as_ptr() as *const c_void, PCWSTR(q.as_ptr()), &mut ptr, &mut len).as_bool() && len >= 4 {
            let lang = *(ptr as *const u16);
            let cp = *((ptr as *const u16).add(1));
            candidates.push(format!("{lang:04x}{cp:04x}"));
        }
        candidates.extend(["040904b0".to_string(), "040904e4".to_string(), "000004b0".to_string()]);

        for key in ["FileDescription", "ProductName"] {
            for lc in &candidates {
                let q = wide(&format!("\\StringFileInfo\\{lc}\\{key}"));
                let mut p: *mut c_void = std::ptr::null_mut();
                let mut n = 0u32;
                if VerQueryValueW(data.as_ptr() as *const c_void, PCWSTR(q.as_ptr()), &mut p, &mut n).as_bool() && n > 1 {
                    let s = String::from_utf16_lossy(std::slice::from_raw_parts(p as *const u16, n as usize - 1));
                    let s = s.trim().to_string();
                    if !s.is_empty() {
                        return Some(s);
                    }
                }
            }
        }
        None
    }
}

/// Fallback display name from a path: "C:\\…\\spotify.exe" → "Spotify".
pub fn stem_name(path: &str) -> String {
    let stem = std::path::Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or(path);
    let mut c = stem.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Extract the executable's icon at `size`px and return it as a `data:image/png;base64,…` URL.
pub fn icon_data_url(path: &str, size: i32) -> Option<String> {
    unsafe {
        let mut name = [0u16; 260];
        for (i, u) in path.encode_utf16().take(259).enumerate() {
            name[i] = u;
        }
        let mut icons = [HICON::default()];
        let n = PrivateExtractIconsW(&name, 0, size, size, Some(&mut icons), None, 0);
        if n == 0 || n == u32::MAX || icons[0].is_invalid() {
            return None;
        }
        let result = hicon_to_png(icons[0]);
        let _ = DestroyIcon(icons[0]);
        let png = result?;
        Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)))
    }
}

unsafe fn hicon_to_png(icon: HICON) -> Option<Vec<u8>> {
    let mut info = ICONINFO::default();
    unsafe { GetIconInfo(icon, &mut info).ok()? };
    let color = info.hbmColor;
    let mask = info.hbmMask;
    let out = (|| {
        if color.is_invalid() {
            return None;
        }
        let mut bm = BITMAP::default();
        if unsafe { GetObjectW(HGDIOBJ(color.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut c_void)) } == 0 {
            return None;
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        let dc = unsafe { CreateCompatibleDC(None) };
        let lines = unsafe { GetDIBits(dc, color, 0, h as u32, Some(px.as_mut_ptr() as *mut c_void), &mut bmi, DIB_RGB_COLORS) };
        unsafe {
            let _ = DeleteDC(dc);
        }
        if lines == 0 {
            return None;
        }
        // BGRA → RGBA; very old icons have no alpha channel at all
        let has_alpha = px.chunks_exact(4).any(|p| p[3] != 0);
        for p in px.chunks_exact_mut(4) {
            p.swap(0, 2);
            if !has_alpha {
                p[3] = 255;
            }
        }
        let mut png = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png, w as u32, h as u32);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut wr = enc.write_header().ok()?;
            wr.write_image_data(&px).ok()?;
        }
        Some(png)
    })();
    unsafe {
        if !color.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(color.0));
        }
        if !mask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(mask.0));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_from_paths() {
        assert_eq!(stem_name(r"C:\Program Files\Spotify\spotify.exe"), "Spotify");
    }

    #[test]
    fn reads_version_info_and_icon_of_explorer() {
        let p = r"C:\Windows\explorer.exe";
        assert!(product_name(p).unwrap().to_lowercase().contains("explorer"));
        assert!(icon_data_url(p, 32).unwrap().starts_with("data:image/png;base64,"));
    }

    #[test]
    fn own_process_path() {
        let me = process_path(std::process::id()).unwrap();
        assert!(me.to_lowercase().ends_with(".exe"));
    }
}
