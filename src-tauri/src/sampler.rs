//! System and per-process sampling with native APIs. Designed to be cheap: process handles are cached between
//! samples and only per-process counters are read each time.

use serde::Serialize;
use std::collections::HashMap;
use std::time::Instant;
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
use windows::Win32::System::Performance::{PdhAddEnglishCounterW, PdhCollectQueryData, PdhGetFormattedCounterValue, PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY};
use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::{GetProcessIoCounters, GetProcessTimes, GetSystemTimes, OpenProcess, IO_COUNTERS, PROCESS_QUERY_LIMITED_INFORMATION};

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    /// lower-case executable file name
    pub name: String,
    /// share of the whole machine's CPU, 0..100
    pub cpu: f32,
    /// private (committed) memory, bytes
    pub mem: u64,
    /// read + write bytes per second (disk and file I/O)
    pub io: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    /// unix ms
    pub t: u64,
    pub cpu: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub commit_used: u64,
    pub commit_limit: u64,
    /// physical disks busy %, 0..100
    pub disk: f32,
    pub procs: Vec<Proc>,
}

impl Sample {
    pub fn mem_pct(&self) -> f32 {
        if self.mem_total == 0 { 0.0 } else { self.mem_used as f32 * 100.0 / self.mem_total as f32 }
    }
    pub fn commit_pct(&self) -> f32 {
        if self.commit_limit == 0 { 0.0 } else { self.commit_used as f32 * 100.0 / self.commit_limit as f32 }
    }
}

fn ft(f: FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

struct Tracked {
    handle: Option<HANDLE>,
    created: u64,
    cpu_time: u64,
    io_bytes: u64,
}

impl Drop for Tracked {
    fn drop(&mut self) {
        if let Some(h) = self.handle.take() {
            unsafe {
                let _ = CloseHandle(h);
            }
        }
    }
}

pub struct Sampler {
    prev_sys: Option<(u64, u64)>, // (busy, total) 100-ns units
    tracked: HashMap<u32, Tracked>,
    last: Option<Instant>,
    pdh: Option<(PDH_HQUERY, PDH_HCOUNTER)>,
    cpus: u32,
}

// PDH handles are plain values owned by this sampler's thread
unsafe impl Send for Sampler {}

impl Sampler {
    pub fn new() -> Self {
        let pdh = unsafe {
            let mut q = PDH_HQUERY::default();
            let mut c = PDH_HCOUNTER::default();
            if PdhOpenQueryW(None, 0, &mut q) == 0 && PdhAddEnglishCounterW(q, w!("\\PhysicalDisk(_Total)\\% Idle Time"), 0, &mut c) == 0 {
                let _ = PdhCollectQueryData(q);
                Some((q, c))
            } else {
                None
            }
        };
        let cpus = std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1);
        Self { prev_sys: None, tracked: HashMap::new(), last: None, pdh, cpus }
    }

    fn system_cpu(&mut self) -> f32 {
        let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
        if unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.is_err() {
            return 0.0;
        }
        let total = ft(kernel) + ft(user); // kernel time includes idle time
        let busy = total - ft(idle);
        let pct = match self.prev_sys {
            Some((pb, pt)) if total > pt => (busy - pb) as f32 * 100.0 / (total - pt) as f32,
            _ => 0.0,
        };
        self.prev_sys = Some((busy, total));
        pct.clamp(0.0, 100.0)
    }

    fn disk_busy(&self) -> f32 {
        let Some((q, c)) = self.pdh else { return 0.0 };
        unsafe {
            if PdhCollectQueryData(q) != 0 {
                return 0.0;
            }
            let mut v = PDH_FMT_COUNTERVALUE::default();
            if PdhGetFormattedCounterValue(c, PDH_FMT_DOUBLE, None, &mut v) != 0 {
                return 0.0;
            }
            (100.0 - v.Anonymous.doubleValue as f32).clamp(0.0, 100.0)
        }
    }

    fn memory() -> (u64, u64, u64, u64) {
        let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        if unsafe { GlobalMemoryStatusEx(&mut m) }.is_err() {
            return (0, 0, 0, 0);
        }
        (m.ullTotalPhys - m.ullAvailPhys, m.ullTotalPhys, m.ullTotalPageFile - m.ullAvailPageFile, m.ullTotalPageFile)
    }

    fn snapshot() -> Vec<(u32, u32, String)> {
        let mut out = Vec::with_capacity(400);
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
            let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            if Process32FirstW(snap, &mut e).is_ok() {
                loop {
                    let n = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
                    out.push((e.th32ProcessID, e.th32ParentProcessID, String::from_utf16_lossy(&e.szExeFile[..n]).to_lowercase()));
                    if Process32NextW(snap, &mut e).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
        out
    }

    pub fn sample(&mut self) -> Sample {
        let now = Instant::now();
        let wall_100ns = self.last.map(|l| now.duration_since(l).as_nanos() as u64 / 100).unwrap_or(0);
        let secs = self.last.map(|l| now.duration_since(l).as_secs_f64()).unwrap_or(0.0);
        self.last = Some(now);

        let cpu = self.system_cpu();
        let disk = self.disk_busy();
        let (mem_used, mem_total, commit_used, commit_limit) = Self::memory();

        let list = Self::snapshot();
        let alive: std::collections::HashSet<u32> = list.iter().map(|p| p.0).collect();
        self.tracked.retain(|pid, _| alive.contains(pid));

        let mut procs = Vec::with_capacity(list.len());
        for (pid, ppid, name) in list {
            if pid == 0 {
                continue; // System Idle Process
            }
            let t = self.tracked.entry(pid).or_insert_with(|| Tracked {
                handle: unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok() },
                created: 0,
                cpu_time: 0,
                io_bytes: 0,
            });
            let Some(h) = t.handle else {
                procs.push(Proc { pid, ppid, name, cpu: 0.0, mem: 0, io: 0 });
                continue;
            };
            let (mut c, mut x, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
            let (mut cpu_p, mut io_p, mut mem) = (0f32, 0u64, 0u64);
            if unsafe { GetProcessTimes(h, &mut c, &mut x, &mut k, &mut u) }.is_ok() {
                let created = ft(c);
                let cpu_time = ft(k) + ft(u);
                if t.created == created && wall_100ns > 0 && cpu_time >= t.cpu_time {
                    cpu_p = ((cpu_time - t.cpu_time) as f64 * 100.0 / (wall_100ns as f64 * self.cpus as f64)) as f32;
                }
                if t.created != created {
                    t.io_bytes = 0; // pid reused by a new process
                }
                t.created = created;
                t.cpu_time = cpu_time;
            }
            let mut io = IO_COUNTERS::default();
            if unsafe { GetProcessIoCounters(h, &mut io) }.is_ok() {
                let bytes = io.ReadTransferCount + io.WriteTransferCount;
                if t.io_bytes > 0 && bytes >= t.io_bytes && secs > 0.0 {
                    io_p = ((bytes - t.io_bytes) as f64 / secs) as u64;
                }
                t.io_bytes = bytes;
            }
            let mut pm = PROCESS_MEMORY_COUNTERS_EX { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32, ..Default::default() };
            if unsafe { K32GetProcessMemoryInfo(h, &mut pm as *mut _ as *mut PROCESS_MEMORY_COUNTERS, pm.cb) }.as_bool() {
                mem = pm.PrivateUsage as u64;
            }
            procs.push(Proc { pid, ppid, name, cpu: cpu_p.clamp(0.0, 100.0), mem, io: io_p });
        }

        Sample { t: crate::now_ms(), cpu, mem_used, mem_total, commit_used, commit_limit, disk, procs }
    }
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_this_machine() {
        let mut s = Sampler::new();
        let _ = s.sample();
        // burn a little CPU so our own process shows up
        let t0 = Instant::now();
        let mut x = 0u64;
        while t0.elapsed().as_millis() < 300 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
        }
        std::hint::black_box(x);
        let b = s.sample();
        assert!(b.mem_total > 0 && b.mem_used > 0 && b.mem_used <= b.mem_total);
        assert!(b.commit_limit >= b.mem_total / 2);
        assert!((0.0..=100.0).contains(&b.cpu));
        assert!(b.procs.len() > 20, "sees the process list");
        let me = b.procs.iter().find(|p| p.pid == std::process::id()).expect("own process");
        assert!(me.cpu > 1.0, "own CPU burn measured: {}", me.cpu);
        assert!(me.mem > 0);
    }
}
