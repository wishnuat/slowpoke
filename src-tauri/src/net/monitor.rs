//! The sampling loop state: owns the probes and turns each tick into a Snapshot.

use crate::net::probe::{self, Pinger, WlanClient, Window};
use crate::net::verdict::Snapshot;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

pub const PUBLIC_HOST: Ipv4Addr = Ipv4Addr::new(1, 1, 1, 1);
pub const TICK: Duration = Duration::from_secs(3);
const DNS_EVERY: Duration = Duration::from_secs(15);

pub struct Monitor {
    pinger: Option<Pinger>,
    wlan: Option<WlanClient>,
    router: Window,
    internet: Window,
    dns: Window,
    dns_last: Option<f32>,
    dns_at: Option<Instant>,
    route_key: Option<(Option<Ipv4Addr>, u32)>,
    octets: Option<(u64, u64, Instant)>,
}

impl Default for Monitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Monitor {
    pub fn new() -> Self {
        Self {
            pinger: Pinger::new(),
            wlan: WlanClient::open(),
            router: Window::new(10),
            internet: Window::new(10),
            dns: Window::new(6),
            dns_last: None,
            dns_at: None,
            route_key: None,
            octets: None,
        }
    }

    pub fn tick(&mut self) -> Snapshot {
        let mut s = Snapshot { t: crate::now_ms(), ..Default::default() };
        let route = probe::default_route();
        let key = route.as_ref().map(|r| (r.gateway, r.if_index));
        if key != self.route_key {
            // network changed: old samples describe another path
            self.router.clear();
            self.internet.clear();
            self.dns.clear();
            self.octets = None;
            self.route_key = key;
            self.dns_at = None;
        }
        let Some(route) = route else { return s };
        let Some(iface) = probe::interface(route.if_index).filter(|i| i.up) else { return s };
        s.connected = true;
        s.interface = iface.name.clone();
        s.wireless = iface.wireless;
        s.link_speed = iface.link_speed;
        s.gateway = route.gateway.map(|g| g.to_string());
        if iface.wireless {
            s.wifi = self.wlan.as_ref().and_then(|w| w.current());
        }

        // throughput from interface octet counters
        let now = Instant::now();
        if let Some((rx0, tx0, t0)) = self.octets {
            let dt = now.duration_since(t0).as_secs_f64().max(0.1);
            s.rx_bps = ((iface.in_octets.saturating_sub(rx0)) as f64 * 8.0 / dt) as u64;
            s.tx_bps = ((iface.out_octets.saturating_sub(tx0)) as f64 * 8.0 / dt) as u64;
        }
        self.octets = Some((iface.in_octets, iface.out_octets, now));

        if let Some(p) = &self.pinger {
            if let Some(gw) = route.gateway {
                self.router.push(p.ping(gw, 1000));
            }
            self.internet.push(p.ping(PUBLIC_HOST, 1500));
        }
        s.router = self.router.stats();
        s.internet = self.internet.stats();

        if self.dns_at.is_none_or(|t| t.elapsed() >= DNS_EVERY) {
            let r = probe::dns_time(Duration::from_secs(4));
            self.dns.push(r);
            self.dns_last = r.or(self.dns_last.filter(|_| false));
            self.dns_at = Some(Instant::now());
        }
        let d = self.dns.stats();
        s.dns_ms = self.dns_last;
        s.dns_fail = d.loss;
        s.dns_samples = d.samples;
        s
    }
}
