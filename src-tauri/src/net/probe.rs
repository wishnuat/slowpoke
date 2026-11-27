//! Native, admin-free network probes: default route, interface counters, Wi-Fi link, ICMP echo, DNS timing.

use serde::Serialize;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};
use windows::core::GUID;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, GetIpForwardTable2, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho2, ICMP_ECHO_REPLY, MIB_IF_TABLE2, MIB_IPFORWARD_TABLE2,
};
use windows::Win32::NetworkManagement::WiFi::{
    wlan_intf_opcode_channel_number, wlan_intf_opcode_current_connection, wlan_intf_opcode_rssi, WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory,
    WlanOpenHandle, WlanQueryInterface, WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST,
};
use windows::Win32::Networking::WinSock::AF_INET;

// ---------------------------------------------------------------- route + interfaces

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub gateway: Option<Ipv4Addr>,
    pub if_index: u32,
}

/// The IPv4 default route (0.0.0.0/0) with the lowest metric.
pub fn default_route() -> Option<Route> {
    unsafe {
        let mut t: *mut MIB_IPFORWARD_TABLE2 = std::ptr::null_mut();
        if GetIpForwardTable2(AF_INET, &mut t).is_err() || t.is_null() {
            return None;
        }
        let rows = std::slice::from_raw_parts((*t).Table.as_ptr(), (*t).NumEntries as usize);
        let best = rows
            .iter()
            .filter(|r| r.DestinationPrefix.PrefixLength == 0)
            .min_by_key(|r| r.Metric)
            .map(|r| {
                let gw = u32::from_be(r.NextHop.Ipv4.sin_addr.S_un.S_addr.to_be()).to_be_bytes();
                let ip = Ipv4Addr::new(gw[3], gw[2], gw[1], gw[0]);
                Route { gateway: (!ip.is_unspecified()).then_some(ip), if_index: r.InterfaceIndex }
            });
        FreeMibTable(t as *const _);
        best
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IfInfo {
    pub index: u32,
    pub name: String,
    pub wireless: bool,
    /// bits per second (receive)
    pub link_speed: u64,
    pub in_octets: u64,
    pub out_octets: u64,
    pub up: bool,
}

fn wide(buf: &[u16]) -> String {
    let n = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

pub fn interface(index: u32) -> Option<IfInfo> {
    unsafe {
        let mut t: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut t).is_err() || t.is_null() {
            return None;
        }
        let rows = std::slice::from_raw_parts((*t).Table.as_ptr(), (*t).NumEntries as usize);
        let found = rows.iter().find(|r| r.InterfaceIndex == index).map(|r| IfInfo {
            index,
            name: { let a = wide(&r.Alias); if a.is_empty() { wide(&r.Description) } else { a } },
            wireless: r.Type == 71, // IF_TYPE_IEEE80211
            link_speed: r.ReceiveLinkSpeed,
            in_octets: r.InOctets,
            out_octets: r.OutOctets,
            up: r.OperStatus.0 == 1,
        });
        FreeMibTable(t as *const _);
        found
    }
}

// ---------------------------------------------------------------- Wi-Fi

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Wifi {
    pub ssid: String,
    /// 0..100 (Windows' signal quality)
    pub quality: u32,
    pub rssi: Option<i32>,
    /// Mbps
    pub rx_rate: u32,
    pub tx_rate: u32,
    pub channel: Option<u32>,
    /// "2.4 GHz" | "5 GHz" | "6 GHz"
    pub band: Option<String>,
}

pub fn band_of(channel: u32) -> &'static str {
    match channel {
        1..=14 => "2.4 GHz",
        32..=177 => "5 GHz",
        _ => "6 GHz",
    }
}

pub struct WlanClient(HANDLE);

impl WlanClient {
    pub fn open() -> Option<Self> {
        let mut ver = 0u32;
        let mut h = HANDLE::default();
        (unsafe { WlanOpenHandle(2, None, &mut ver, &mut h) } == 0).then_some(Self(h))
    }

    fn query<T: Copy>(&self, guid: &GUID, op: windows::Win32::NetworkManagement::WiFi::WLAN_INTF_OPCODE) -> Option<T> {
        unsafe {
            let mut size = 0u32;
            let mut data: *mut core::ffi::c_void = std::ptr::null_mut();
            if WlanQueryInterface(self.0, guid, op, None, &mut size, &mut data, None) != 0 || data.is_null() {
                return None;
            }
            let v = (size as usize >= std::mem::size_of::<T>()).then(|| *(data as *const T));
            WlanFreeMemory(data);
            v
        }
    }

    /// The connected Wi-Fi interface, if any.
    pub fn current(&self) -> Option<Wifi> {
        unsafe {
            let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
            if WlanEnumInterfaces(self.0, None, &mut list) != 0 || list.is_null() {
                return None;
            }
            let items = std::slice::from_raw_parts((*list).InterfaceInfo.as_ptr(), (*list).dwNumberOfItems as usize);
            let guids: Vec<GUID> = items.iter().map(|i| i.InterfaceGuid).collect();
            WlanFreeMemory(list as *const _);
            for g in guids {
                let Some(conn) = self.query::<WLAN_CONNECTION_ATTRIBUTES>(&g, wlan_intf_opcode_current_connection) else { continue };
                let a = conn.wlanAssociationAttributes;
                let len = (a.dot11Ssid.uSSIDLength as usize).min(32);
                let channel = self.query::<u32>(&g, wlan_intf_opcode_channel_number).filter(|c| *c > 0);
                return Some(Wifi {
                    ssid: String::from_utf8_lossy(&a.dot11Ssid.ucSSID[..len]).into_owned(),
                    quality: a.wlanSignalQuality,
                    rssi: self.query::<i32>(&g, wlan_intf_opcode_rssi),
                    rx_rate: a.ulRxRate / 1000,
                    tx_rate: a.ulTxRate / 1000,
                    channel,
                    band: channel.map(|c| band_of(c).to_string()),
                });
            }
            None
        }
    }
}

impl Drop for WlanClient {
    fn drop(&mut self) {
        unsafe {
            WlanCloseHandle(self.0, None);
        }
    }
}

unsafe impl Send for WlanClient {}

// ---------------------------------------------------------------- ICMP

pub struct Pinger(HANDLE);

unsafe impl Send for Pinger {}

impl Pinger {
    pub fn new() -> Option<Self> {
        unsafe { IcmpCreateFile().ok().map(Self) }
    }

    /// Round-trip time in ms, or None on timeout/unreachable.
    pub fn ping(&self, ip: Ipv4Addr, timeout_ms: u32) -> Option<f32> {
        let payload = *b"netdoctor-probe!";
        let mut reply = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + payload.len() + 64];
        let dest = u32::from_ne_bytes(ip.octets());
        let t0 = Instant::now();
        let n = unsafe {
            IcmpSendEcho2(self.0, None, None, None, dest, payload.as_ptr() as *const _, payload.len() as u16, None, reply.as_mut_ptr() as *mut _, reply.len() as u32, timeout_ms)
        };
        if n == 0 {
            return None;
        }
        let r = unsafe { &*(reply.as_ptr() as *const ICMP_ECHO_REPLY) };
        if r.Status != 0 {
            return None;
        }
        // RoundTripTime has 1 ms resolution; fall back to the wall clock for sub-ms LAN replies
        let wall = t0.elapsed().as_secs_f32() * 1000.0;
        Some(if r.RoundTripTime == 0 { wall.min(1.0) } else { r.RoundTripTime as f32 })
    }
}

impl Drop for Pinger {
    fn drop(&mut self) {
        unsafe {
            let _ = IcmpCloseHandle(self.0);
        }
    }
}

// ---------------------------------------------------------------- DNS

/// The first DNS server Windows is configured to use.
pub fn dns_server() -> Option<Ipv4Addr> {
    use windows::Win32::NetworkManagement::IpHelper::{GetNetworkParams, FIXED_INFO_W2KSP1};
    unsafe {
        let mut len = 0u32;
        let _ = GetNetworkParams(None, &mut len);
        if len == 0 {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        let info = buf.as_mut_ptr() as *mut FIXED_INFO_W2KSP1;
        if GetNetworkParams(Some(info), &mut len).is_err() {
            return None;
        }
        let raw = (*info).DnsServerList.IpAddress.String;
        let n = raw.iter().position(|c| *c == 0).unwrap_or(raw.len());
        let s: String = raw[..n].iter().map(|c| *c as u8 as char).collect();
        s.parse().ok()
    }
}

/// A minimal DNS query for an A record (id, RD flag, one question).
pub fn dns_query(id: u16, name: &str) -> Vec<u8> {
    let mut q = Vec::with_capacity(32 + name.len());
    q.extend(id.to_be_bytes());
    q.extend([0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]); // RD; QDCOUNT=1
    for label in name.split('.') {
        q.push(label.len() as u8);
        q.extend(label.as_bytes());
    }
    q.extend([0, 0, 1, 0, 1]); // root, QTYPE=A, QCLASS=IN
    q
}

/// RCODE of a reply matching `id` (0 = NOERROR), or None if it isn't a reply to us.
pub fn dns_rcode(reply: &[u8], id: u16) -> Option<u8> {
    (reply.len() >= 12 && reply[0..2] == id.to_be_bytes() && reply[2] & 0x80 != 0).then(|| reply[3] & 0x0F)
}

const DNS_NAMES: &[&str] = &["www.google.com", "www.microsoft.com", "www.wikipedia.org", "www.cloudflare.com", "www.amazon.com"];

/// Time one lookup of a popular name straight against the configured DNS server: this skips Windows' own cache
/// but hits the resolver's, which is what everyday browsing experiences. Returns ms, or None on timeout/failure.
pub fn dns_time(timeout: Duration) -> Option<f32> {
    use std::net::UdpSocket;
    let server = dns_server()?;
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.set_read_timeout(Some(timeout)).ok()?;
    let seed = crate::now_ms();
    let id = (seed as u16) ^ 0x5a5a;
    let name = DNS_NAMES[(seed / 1000) as usize % DNS_NAMES.len()];
    let t0 = Instant::now();
    sock.send_to(&dns_query(id, name), (server, 53)).ok()?;
    let mut buf = [0u8; 1500];
    loop {
        let (n, _) = sock.recv_from(&mut buf).ok()?;
        if let Some(rcode) = dns_rcode(&buf[..n], id) {
            return (rcode == 0).then(|| t0.elapsed().as_secs_f32() * 1000.0);
        }
        if t0.elapsed() > timeout {
            return None;
        }
    }
}

// ---------------------------------------------------------------- windows of samples

/// Latency/jitter/loss over a sliding window of echo results.
#[derive(Default, Clone, Debug)]
pub struct Window {
    pub samples: std::collections::VecDeque<Option<f32>>,
    pub cap: usize,
}

#[derive(Serialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PathStats {
    /// median RTT of answered echoes (ms)
    pub latency: Option<f32>,
    /// mean absolute difference between consecutive RTTs (ms)
    pub jitter: Option<f32>,
    /// 0..100
    pub loss: f32,
    pub samples: usize,
}

impl Window {
    pub fn new(cap: usize) -> Self {
        Self { samples: Default::default(), cap }
    }
    pub fn push(&mut self, v: Option<f32>) {
        self.samples.push_back(v);
        while self.samples.len() > self.cap {
            self.samples.pop_front();
        }
    }
    pub fn clear(&mut self) {
        self.samples.clear();
    }
    pub fn stats(&self) -> PathStats {
        let n = self.samples.len();
        if n == 0 {
            return PathStats::default();
        }
        let ok: Vec<f32> = self.samples.iter().flatten().copied().collect();
        let loss = (n - ok.len()) as f32 * 100.0 / n as f32;
        let latency = (!ok.is_empty()).then(|| {
            let mut s = ok.clone();
            s.sort_by(|a, b| a.partial_cmp(b).unwrap());
            s[s.len() / 2]
        });
        let jitter = (ok.len() >= 2).then(|| ok.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / (ok.len() - 1) as f32);
        PathStats { latency, jitter, loss, samples: n }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_stats() {
        let mut w = Window::new(5);
        for v in [Some(10.0), Some(20.0), None, Some(12.0), Some(14.0), Some(16.0)] {
            w.push(v);
        }
        let s = w.stats();
        assert_eq!(s.samples, 5, "capped");
        assert!((s.loss - 20.0).abs() < 1e-4);
        assert_eq!(s.latency, Some(16.0));
        assert!((s.jitter.unwrap() - (8.0 + 2.0 + 2.0) / 3.0).abs() < 1e-4);
        assert_eq!(Window::new(3).stats(), PathStats::default());
    }

    #[test]
    fn dns_packets() {
        let q = dns_query(0x1234, "www.google.com");
        assert_eq!(&q[..4], &[0x12, 0x34, 0x01, 0x00]);
        assert_eq!(&q[12..16], &[3, b'w', b'w', b'w']);
        assert_eq!(&q[q.len() - 5..], &[0, 0, 1, 0, 1]);
        assert_eq!(dns_rcode(&[0x12, 0x34, 0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0], 0x1234), Some(0));
        assert_eq!(dns_rcode(&[0x12, 0x34, 0x81, 0x83, 0, 1, 0, 0, 0, 0, 0, 0], 0x1234), Some(3), "NXDOMAIN");
        assert_eq!(dns_rcode(&[0x99, 0x99, 0x81, 0x80, 0, 0, 0, 0, 0, 0, 0, 0], 0x1234), None, "someone else's reply");
    }

    #[test]
    fn bands() {
        assert_eq!(band_of(6), "2.4 GHz");
        assert_eq!(band_of(36), "5 GHz");
        assert_eq!(band_of(181), "6 GHz");
    }

    /// Live probes against this machine. They skip quietly when offline.
    #[test]
    fn live_probes() {
        let Some(route) = default_route() else {
            eprintln!("offline: no default route, skipping");
            return;
        };
        let iface = interface(route.if_index).expect("default-route interface exists");
        assert!(iface.up);
        assert!(!iface.name.is_empty());
        if let Some(gw) = route.gateway {
            let p = Pinger::new().unwrap();
            let rtts: Vec<Option<f32>> = (0..3).map(|_| p.ping(gw, 1000)).collect();
            eprintln!("gateway {gw}: {rtts:?}");
        }
        if iface.wireless {
            let w = WlanClient::open().and_then(|c| c.current()).expect("Wi-Fi details for a wireless default route");
            assert!(w.quality <= 100);
            eprintln!("wifi {:?}", w);
        }
        eprintln!("dns server {:?}", dns_server());
        let times: Vec<Option<f32>> = (0..3).map(|_| dns_time(Duration::from_secs(3))).collect();
        eprintln!("dns {times:?}");
        assert!(times.iter().any(|t| t.is_some()), "DNS answers while online");
    }
}
