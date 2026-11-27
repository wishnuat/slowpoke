//! Pure diagnosis: walk the path from the PC outward and blame the first unhealthy hop.

use crate::net::probe::{PathStats, Wifi};
use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum Health {
    Good,
    Degraded,
    Down,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum Layer {
    Link,
    Router,
    Internet,
    Dns,
    Traffic,
}

impl Layer {
    pub fn code(self) -> u8 {
        match self {
            Layer::Link => 1,
            Layer::Router => 2,
            Layer::Internet => 3,
            Layer::Dns => 4,
            Layer::Traffic => 5,
        }
    }
    pub fn from_code(c: u8) -> Option<Self> {
        Some(match c {
            1 => Layer::Link,
            2 => Layer::Router,
            3 => Layer::Internet,
            4 => Layer::Dns,
            5 => Layer::Traffic,
            _ => return None,
        })
    }
}

/// Everything measured in one tick.
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub t: u64,
    pub connected: bool,
    pub interface: String,
    pub wireless: bool,
    /// bits per second
    pub link_speed: u64,
    pub wifi: Option<Wifi>,
    pub gateway: Option<String>,
    pub router: PathStats,
    pub internet: PathStats,
    /// last DNS lookup time, ms
    pub dns_ms: Option<f32>,
    /// failed DNS lookups in the recent window, 0..100
    pub dns_fail: f32,
    pub dns_samples: usize,
    /// bits per second on the active interface
    pub rx_bps: u64,
    pub tx_bps: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Hop {
    pub layer: Layer,
    pub label: String,
    pub health: Health,
    pub value: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    pub t: u64,
    pub health: Health,
    pub layer: Option<Layer>,
    pub headline: String,
    pub advice: String,
    pub hops: Vec<Hop>,
    pub snapshot: Snapshot,
}

fn ms(v: Option<f32>) -> String {
    v.map(|x| if x < 1.0 { "<1 ms".to_string() } else { format!("{:.0} ms", x) }).unwrap_or_else(|| "–".into())
}

pub fn mbps(bps: u64) -> String {
    let m = bps as f64 / 1e6;
    if m >= 10.0 { format!("{m:.0} Mbps") } else { format!("{m:.1} Mbps") }
}

pub fn judge(s: &Snapshot) -> Verdict {
    let mut hops: Vec<Hop> = Vec::new();
    // (health, layer, headline, advice) of the first problem found, walking outward
    let mut problem: Option<(Health, Layer, String, String)> = None;
    let flag = |p: &mut Option<(Health, Layer, String, String)>, h: Health, l: Layer, head: String, adv: &str| {
        if p.is_none() || p.as_ref().is_some_and(|x| h > x.0 && x.1 == l) {
            *p = Some((h, l, head, adv.to_string()));
        }
    };

    // ---- link
    if !s.connected {
        hops.push(Hop { layer: Layer::Link, label: "Network".into(), health: Health::Down, value: "not connected".into() });
        flag(&mut problem, Health::Down, Layer::Link, "You're not connected to any network.".into(), "Turn on Wi-Fi and pick a network, or plug in a cable. If Wi-Fi is on, toggling airplane mode often wakes it up.");
    } else if s.wireless {
        let w = s.wifi.clone().unwrap_or_default();
        let mut h = Health::Good;
        if w.quality < 25 {
            h = Health::Down;
            flag(&mut problem, Health::Degraded, Layer::Link, format!("Your Wi-Fi signal is very weak ({} %).", w.quality), "Move closer to the router or remove obstacles (walls, metal, aquariums). A Wi-Fi extender or mesh helps for far rooms.");
        } else if w.quality < 45 {
            h = Health::Degraded;
            let adv = if w.band.as_deref() == Some("2.4 GHz") { "Move closer to the router. Your laptop is on 2.4 GHz: if your router offers a 5 GHz network, it's usually much faster nearby." } else { "Move closer to the router or remove obstacles between you and it." };
            flag(&mut problem, Health::Degraded, Layer::Link, format!("Weak Wi-Fi signal ({} %).", w.quality), adv);
        } else if w.rx_rate > 0 && w.rx_rate < 20 {
            h = Health::Degraded;
            flag(&mut problem, Health::Degraded, Layer::Link, format!("Your Wi-Fi link is slow ({} Mbps).", w.rx_rate), "The connection to the router negotiated a low speed. Moving closer, or switching to the 5 GHz network, usually fixes it.");
        }
        let band = w.band.as_ref().map(|b| format!(" · {b}")).unwrap_or_default();
        hops.push(Hop { layer: Layer::Link, label: if w.ssid.is_empty() { "Wi-Fi".into() } else { format!("Wi-Fi “{}”", w.ssid) }, health: h, value: format!("signal {} %{band}", w.quality) });
    } else {
        hops.push(Hop { layer: Layer::Link, label: "Ethernet".into(), health: Health::Good, value: if s.link_speed > 0 { mbps(s.link_speed) } else { "connected".into() } });
    }

    // ---- router
    if s.connected {
        let r = s.router;
        let i = s.internet;
        let internet_ok = i.samples >= 3 && i.loss < 50.0;
        let (h, value) = if s.gateway.is_none() {
            (Health::Down, "no gateway".to_string())
        } else if r.samples < 3 {
            (Health::Good, "measuring…".to_string())
        } else if r.loss >= 99.0 && internet_ok {
            (Health::Good, "doesn't answer ping (normal for some routers)".to_string())
        } else if r.loss >= 99.0 {
            (Health::Down, "no reply".to_string())
        } else if r.loss >= 15.0 || r.latency.unwrap_or(0.0) > 100.0 || r.jitter.unwrap_or(0.0) > 60.0 {
            (Health::Degraded, format!("{} · {:.0} % loss", ms(r.latency), r.loss))
        } else {
            (Health::Good, ms(r.latency))
        };
        match h {
            Health::Down if s.gateway.is_none() => flag(&mut problem, Health::Down, Layer::Router, "Connected, but there's no router on this network.".into(), "The network didn't give your PC a gateway. Reconnect to the network, or restart the router."),
            Health::Down => flag(&mut problem, Health::Down, Layer::Router, "Your router isn't responding.".into(), "Restart the router: unplug it for 30 seconds. If you're on Wi-Fi, check you're on the right network."),
            Health::Degraded => {
                let head = if r.loss >= 15.0 { format!("The connection to your router is dropping packets ({:.0} % loss).", r.loss) } else { format!("Your router is slow to respond ({}).", ms(r.latency)) };
                let adv = if s.wireless { "This is usually Wi-Fi interference or distance. Move closer, or switch to the 5 GHz network. If it persists, restart the router." } else { "Restart the router. If it keeps happening, the router may be overloaded or overheating." };
                flag(&mut problem, Health::Degraded, Layer::Router, head, adv)
            }
            Health::Good => {}
        }
        hops.push(Hop { layer: Layer::Router, label: s.gateway.clone().map(|g| format!("Router {g}")).unwrap_or_else(|| "Router".into()), health: h, value });
    }

    // ---- internet
    if s.connected {
        let i = s.internet;
        let dns_dead = s.dns_samples >= 2 && s.dns_fail >= 99.0;
        let (h, value) = if i.samples < 3 {
            (Health::Good, "measuring…".to_string())
        } else if i.loss >= 99.0 && (dns_dead || s.dns_samples < 2) {
            (Health::Down, "no reply".to_string())
        } else if i.loss >= 99.0 {
            (Health::Good, "ping blocked, DNS works".to_string())
        } else if i.loss >= 10.0 || i.latency.unwrap_or(0.0) > 150.0 || i.jitter.unwrap_or(0.0) > 50.0 {
            (Health::Degraded, format!("{} · {:.0} % loss · jitter {}", ms(i.latency), i.loss, ms(i.jitter)))
        } else {
            (Health::Good, format!("{} · {:.0} % loss", ms(i.latency), i.loss))
        };
        match h {
            Health::Down => flag(&mut problem, Health::Down, Layer::Internet, "Your router works, but the internet isn't reachable.".into(), "This points to your internet provider or the router's internet connection. Restart the modem/router. If it stays down, check your provider's status or call them."),
            Health::Degraded => {
                let head = if i.loss >= 10.0 { format!("Your internet provider is having trouble ({:.0} % packet loss).", i.loss) } else if i.latency.unwrap_or(0.0) > 150.0 { format!("Your internet connection is slow to respond ({}).", ms(i.latency)) } else { format!("Your internet connection is unstable (jitter {}).", ms(i.jitter)) };
                // heavy local traffic explains high latency better than the ISP does
                if s.rx_bps + s.tx_bps > 8_000_000 && i.loss < 10.0 {
                    flag(&mut problem, Health::Degraded, Layer::Traffic, format!("Your connection is busy: something is transferring {} down / {} up.", mbps(s.rx_bps), mbps(s.tx_bps)), "A download, update, backup or cloud sync is filling the connection, which slows everything else. Pause it, or schedule it for later. (Windows can't say which app without administrator rights.)");
                } else {
                    flag(&mut problem, Health::Degraded, Layer::Internet, head, "Your Wi-Fi and router look fine, so the problem is beyond them. It's often temporary. If it lasts, restart the modem and check your provider's status page.");
                }
            }
            Health::Good => {}
        }
        hops.push(Hop { layer: Layer::Internet, label: "Internet".into(), health: h, value });
    }

    // ---- DNS
    if s.connected {
        let (h, value) = if s.dns_samples < 1 {
            (Health::Good, "measuring…".to_string())
        } else if s.dns_fail >= 50.0 {
            (Health::Down, format!("{:.0} % of lookups failed", s.dns_fail))
        } else if s.dns_ms.unwrap_or(0.0) > 300.0 {
            (Health::Degraded, ms(s.dns_ms))
        } else {
            (Health::Good, ms(s.dns_ms))
        };
        match h {
            Health::Down => flag(&mut problem, Health::Degraded, Layer::Dns, "Websites can't be looked up: DNS isn't answering.".into(), "Your connection works but name lookups fail. Set DNS to 1.1.1.1 or 8.8.8.8 in Settings → Network → your connection → DNS server assignment."),
            Health::Degraded => flag(&mut problem, Health::Degraded, Layer::Dns, format!("DNS is slow ({}), so every website starts late.", ms(s.dns_ms)), "Switch DNS to 1.1.1.1 or 8.8.8.8 in Settings → Network → your connection → DNS server assignment."),
            Health::Good => {}
        }
        hops.push(Hop { layer: Layer::Dns, label: "DNS".into(), health: h, value });
    }

    let (health, layer, headline, advice) = match problem {
        Some((h, l, head, adv)) => (h, Some(l), head, adv),
        None => {
            let mut bits = Vec::new();
            if s.wireless {
                if let Some(w) = &s.wifi {
                    bits.push(format!("Wi-Fi {} %", w.quality));
                }
            }
            if let Some(l) = s.router.latency {
                bits.push(format!("router {}", ms(Some(l))));
            }
            if let Some(l) = s.internet.latency {
                bits.push(format!("internet {}", ms(Some(l))));
            }
            if let Some(d) = s.dns_ms {
                bits.push(format!("DNS {}", ms(Some(d))));
            }
            (Health::Good, None, if bits.is_empty() { "All good.".into() } else { format!("All good: {}.", bits.join(", ")) }, "Nothing to fix. Slowpoke keeps an eye on the connection and remembers any bad moments.".into())
        }
    };
    // a problem on the first hop can make later hops look bad too: mark them as "affected", not separate faults
    Verdict { t: s.t, health, layer, headline, advice, hops, snapshot: s.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(lat: f32, loss: f32, jitter: f32) -> PathStats {
        PathStats { latency: Some(lat), jitter: Some(jitter), loss, samples: 10 }
    }

    fn good() -> Snapshot {
        Snapshot {
            t: 1,
            connected: true,
            interface: "Wi-Fi".into(),
            wireless: true,
            link_speed: 400_000_000,
            wifi: Some(Wifi { ssid: "Home".into(), quality: 82, rssi: Some(-55), rx_rate: 400, tx_rate: 300, channel: Some(36), band: Some("5 GHz".into()) }),
            gateway: Some("192.168.1.1".into()),
            router: stats(3.0, 0.0, 1.0),
            internet: stats(18.0, 0.0, 2.0),
            dns_ms: Some(24.0),
            dns_fail: 0.0,
            dns_samples: 4,
            rx_bps: 200_000,
            tx_bps: 50_000,
        }
    }

    #[test]
    fn all_good() {
        let v = judge(&good());
        assert_eq!(v.health, Health::Good);
        assert_eq!(v.headline, "All good: Wi-Fi 82 %, router 3 ms, internet 18 ms, DNS 24 ms.");
        assert_eq!(v.hops.len(), 4);
        assert!(v.hops.iter().all(|h| h.health == Health::Good));
    }

    #[test]
    fn weak_wifi_is_blamed_first_even_if_everything_after_suffers() {
        let mut s = good();
        s.wifi.as_mut().unwrap().quality = 32;
        s.wifi.as_mut().unwrap().band = Some("2.4 GHz".into());
        s.router = stats(140.0, 20.0, 80.0);
        s.internet = stats(190.0, 20.0, 80.0);
        let v = judge(&s);
        assert_eq!(v.layer, Some(Layer::Link));
        assert_eq!(v.headline, "Weak Wi-Fi signal (32 %).");
        assert!(v.advice.contains("5 GHz"));
    }

    #[test]
    fn slow_router() {
        let mut s = good();
        s.router = stats(180.0, 0.0, 10.0);
        s.internet = stats(200.0, 0.0, 10.0);
        let v = judge(&s);
        assert_eq!((v.health, v.layer), (Health::Degraded, Some(Layer::Router)));
        assert_eq!(v.headline, "Your router is slow to respond (180 ms).");
    }

    #[test]
    fn isp_trouble_when_router_is_fine() {
        let mut s = good();
        s.internet = stats(60.0, 40.0, 20.0);
        let v = judge(&s);
        assert_eq!(v.layer, Some(Layer::Internet));
        assert_eq!(v.headline, "Your internet provider is having trouble (40 % packet loss).");
    }

    #[test]
    fn busy_connection_is_not_blamed_on_the_isp() {
        let mut s = good();
        s.internet = stats(240.0, 0.0, 30.0);
        s.rx_bps = 45_000_000;
        let v = judge(&s);
        assert_eq!(v.layer, Some(Layer::Traffic));
        assert!(v.headline.contains("45 Mbps down"));
        assert!(v.advice.contains("administrator"), "honest about per-app limits");
    }

    #[test]
    fn internet_down_and_router_that_ignores_ping() {
        let mut s = good();
        s.router = PathStats { latency: None, jitter: None, loss: 100.0, samples: 10 };
        s.internet = stats(20.0, 0.0, 2.0);
        assert_eq!(judge(&s).health, Health::Good, "routers that drop ping are fine when the internet works");

        let mut s = good();
        s.internet = PathStats { latency: None, jitter: None, loss: 100.0, samples: 10 };
        s.dns_fail = 100.0;
        let v = judge(&s);
        assert_eq!((v.health, v.layer), (Health::Down, Some(Layer::Internet)));
    }

    #[test]
    fn slow_and_broken_dns() {
        let mut s = good();
        s.dns_ms = Some(900.0);
        let v = judge(&s);
        assert_eq!(v.layer, Some(Layer::Dns));
        assert_eq!(v.headline, "DNS is slow (900 ms), so every website starts late.");
        s.dns_fail = 75.0;
        assert!(judge(&s).headline.contains("DNS isn't answering"));
    }

    #[test]
    fn ethernet_and_disconnected() {
        let mut s = good();
        s.wireless = false;
        s.wifi = None;
        s.link_speed = 1_000_000_000;
        let v = judge(&s);
        assert_eq!(v.hops[0].label, "Ethernet");
        assert_eq!(v.hops[0].value, "1000 Mbps");
        assert_eq!(v.health, Health::Good);

        let off = Snapshot { connected: false, ..Default::default() };
        let v = judge(&off);
        assert_eq!((v.health, v.layer), (Health::Down, Some(Layer::Link)));
        assert_eq!(v.hops.len(), 1);
    }
}
