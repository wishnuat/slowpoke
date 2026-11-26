//! Which Windows services run inside which svchost.exe (pid → service names).

use std::collections::HashMap;
use windows::core::PCWSTR;
use windows::Win32::System::Services::{
    CloseServiceHandle, EnumServicesStatusExW, OpenSCManagerW, ENUM_SERVICE_STATUS_PROCESSW, SC_ENUM_PROCESS_INFO,
    SC_MANAGER_ENUMERATE_SERVICE, SERVICE_ACTIVE, SERVICE_WIN32,
};

pub fn services_by_pid() -> HashMap<u32, Vec<String>> {
    let mut out: HashMap<u32, Vec<String>> = HashMap::new();
    unsafe {
        let Ok(scm) = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_ENUMERATE_SERVICE) else { return out };
        let mut needed = 0u32;
        let mut count = 0u32;
        let mut resume = 0u32;
        // first call: size the buffer
        let _ = EnumServicesStatusExW(scm, SC_ENUM_PROCESS_INFO, SERVICE_WIN32, SERVICE_ACTIVE, None, &mut needed, &mut count, Some(&mut resume), PCWSTR::null());
        if needed > 0 {
            let mut buf = vec![0u8; needed as usize + 4096];
            resume = 0;
            if EnumServicesStatusExW(scm, SC_ENUM_PROCESS_INFO, SERVICE_WIN32, SERVICE_ACTIVE, Some(&mut buf), &mut needed, &mut count, Some(&mut resume), PCWSTR::null()).is_ok() {
                let items = std::slice::from_raw_parts(buf.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW, count as usize);
                for s in items {
                    let name = s.lpServiceName.to_string().unwrap_or_default();
                    out.entry(s.ServiceStatusProcess.dwProcessId).or_default().push(name);
                }
            }
        }
        let _ = CloseServiceHandle(scm);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_running_services() {
        let m = super::services_by_pid();
        assert!(m.values().flatten().count() > 20, "a Windows machine runs dozens of services");
        assert!(m.values().flatten().any(|s| s.eq_ignore_ascii_case("EventLog")), "EventLog always runs");
    }
}
