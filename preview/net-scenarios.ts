// Fictional network states, shaped exactly like the Rust Verdict.
const jitter = (base: number, amp: number, t: number) => Math.max(0.5, base + amp * Math.sin(t * 1.7) + amp * 0.5 * Math.sin(t * 4.3));

export function verdictFor(scenario: string, t: number) {
  const wifiQ = scenario === "wifi" ? Math.round(31 + 4 * Math.sin(t)) : Math.round(86 + 3 * Math.sin(t / 3));
  const routerMs = scenario === "wifi" ? jitter(130, 40, t) : jitter(3, 1, t);
  const inetMs = scenario === "wifi" ? jitter(160, 50, t) : scenario === "isp" ? jitter(95, 40, t) : jitter(19, 3, t);
  const inetLoss = scenario === "isp" ? 40 : scenario === "wifi" ? 10 : 0;
  const dnsMs = scenario === "dns" ? 880 + 60 * Math.sin(t) : jitter(16, 4, t);
  const snapshot = {
    t: Date.now(), connected: true, interface: "Wi-Fi", wireless: true, linkSpeed: 433_000_000,
    wifi: { ssid: "Home-5G", quality: wifiQ, rssi: scenario === "wifi" ? -81 : -52, rxRate: scenario === "wifi" ? 24 : 433, txRate: scenario === "wifi" ? 18 : 390, channel: scenario === "wifi" ? 6 : 44, band: scenario === "wifi" ? "2.4 GHz" : "5 GHz" },
    gateway: "192.168.1.1",
    router: { latency: routerMs, jitter: scenario === "wifi" ? 55 : 1, loss: scenario === "wifi" ? 10 : 0, samples: 10 },
    internet: { latency: inetMs, jitter: scenario === "isp" ? 38 : 3, loss: inetLoss, samples: 10 },
    dnsMs, dnsFail: 0, dnsSamples: 6,
    rxBps: Math.round((1.2 + Math.abs(Math.sin(t / 2)) * 2.5) * 1e6), txBps: 180_000,
  };
  const ok = (v: string) => ({ health: "good", value: v });
  const hop = (layer: string, label: string, h: { health: string; value: string }) => ({ layer, label, ...h });
  const ms = (v: number) => `${Math.round(v)} ms`;
  const hops = [
    hop("link", `Wi-Fi “${snapshot.wifi.ssid}”`, scenario === "wifi" ? { health: "degraded", value: `signal ${wifiQ} % · 2.4 GHz` } : ok(`signal ${wifiQ} % · 5 GHz`)),
    hop("router", "Router 192.168.1.1", scenario === "wifi" ? { health: "degraded", value: `${ms(routerMs)} · 10 % loss` } : ok(ms(routerMs))),
    hop("internet", "Internet", scenario === "isp" ? { health: "degraded", value: `${ms(inetMs)} · 40 % loss · jitter 38 ms` } : scenario === "wifi" ? { health: "degraded", value: `${ms(inetMs)} · 10 % loss` } : ok(`${ms(inetMs)} · 0 % loss`)),
    hop("dns", "DNS", scenario === "dns" ? { health: "degraded", value: ms(dnsMs) } : ok(ms(dnsMs))),
  ];
  const map: Record<string, [string, string | null, string, string]> = {
    good: ["good", null, `All good: Wi-Fi ${wifiQ} %, router ${ms(routerMs)}, internet ${ms(inetMs)}, DNS ${ms(dnsMs)}.`, "Nothing to fix. Slowpoke keeps an eye on the connection and remembers any bad moments."],
    wifi: ["degraded", "link", `Weak Wi-Fi signal (${wifiQ} %).`, "Move closer to the router. Your laptop is on 2.4 GHz: if your router offers a 5 GHz network, it's usually much faster nearby."],
    isp: ["degraded", "internet", "Your internet provider is having trouble (40 % packet loss).", "Your Wi-Fi and router look fine, so the problem is beyond them. It's often temporary. If it lasts, restart the modem and check your provider's status page."],
    dns: ["degraded", "dns", `DNS is slow (${ms(dnsMs)}), so every website starts late.`, "Switch DNS to 1.1.1.1 or 8.8.8.8 in Settings → Network → your connection → DNS server assignment."],
  };
  const [health, layer, headline, advice] = map[scenario] ?? map.good;
  return { t: Date.now(), health, layer, headline, advice, hops, snapshot };
}
