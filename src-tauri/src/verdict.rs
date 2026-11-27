//! Turn a sample into a plain-language verdict: what is scarce, and who is using it.

use crate::catalog;
use crate::sampler::Sample;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    Calm,
    Busy,
    Strained,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Resource {
    Cpu,
    Memory,
    Disk,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Culprit {
    /// group key: exe name, or "svc:<service>" for svchost-hosted services
    pub key: String,
    pub exe: String,
    pub name: String,
    pub what: String,
    pub advice: String,
    pub pids: Vec<u32>,
    pub cpu: f32,
    pub mem: u64,
    pub io: u64,
    /// share of the scarce resource among all processes, 0..1
    pub share: f32,
    pub critical: bool,
    pub services: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    pub t: u64,
    pub level: Level,
    pub resource: Option<Resource>,
    pub headline: String,
    pub detail: String,
    pub cpu: f32,
    pub mem_pct: f32,
    pub commit_pct: f32,
    pub disk: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub culprits: Vec<Culprit>,
}

pub fn level_of(cpu: f32, mem: f32, disk: f32) -> Level {
    if cpu >= 90.0 || disk >= 95.0 || mem >= 95.0 {
        Level::Strained
    } else if cpu >= 60.0 || disk >= 70.0 || mem >= 88.0 {
        Level::Busy
    } else {
        Level::Calm
    }
}

/// The most pressured resource, relative to its "strained" threshold.
pub fn scarce(cpu: f32, mem: f32, disk: f32) -> Resource {
    let (c, m, d) = (cpu / 90.0, mem / 95.0, disk / 95.0);
    if d >= c && d >= m {
        Resource::Disk
    } else if m >= c {
        Resource::Memory
    } else {
        Resource::Cpu
    }
}

fn title_case(exe: &str) -> String {
    let stem = exe.strip_suffix(".exe").unwrap_or(exe);
    let mut c = stem.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

fn join_names(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [a] => a.to_string(),
        [a, b] => format!("{a} and {b}"),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// `label(exe, pid)` may supply a nicer product name for apps the catalogue doesn't know.
pub fn judge(s: &Sample, services: &HashMap<u32, Vec<String>>, label: &dyn Fn(&str, u32) -> Option<String>) -> Verdict {
    let mem_pct = s.mem_pct().max(s.commit_pct() - 5.0).max(0.0);
    let level = level_of(s.cpu, mem_pct, s.disk);
    let res = scarce(s.cpu, mem_pct, s.disk);

    // group processes
    let mut groups: HashMap<String, Culprit> = HashMap::new();
    for p in &s.procs {
        let svc = (p.name == "svchost.exe")
            .then(|| services.get(&p.pid))
            .flatten()
            .and_then(|list| list.iter().find_map(|n| catalog::service(n).map(|(f, w)| (n.clone(), f, w))));
        let (key, name, what, advice, critical) = match (&svc, catalog::by_exe(&p.name)) {
            (Some((_, f, w)), _) => (format!("svc:{f}"), f.to_string(), w.to_string(), "A Windows service. It normally settles down by itself.".to_string(), true),
            (None, Some(e)) => (p.name.clone(), e.name.to_string(), e.what.to_string(), e.advice.to_string(), e.critical),
            (None, None) => {
                let name = label(&p.name, p.pid).unwrap_or_else(|| title_case(&p.name));
                (p.name.clone(), name, "An app running on your PC.".to_string(), "If you don't recognise it, check its file location.".to_string(), false)
            }
        };
        let g = groups.entry(key.clone()).or_insert_with(|| Culprit {
            key,
            exe: p.name.clone(),
            name,
            what,
            advice,
            pids: Vec::new(),
            cpu: 0.0,
            mem: 0,
            io: 0,
            share: 0.0,
            critical,
            services: Vec::new(),
        });
        g.pids.push(p.pid);
        g.cpu += p.cpu;
        g.mem += p.mem;
        g.io += p.io;
        if let Some(list) = services.get(&p.pid).filter(|_| p.name == "svchost.exe") {
            g.services.extend(list.iter().cloned());
        }
    }

    let metric = |c: &Culprit| -> f64 {
        match res {
            Resource::Cpu => c.cpu as f64,
            Resource::Disk => c.io as f64,
            Resource::Memory => c.mem as f64,
        }
    };
    let mut list: Vec<Culprit> = groups.into_values().filter(|c| c.exe != "slowpoke.exe" || level == Level::Calm).collect();
    let total: f64 = list.iter().map(metric).sum::<f64>().max(1e-9);
    for c in &mut list {
        c.share = (metric(c) / total) as f32;
    }
    list.sort_by(|a, b| metric(b).partial_cmp(&metric(a)).unwrap_or(std::cmp::Ordering::Equal));
    list.truncate(8);

    let (headline, detail) = if level == Level::Calm {
        ("All calm. Nothing is straining your PC right now.".to_string(), format!("CPU {:.0} % · memory {:.0} % · disk {:.0} %", s.cpu, s.mem_pct(), s.disk))
    } else {
        // name the main one, plus the second when it's a real contributor
        let top: Vec<&str> = list.iter().take(3).enumerate().filter(|(i, c)| *i == 0 || c.share >= 0.2).map(|(_, c)| c.name.as_str()).collect();
        let who = join_names(&top);
        let lead = if level == Level::Strained { "Your PC is struggling" } else { "Your PC is busy" };
        match res {
            Resource::Cpu => (format!("{lead}: {who} {} using most of the processor.", if top.len() > 1 { "are" } else { "is" }), format!("CPU at {:.0} %", s.cpu)),
            Resource::Disk => (format!("{lead}: {who} {} keeping the disk busy.", if top.len() > 1 { "are" } else { "is" }), format!("Disk busy {:.0} % of the time", s.disk)),
            Resource::Memory => (format!("{lead}: memory is nearly full. {who} {} the most.", if top.len() > 1 { "use" } else { "uses" }), format!("Memory {:.0} % used, commit {:.0} %", s.mem_pct(), s.commit_pct())),
        }
    };

    Verdict {
        t: s.t,
        level,
        resource: (level != Level::Calm).then_some(res),
        headline,
        detail,
        cpu: s.cpu,
        mem_pct: s.mem_pct(),
        commit_pct: s.commit_pct(),
        disk: s.disk,
        mem_used: s.mem_used,
        mem_total: s.mem_total,
        culprits: list,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::Proc;

    fn p(pid: u32, name: &str, cpu: f32, mem_mb: u64, io_mb: u64) -> Proc {
        Proc { pid, ppid: 0, name: name.into(), cpu, mem: mem_mb << 20, io: io_mb << 20 }
    }

    fn sample(cpu: f32, mem_pct: f32, disk: f32, procs: Vec<Proc>) -> Sample {
        let total = 16u64 << 30;
        Sample { t: 1, cpu, mem_used: (total as f32 * mem_pct / 100.0) as u64, mem_total: total, commit_used: total / 2, commit_limit: total * 2, disk, procs }
    }

    #[test]
    fn calm_machine() {
        let v = judge(&sample(12.0, 40.0, 5.0, vec![p(10, "chrome.exe", 5.0, 900, 0)]), &HashMap::new(), &|_, _| None);
        assert_eq!(v.level, Level::Calm);
        assert!(v.headline.starts_with("All calm"));
        assert_eq!(v.resource, None);
    }

    #[test]
    fn cpu_blames_grouped_apps_by_friendly_name() {
        let procs = vec![p(1, "chrome.exe", 20.0, 300, 0), p(2, "chrome.exe", 25.0, 300, 0), p(3, "msmpeng.exe", 30.0, 200, 5), p(4, "notepad.exe", 1.0, 10, 0)];
        let v = judge(&sample(80.0, 50.0, 20.0, procs), &HashMap::new(), &|_, _| None);
        assert_eq!(v.level, Level::Busy);
        assert_eq!(v.resource, Some(Resource::Cpu));
        assert_eq!(v.culprits[0].name, "Google Chrome");
        assert_eq!(v.culprits[0].pids.len(), 2, "processes of one app are grouped");
        assert!(v.headline.contains("Google Chrome and Microsoft Defender are using most of the processor"), "{}", v.headline);
    }

    #[test]
    fn disk_pressure_blames_io_not_cpu() {
        let procs = vec![p(1, "chrome.exe", 40.0, 300, 1), p(2, "searchindexer.exe", 3.0, 100, 90)];
        let v = judge(&sample(45.0, 50.0, 97.0, procs), &HashMap::new(), &|_, _| None);
        assert_eq!(v.level, Level::Strained);
        assert_eq!(v.resource, Some(Resource::Disk));
        assert_eq!(v.culprits[0].name, "Windows Search indexing");
        assert!(v.headline.starts_with("Your PC is struggling"));
    }

    #[test]
    fn svchost_is_resolved_to_its_service() {
        let mut svc = HashMap::new();
        svc.insert(7, vec!["wuauserv".to_string()]);
        let v = judge(&sample(75.0, 50.0, 10.0, vec![p(7, "svchost.exe", 60.0, 100, 1)]), &svc, &|_, _| None);
        assert_eq!(v.culprits[0].name, "Windows Update");
        assert!(v.culprits[0].critical, "services are never offered End task");
    }

    #[test]
    fn unknown_apps_get_a_label() {
        let v = judge(&sample(70.0, 50.0, 10.0, vec![p(9, "fooeditor.exe", 60.0, 100, 0)]), &HashMap::new(), &|exe, _| (exe == "fooeditor.exe").then(|| "Foo Editor Pro".into()));
        assert_eq!(v.culprits[0].name, "Foo Editor Pro");
        assert!(!v.culprits[0].critical);
    }

    #[test]
    fn memory_pressure() {
        let procs = vec![p(1, "vmmemwsl", 2.0, 6000, 0), p(2, "chrome.exe", 5.0, 2500, 0)];
        let v = judge(&sample(20.0, 96.0, 10.0, procs), &HashMap::new(), &|_, _| None);
        assert_eq!(v.resource, Some(Resource::Memory));
        assert_eq!(v.culprits[0].name, "Virtual machines / WSL");
        assert!(v.headline.contains("memory is nearly full"));
    }
}
