//! Compact history: one fixed-size record per sample, one file per day, ~7 days kept.
//! A record stores system load and the top three groups for CPU, disk and memory, by name id.

use crate::verdict::Verdict;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const TOP: usize = 3;
const REC: usize = 4 + 4 + TOP * 3 * 3; // t(u32 s) + cpu,mem,disk,commit (u8) + 3 lists × 3 × (u16 id, u8 value)
const KEEP_DAYS: u64 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize)]
pub struct Rec {
    /// unix seconds
    pub t: u32,
    pub cpu: u8,
    pub mem: u8,
    pub disk: u8,
    pub commit: u8,
    /// (name id, % of the machine's CPU)
    pub top_cpu: [(u16, u8); TOP],
    /// (name id, MB/s capped at 255)
    pub top_io: [(u16, u8); TOP],
    /// (name id, % of RAM)
    pub top_mem: [(u16, u8); TOP],
}

impl Rec {
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend(self.t.to_le_bytes());
        out.extend([self.cpu, self.mem, self.disk, self.commit]);
        for list in [&self.top_cpu, &self.top_io, &self.top_mem] {
            for (id, v) in list {
                out.extend(id.to_le_bytes());
                out.push(*v);
            }
        }
    }
    fn decode(b: &[u8]) -> Self {
        let mut r = Rec { t: u32::from_le_bytes([b[0], b[1], b[2], b[3]]), cpu: b[4], mem: b[5], disk: b[6], commit: b[7], ..Default::default() };
        let mut i = 8;
        for list in [&mut r.top_cpu, &mut r.top_io, &mut r.top_mem] {
            for slot in list.iter_mut() {
                *slot = (u16::from_le_bytes([b[i], b[i + 1]]), b[i + 2]);
                i += 3;
            }
        }
        r
    }
    pub fn busy(&self) -> bool {
        self.cpu >= 85 || self.disk >= 90 || self.mem >= 93
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Moment {
    pub start: u32,
    pub end: u32,
    /// "cpu" | "disk" | "memory"
    pub resource: String,
    pub peak: u8,
    /// names, biggest first
    pub culprits: Vec<String>,
}

pub struct History {
    dir: PathBuf,
    names: Vec<String>,
    ids: HashMap<String, u16>,
    pub recs: VecDeque<Rec>,
    pending: Vec<u8>,
    dirty_names: bool,
}

fn day(t: u32) -> u64 {
    t as u64 / 86_400
}

impl History {
    pub fn open(dir: &Path) -> Self {
        let _ = std::fs::create_dir_all(dir);
        let names: Vec<String> = std::fs::read(dir.join("names.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let ids = names.iter().enumerate().map(|(i, n)| (n.clone(), i as u16)).collect();
        let mut h = Self { dir: dir.to_path_buf(), names, ids, recs: VecDeque::new(), pending: Vec::new(), dirty_names: false };
        h.load();
        h
    }

    fn load(&mut self) {
        let now = (crate::now_ms() / 1000) as u32;
        let mut files: Vec<PathBuf> = std::fs::read_dir(&self.dir)
            .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "bin")).collect())
            .unwrap_or_default();
        files.sort();
        for f in files {
            let d: u64 = f.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse().ok()).unwrap_or(0);
            if d + KEEP_DAYS < day(now) {
                let _ = std::fs::remove_file(&f);
                continue;
            }
            let mut b = Vec::new();
            if std::fs::File::open(&f).and_then(|mut x| x.read_to_end(&mut b)).is_ok() {
                for chunk in b.chunks_exact(REC) {
                    self.recs.push_back(Rec::decode(chunk));
                }
            }
        }
    }

    pub fn id(&mut self, name: &str) -> u16 {
        if let Some(i) = self.ids.get(name) {
            return *i;
        }
        let i = self.names.len().min(u16::MAX as usize - 1) as u16;
        self.names.push(name.to_string());
        self.ids.insert(name.to_string(), i);
        self.dirty_names = true;
        i
    }

    pub fn name(&self, id: u16) -> &str {
        self.names.get(id as usize).map(|s| s.as_str()).unwrap_or("?")
    }

    /// Append the record for a verdict. (Verdict culprits are sorted by the scarce resource; we keep all three views.)
    pub fn push(&mut self, v: &Verdict) {
        let pick = |list: &mut Vec<(String, f64)>| -> Vec<(String, f64)> {
            list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            list.iter().take(TOP).cloned().collect()
        };
        let mut cpu: Vec<(String, f64)> = v.culprits.iter().map(|c| (c.name.clone(), c.cpu as f64)).collect();
        let mut io: Vec<(String, f64)> = v.culprits.iter().map(|c| (c.name.clone(), c.io as f64 / 1_048_576.0)).collect();
        let mut mem: Vec<(String, f64)> = v.culprits.iter().map(|c| (c.name.clone(), c.mem as f64 * 100.0 / v.mem_total.max(1) as f64)).collect();
        let mut r = Rec {
            t: (v.t / 1000) as u32,
            cpu: v.cpu.round() as u8,
            mem: v.mem_pct.round() as u8,
            disk: v.disk.round() as u8,
            commit: v.commit_pct.round().min(255.0) as u8,
            ..Default::default()
        };
        for (slot, (n, x)) in r.top_cpu.iter_mut().zip(pick(&mut cpu)) {
            *slot = (self.id(&n), x.round().clamp(0.0, 255.0) as u8);
        }
        for (slot, (n, x)) in r.top_io.iter_mut().zip(pick(&mut io)) {
            *slot = (self.id(&n), x.round().clamp(0.0, 255.0) as u8);
        }
        for (slot, (n, x)) in r.top_mem.iter_mut().zip(pick(&mut mem)) {
            *slot = (self.id(&n), x.round().clamp(0.0, 255.0) as u8);
        }
        r.encode(&mut self.pending);
        self.recs.push_back(r);
        let cutoff = r.t.saturating_sub((KEEP_DAYS * 86_400) as u32);
        while self.recs.front().is_some_and(|x| x.t < cutoff) {
            self.recs.pop_front();
        }
    }

    /// Write pending records (call every ~minute and on exit).
    pub fn flush(&mut self) {
        if !self.pending.is_empty() {
            let d = self.recs.back().map(|r| day(r.t)).unwrap_or(0);
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join(format!("{d}.bin"))) {
                if f.write_all(&self.pending).is_ok() {
                    self.pending.clear();
                }
            }
        }
        if self.dirty_names {
            if std::fs::write(self.dir.join("names.json"), serde_json::to_vec(&self.names).unwrap()).is_ok() {
                self.dirty_names = false;
            }
        }
    }

    pub fn range(&self, from: u32) -> impl Iterator<Item = &Rec> {
        self.recs.iter().filter(move |r| r.t >= from)
    }

    /// Sustained busy periods since `from`: at least `min_secs` long, gaps under 12 s merged.
    pub fn moments(&self, from: u32, min_secs: u32) -> Vec<Moment> {
        let mut out = Vec::new();
        let mut cur: Vec<&Rec> = Vec::new();
        let finish = |cur: &mut Vec<&Rec>, out: &mut Vec<Moment>| {
            if cur.len() >= 2 && cur.last().unwrap().t - cur[0].t >= min_secs {
                out.push(self.summarize(cur));
            }
            cur.clear();
        };
        for r in self.range(from) {
            if r.busy() {
                if cur.last().is_some_and(|l| r.t - l.t > 12) {
                    finish(&mut cur, &mut out);
                }
                cur.push(r);
            } else if cur.last().is_some_and(|l| r.t - l.t > 12) {
                finish(&mut cur, &mut out);
            }
        }
        finish(&mut cur, &mut out);
        out
    }

    fn summarize(&self, recs: &[&Rec]) -> Moment {
        let n = recs.len() as f32;
        let (c, d, m) = recs.iter().fold((0f32, 0f32, 0f32), |a, r| (a.0 + r.cpu as f32, a.1 + r.disk as f32, a.2 + r.mem as f32));
        let res = crate::verdict::scarce(c / n, m / n, d / n);
        let (key, peak) = match res {
            crate::verdict::Resource::Cpu => ("cpu", recs.iter().map(|r| r.cpu).max().unwrap_or(0)),
            crate::verdict::Resource::Disk => ("disk", recs.iter().map(|r| r.disk).max().unwrap_or(0)),
            crate::verdict::Resource::Memory => ("memory", recs.iter().map(|r| r.mem).max().unwrap_or(0)),
        };
        let mut score: HashMap<u16, u32> = HashMap::new();
        for r in recs {
            let list = match res {
                crate::verdict::Resource::Cpu => &r.top_cpu,
                crate::verdict::Resource::Disk => &r.top_io,
                crate::verdict::Resource::Memory => &r.top_mem,
            };
            for (id, v) in list {
                *score.entry(*id).or_default() += *v as u32;
            }
        }
        let mut ranked: Vec<(u16, u32)> = score.into_iter().filter(|(_, s)| *s > 0).collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1));
        Moment {
            start: recs[0].t,
            end: recs.last().unwrap().t,
            resource: key.into(),
            peak,
            culprits: ranked.iter().take(3).map(|(id, _)| self.name(*id).to_string()).collect(),
        }
    }
}

impl Drop for History {
    fn drop(&mut self) {
        self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verdict::{Culprit, Level, Verdict};

    fn verdict(t: u64, cpu: f32, disk: f32, who: &[(&str, f32, u64)]) -> Verdict {
        Verdict {
            t: t * 1000,
            level: Level::Calm,
            resource: None,
            headline: String::new(),
            detail: String::new(),
            cpu,
            mem_pct: 40.0,
            commit_pct: 30.0,
            disk,
            mem_used: 0,
            mem_total: 16 << 30,
            culprits: who
                .iter()
                .map(|(n, c, io)| Culprit {
                    key: n.to_string(),
                    exe: n.to_string(),
                    name: n.to_string(),
                    what: String::new(),
                    advice: String::new(),
                    pids: vec![],
                    cpu: *c,
                    mem: 100 << 20,
                    io: io << 20,
                    share: 0.0,
                    critical: false,
                    services: vec![],
                })
                .collect(),
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("slowpoke-hist-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn record_roundtrip_and_persistence() {
        let dir = tmp("persist");
        let now = (crate::now_ms() / 1000) as u64;
        {
            let mut h = History::open(&dir);
            h.push(&verdict(now, 50.0, 20.0, &[("Google Chrome", 30.0, 2), ("Excel", 10.0, 0)]));
            h.flush();
        }
        let h = History::open(&dir);
        assert_eq!(h.recs.len(), 1);
        let r = h.recs[0];
        assert_eq!(r.cpu, 50);
        assert_eq!(h.name(r.top_cpu[0].0), "Google Chrome");
        assert_eq!(r.top_cpu[0].1, 30);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn detects_sustained_busy_moments_and_names_the_cause() {
        let dir = tmp("moments");
        let mut h = History::open(&dir);
        let t0 = (crate::now_ms() / 1000) as u64 - 600;
        for i in 0..20 {
            h.push(&verdict(t0 + i * 2, 15.0, 5.0, &[("Explorer", 2.0, 0)])); // calm
        }
        for i in 20..35 {
            h.push(&verdict(t0 + i * 2, 20.0, 97.0, &[("Microsoft Defender", 10.0, 80), ("Google Chrome", 8.0, 1)])); // disk storm
        }
        for i in 35..40 {
            h.push(&verdict(t0 + i * 2, 15.0, 5.0, &[("Explorer", 2.0, 0)]));
        }
        h.push(&verdict(t0 + 82, 99.0, 5.0, &[("Game", 90.0, 0)])); // single spike: not a moment
        let m = h.moments(t0 as u32, 8);
        assert_eq!(m.len(), 1, "{m:?}");
        assert_eq!(m[0].resource, "disk");
        assert_eq!(m[0].culprits[0], "Microsoft Defender");
        assert_eq!(m[0].end - m[0].start, 28);
        let _ = std::fs::remove_dir_all(dir);
    }
}
