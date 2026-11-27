//! One answer to "why is everything slow?": weigh the PC verdict against the network verdict, blame the worse
//! one, and say plainly that the other side is fine (that's often the most useful half of the answer).

use crate::net::verdict::{Health, Verdict as NetVerdict};
use crate::verdict::{Level, Verdict as PcVerdict};
use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Area {
    Pc,
    Network,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Overall {
    pub level: Level,
    /// where the problem is; None when everything is fine
    pub area: Option<Area>,
    pub headline: String,
    pub detail: String,
    /// one line about the other side: a second problem, or the reassurance that it's fine
    pub also: Option<String>,
}

pub fn rank(l: Level) -> u8 {
    match l {
        Level::Calm => 0,
        Level::Busy => 1,
        Level::Strained => 2,
    }
}

fn net_level(h: Health) -> Level {
    match h {
        Health::Good => Level::Calm,
        Health::Degraded => Level::Busy,
        Health::Down => Level::Strained,
    }
}

/// Severity on one scale. A degraded connection outranks a merely busy PC (a busy PC is often normal),
/// a struggling PC outranks a degraded connection, and no connection at all outranks everything.
fn pc_score(l: Level) -> u8 {
    [0, 2, 4][rank(l) as usize]
}
fn net_score(h: Health) -> u8 {
    match h {
        Health::Good => 0,
        Health::Degraded => 3,
        Health::Down => 5,
    }
}

fn pc_fine(pc: &PcVerdict) -> String {
    format!("Your PC itself is calm (processor {:.0} %, memory {:.0} %, disk {:.0} %).", pc.cpu, pc.mem_pct, pc.disk)
}

fn net_fine(net: &NetVerdict) -> String {
    match net.snapshot.internet.latency {
        Some(ms) => format!("The internet connection is fine ({:.0} ms to the internet).", ms.round()),
        None => "The internet connection is fine.".into(),
    }
}

pub fn combine(pc: &PcVerdict, net: Option<&NetVerdict>) -> Overall {
    let Some(net) = net else {
        // network not measured yet: the PC alone
        return Overall { level: pc.level, area: (pc.level != Level::Calm).then_some(Area::Pc), headline: pc.headline.clone(), detail: pc.detail.clone(), also: None };
    };
    let (ps, ns) = (pc_score(pc.level), net_score(net.health));
    let level = if rank(net_level(net.health)) > rank(pc.level) { net_level(net.health) } else { pc.level };

    if ps == 0 && ns == 0 {
        let ms = net.snapshot.internet.latency.map(|m| format!(", internet {:.0} ms", m.round())).unwrap_or_default();
        return Overall {
            level,
            area: None,
            headline: "All good: your PC is calm and the internet is healthy.".into(),
            detail: format!("Processor {:.0} % · memory {:.0} % · disk {:.0} %{ms}", pc.cpu, pc.mem_pct, pc.disk),
            also: None,
        };
    }
    if ns > ps {
        Overall {
            level,
            area: Some(Area::Network),
            headline: net.headline.clone(),
            detail: net.advice.clone(),
            also: Some(if ps == 0 { pc_fine(pc) } else { format!("Also: {}", pc.headline) }),
        }
    } else {
        Overall {
            level,
            area: Some(Area::Pc),
            headline: pc.headline.clone(),
            detail: pc.detail.clone(),
            also: Some(if ns == 0 { net_fine(net) } else { format!("Also: {}", net.headline) }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::probe::PathStats;
    use crate::net::verdict::{Layer, Snapshot};

    fn pc(level: Level) -> PcVerdict {
        let headline = match level {
            Level::Calm => "All calm. Nothing is straining your PC right now.",
            Level::Busy => "Your PC is busy: Google Chrome is using most of the processor.",
            Level::Strained => "Your PC is struggling: Windows Update is keeping the disk busy.",
        };
        PcVerdict { t: 0, level, resource: None, headline: headline.into(), detail: "CPU at 80 %".into(), cpu: 12.0, mem_pct: 40.0, commit_pct: 30.0, disk: 5.0, mem_used: 0, mem_total: 0, culprits: vec![] }
    }

    fn net(health: Health) -> NetVerdict {
        let headline = match health {
            Health::Good => "All good: Wi-Fi 84 %, router 4 ms, internet 23 ms, DNS 22 ms.",
            Health::Degraded => "Weak Wi-Fi signal (28 %).",
            Health::Down => "Your router works, but the internet isn't reachable.",
        };
        let snapshot = Snapshot { internet: PathStats { latency: Some(23.4), jitter: Some(2.0), loss: 0.0, samples: 10 }, ..Default::default() };
        NetVerdict { t: 0, health, layer: (health != Health::Good).then_some(Layer::Link), headline: headline.into(), advice: "Move closer to the router.".into(), hops: vec![], snapshot }
    }

    #[test]
    fn everything_fine() {
        let o = combine(&pc(Level::Calm), Some(&net(Health::Good)));
        assert_eq!(o.area, None);
        assert_eq!(o.level, Level::Calm);
        assert!(o.headline.starts_with("All good"));
        assert!(o.detail.contains("internet 23 ms"), "{}", o.detail);
    }

    #[test]
    fn weak_wifi_on_a_calm_pc_says_the_pc_is_fine() {
        let o = combine(&pc(Level::Calm), Some(&net(Health::Degraded)));
        assert_eq!(o.area, Some(Area::Network));
        assert_eq!(o.level, Level::Busy);
        assert_eq!(o.headline, "Weak Wi-Fi signal (28 %).");
        assert!(o.also.unwrap().starts_with("Your PC itself is calm"));
    }

    #[test]
    fn struggling_pc_with_good_internet() {
        let o = combine(&pc(Level::Strained), Some(&net(Health::Good)));
        assert_eq!(o.area, Some(Area::Pc));
        assert_eq!(o.level, Level::Strained);
        assert_eq!(o.also.unwrap(), "The internet connection is fine (23 ms to the internet).");
    }

    #[test]
    fn degraded_network_beats_a_merely_busy_pc_but_not_a_struggling_one() {
        let o = combine(&pc(Level::Busy), Some(&net(Health::Degraded)));
        assert_eq!(o.area, Some(Area::Network));
        assert!(o.also.unwrap().starts_with("Also: Your PC is busy"));
        let o = combine(&pc(Level::Strained), Some(&net(Health::Degraded)));
        assert_eq!(o.area, Some(Area::Pc));
        assert!(o.also.unwrap().starts_with("Also: Weak Wi-Fi"));
    }

    #[test]
    fn no_internet_wins_and_sets_the_level() {
        let o = combine(&pc(Level::Strained), Some(&net(Health::Down)));
        assert_eq!(o.area, Some(Area::Network));
        assert_eq!(o.level, Level::Strained);
    }

    #[test]
    fn before_the_first_network_check() {
        let o = combine(&pc(Level::Busy), None);
        assert_eq!(o.area, Some(Area::Pc));
        assert_eq!(o.also, None);
    }
}
