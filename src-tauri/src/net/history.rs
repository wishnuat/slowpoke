//! Compact 7-day history: one 20-byte record per tick, one file per day, plus "bad moment" detection.

use crate::net::verdict::{Health, Layer, Verdict};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const REC: usize = 20;
const KEEP_DAYS: u64 = 7;
const NONE16: u16 = u16::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rec {
    /// unix seconds
    pub t: u32,
    /// 0 good, 1 degraded, 2 down
    pub health: u8,
    /// Layer::code, 0 = none
    pub layer: u8,
    /// Wi-Fi quality 0..100, 255 = wired/unknown
    pub signal: u8,
    pub inet_loss: u8,
    /// ms, u16::MAX = no reply
    pub router_ms: u16,
    pub inet_ms: u16,
    pub dns_ms: u16,
    /// tenths of Mbps
    pub rx: u16,
    pub tx: u16,
    pub router_loss: u8,
    pub _pad: u8,
}

fn ms16(v: Option<f32>) -> u16 {
    v.map(|x| x.round().clamp(0.0, 65_000.0) as u16).unwrap_or(NONE16)
}

impl Rec {
    pub fn from(v: &Verdict) -> Self {
        let s = &v.snapshot;
        Rec {
            t: (v.t / 1000) as u32,
            health: match v.health { Health::Good => 0, Health::Degraded => 1, Health::Down => 2 },
            layer: v.layer.map(|l| l.code()).unwrap_or(0),
            signal: if s.wireless { s.wifi.as_ref().map(|w| w.quality.min(100) as u8).unwrap_or(255) } else { 255 },
            inet_loss: s.internet.loss.round().clamp(0.0, 100.0) as u8,
            router_ms: ms16(s.router.latency),
            inet_ms: ms16(s.internet.latency),
            dns_ms: ms16(s.dns_ms),
            rx: (s.rx_bps / 100_000).min(65_000) as u16,
            tx: (s.tx_bps / 100_000).min(65_000) as u16,
            router_loss: s.router.loss.round().clamp(0.0, 100.0) as u8,
            _pad: 0,
        }
    }

    fn encode(&self, o: &mut Vec<u8>) {
        o.extend(self.t.to_le_bytes());
        o.extend([self.health, self.layer, self.signal, self.inet_loss]);
        for v in [self.router_ms, self.inet_ms, self.dns_ms, self.rx, self.tx] {
            o.extend(v.to_le_bytes());
        }
        o.extend([self.router_loss, 0]);
    }

    fn decode(b: &[u8]) -> Self {
        let u16at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        Rec {
            t: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            health: b[4],
            layer: b[5],
            signal: b[6],
            inet_loss: b[7],
            router_ms: u16at(8),
            inet_ms: u16at(10),
            dns_ms: u16at(12),
            rx: u16at(14),
            tx: u16at(16),
            router_loss: b[18],
            _pad: 0,
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Moment {
    pub start: u32,
    pub end: u32,
    pub layer: Option<Layer>,
    /// "degraded" | "down"
    pub severity: String,
    /// worst measurement, as text ("internet 40 % loss", "signal 21 %")
    pub worst: String,
}

pub struct History {
    dir: PathBuf,
    pub recs: VecDeque<Rec>,
    pending: Vec<u8>,
}

impl History {
    pub fn open(dir: &Path) -> Self {
        let _ = std::fs::create_dir_all(dir);
        let mut h = Self { dir: dir.to_path_buf(), recs: VecDeque::new(), pending: Vec::new() };
        let today = crate::now_ms() / 1000 / 86_400;
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir).map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "bin")).collect()).unwrap_or_default();
        files.sort();
        for f in files {
            let d: u64 = f.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse().ok()).unwrap_or(0);
            if d + KEEP_DAYS < today {
                let _ = std::fs::remove_file(&f);
                continue;
            }
            let mut b = Vec::new();
            if std::fs::File::open(&f).and_then(|mut x| x.read_to_end(&mut b)).is_ok() {
                h.recs.extend(b.chunks_exact(REC).map(Rec::decode));
            }
        }
        h
    }

    pub fn push(&mut self, r: Rec) {
        r.encode(&mut self.pending);
        self.recs.push_back(r);
        let cutoff = r.t.saturating_sub((KEEP_DAYS * 86_400) as u32);
        while self.recs.front().is_some_and(|x| x.t < cutoff) {
            self.recs.pop_front();
        }
    }

    pub fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let day = self.recs.back().map(|r| r.t as u64 / 86_400).unwrap_or(0);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join(format!("{day}.bin"))) {
            if f.write_all(&self.pending).is_ok() {
                self.pending.clear();
            }
        }
    }

    pub fn range(&self, from: u32) -> impl Iterator<Item = &Rec> {
        self.recs.iter().filter(move |r| r.t >= from)
    }

    /// Periods of ≥ `min_secs` that weren't healthy (gaps < 10 s merged), newest first.
    pub fn moments(&self, from: u32, min_secs: u32) -> Vec<Moment> {
        let mut out = Vec::new();
        let mut cur: Vec<&Rec> = Vec::new();
        let close = |cur: &mut Vec<&Rec>, out: &mut Vec<Moment>| {
            if cur.len() >= 2 && cur.last().unwrap().t - cur[0].t >= min_secs {
                out.push(summarize(cur));
            }
            cur.clear();
        };
        for r in self.range(from) {
            let bad = r.health > 0;
            if cur.last().is_some_and(|l| r.t - l.t > 10) {
                close(&mut cur, &mut out);
            }
            if bad {
                cur.push(r);
            }
        }
        close(&mut cur, &mut out);
        out.reverse();
        out
    }
}

fn summarize(recs: &[&Rec]) -> Moment {
    let mut count: HashMap<u8, usize> = HashMap::new();
    for r in recs {
        *count.entry(r.layer).or_default() += 1;
    }
    let layer_code = count.into_iter().max_by_key(|(_, n)| *n).map(|(l, _)| l).unwrap_or(0);
    let layer = Layer::from_code(layer_code);
    let worst = match layer {
        Some(Layer::Link) => recs.iter().map(|r| r.signal).filter(|s| *s != 255).min().map(|s| format!("signal down to {s} %")).unwrap_or_else(|| "disconnected".into()),
        Some(Layer::Router) => recs.iter().map(|r| r.router_ms).filter(|m| *m != NONE16).max().map(|m| format!("router up to {m} ms")).unwrap_or_else(|| "router not answering".into()),
        Some(Layer::Internet) => {
            let loss = recs.iter().map(|r| r.inet_loss).max().unwrap_or(0);
            let lat = recs.iter().map(|r| r.inet_ms).filter(|m| *m != NONE16).max();
            if loss >= 10 { format!("up to {loss} % packet loss") } else { lat.map(|m| format!("latency up to {m} ms")).unwrap_or_else(|| "no reply".into()) }
        }
        Some(Layer::Dns) => recs.iter().map(|r| r.dns_ms).filter(|m| *m != NONE16).max().map(|m| format!("DNS up to {m} ms")).unwrap_or_else(|| "DNS failing".into()),
        Some(Layer::Traffic) => recs.iter().map(|r| r.rx.max(r.tx)).max().map(|x| format!("traffic up to {:.0} Mbps", x as f32 / 10.0)).unwrap_or_default(),
        None => String::new(),
    };
    Moment {
        start: recs[0].t,
        end: recs.last().unwrap().t,
        layer,
        severity: if recs.iter().any(|r| r.health == 2) { "down".into() } else { "degraded".into() },
        worst,
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

    fn rec(t: u32, health: u8, layer: u8, signal: u8, loss: u8) -> Rec {
        Rec { t, health, layer, signal, inet_loss: loss, router_ms: 3, inet_ms: 20, dns_ms: 25, rx: 10, tx: 2, router_loss: 0, _pad: 0 }
    }

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("netdoctor-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn roundtrip_and_persist() {
        let d = tmp("rt");
        let now = (crate::now_ms() / 1000) as u32;
        let r = Rec { t: now, health: 1, layer: 3, signal: 255, inet_loss: 40, router_ms: 4, inet_ms: NONE16, dns_ms: 900, rx: 450, tx: 12, router_loss: 0, _pad: 0 };
        {
            let mut h = History::open(&d);
            h.push(r);
            h.flush();
        }
        let h = History::open(&d);
        assert_eq!(h.recs.len(), 1);
        assert_eq!(h.recs[0], r);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn moments_need_to_last_and_name_their_cause() {
        let d = tmp("m");
        let mut h = History::open(&d);
        let t0 = (crate::now_ms() / 1000) as u32 - 1000;
        for i in 0..10 {
            h.push(rec(t0 + i * 3, 0, 0, 80, 0));
        }
        for i in 10..20 {
            h.push(rec(t0 + i * 3, 1, 3, 80, 35 + i as u8)); // ISP loss for 27 s
        }
        for i in 20..24 {
            h.push(rec(t0 + i * 3, 0, 0, 80, 0));
        }
        h.push(rec(t0 + 80, 1, 1, 20, 0)); // single blip: ignored
        let m = h.moments(t0, 15);
        assert_eq!(m.len(), 1, "{m:?}");
        assert_eq!(m[0].layer, Some(Layer::Internet));
        assert_eq!(m[0].worst, "up to 54 % packet loss");
        assert_eq!(m[0].severity, "degraded");
        let _ = std::fs::remove_dir_all(d);
    }
}
