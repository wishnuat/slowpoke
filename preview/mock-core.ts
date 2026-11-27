// Browser-only stand-in for @tauri-apps/api/core (UI review, screenshots, demo video). Fictional but realistic data.
// ?scenario=pc (busy disk, internet fine) | wifi (calm PC, weak Wi-Fi) | isp | calm
// The demo video switches scenarios live through window.__setScenario.
import { verdictFor } from "./net-scenarios";

const GB = 1024 ** 3, MB = 1024 ** 2;
const now = Math.floor(Date.now() / 1000);
const c = (key: string, name: string, what: string, advice: string, pids: number, cpu: number, mem: number, io: number, share: number, critical = false, services: string[] = []) =>
  ({ key, exe: key, name, what, advice, pids: Array.from({ length: pids }, (_, i) => 1000 + i), cpu, mem, io, share, critical, services });

const CULPRITS = [
  c("msmpeng.exe", "Microsoft Defender", "Windows' built-in antivirus, scanning files.", "Normal during scheduled scans and after installing or extracting many files. It slows down by itself.", 1, 14.2, 310 * MB, 61 * MB, 0.52, true),
  c("svc:Windows Update", "Windows Update", "Downloads and installs updates.", "A Windows service. It normally settles down by itself.", 2, 9.1, 180 * MB, 38 * MB, 0.32, true, ["wuauserv", "BITS"]),
  c("chrome.exe", "Google Chrome", "Every tab and extension runs as its own process.", "Close heavy tabs. Chrome's own Task Manager (Shift+Esc) shows which tab is busy.", 27, 8.3, 2.9 * GB, 6 * MB, 0.05),
  c("onedrive.exe", "OneDrive", "Syncs your files with the cloud.", "Busy while uploading or downloading many files. You can pause syncing from its tray icon.", 1, 2.1, 140 * MB, 5 * MB, 0.04),
  c("ms-teams.exe", "Microsoft Teams", "Chats, calls and meetings.", "Video calls are heavy by nature. Turning off background effects helps.", 6, 3.4, 820 * MB, 1 * MB, 0.01),
];

export const scenario = () => (window as any).__scenario ?? new URLSearchParams(location.search).get("scenario") ?? "pc";

export function pcVerdict(sc: string, t: number) {
  const w = (f: number, p: number) => 0.5 + 0.5 * Math.sin(t / f + p);
  if (sc === "pc") {
    return {
      t: Date.now() + t, level: "busy", resource: "disk",
      headline: "Your PC is busy: Microsoft Defender and Windows Update are keeping the disk busy.",
      detail: "Disk busy 88 % of the time", cpu: 38 + 6 * w(3, 0), memPct: 71 + 2 * w(7, 1), commitPct: 55, disk: 86 + 5 * w(2, 2), memUsed: 11.4 * GB, memTotal: 16 * GB,
      culprits: CULPRITS,
    };
  }
  const cpu = 11 + 6 * w(3, 0), mem = 46 + 2 * w(7, 1), disk = 3 + 3 * w(2, 2);
  return {
    t: Date.now() + t, level: "calm", resource: null,
    headline: "All calm. Nothing is straining your PC right now.",
    detail: `CPU ${cpu.toFixed(0)} % · memory ${mem.toFixed(0)} % · disk ${disk.toFixed(0)} %`, cpu, memPct: mem, commitPct: 40, disk, memUsed: 7.4 * GB, memTotal: 16 * GB,
    culprits: CULPRITS.map((x) => ({ ...x, cpu: x.cpu / 4, io: x.io / 20, share: x.share })).sort((a, b) => b.mem - a.mem),
  };
}

/** Same rules as src-tauri/src/overall.rs */
export function overall(pc: any, net: any) {
  const ps = { calm: 0, busy: 2, strained: 4 }[pc.level as string]!;
  const ns = { good: 0, degraded: 3, down: 5 }[net.health as string]!;
  const nl = { good: "calm", degraded: "busy", down: "strained" }[net.health as string]!;
  const rank = (l: string) => ({ calm: 0, busy: 1, strained: 2 })[l]!;
  const level = rank(nl) > rank(pc.level) ? nl : pc.level;
  const lat = Math.round(net.snapshot.internet.latency);
  if (ps === 0 && ns === 0)
    return { level, area: null, headline: "All good: your PC is calm and the internet is healthy.", detail: `Processor ${pc.cpu.toFixed(0)} % · memory ${pc.memPct.toFixed(0)} % · disk ${pc.disk.toFixed(0)} %, internet ${lat} ms`, also: null };
  if (ns > ps)
    return { level, area: "network", headline: net.headline, detail: net.advice, also: ps === 0 ? `Your PC itself is calm (processor ${pc.cpu.toFixed(0)} %, memory ${pc.memPct.toFixed(0)} %, disk ${pc.disk.toFixed(0)} %).` : `Also: ${pc.headline}` };
  return { level, area: "pc", headline: pc.headline, detail: pc.detail, also: ns === 0 ? `The internet connection is fine (${lat} ms to the internet).` : `Also: ${net.headline}` };
}

export function nowFor(t: number) {
  const sc = scenario();
  const pc = pcVerdict(sc, t);
  const net = { ...verdictFor(sc === "pc" || sc === "calm" ? "good" : sc, t), t: Math.floor(t / 1.5) };
  return { verdict: pc, net, overall: overall(pc, net), cost: { cpu: 0.6, mem: 96 * MB, windowOpen: true } };
}
export const NOW = nowFor(0);

function timeline(hours: number) {
  const from = now - hours * 3600, step = Math.max(2, Math.floor((hours * 3600) / 720));
  const points: number[][] = [];
  for (let t = from; t <= now; t += step) {
    const k = (t - from) / (now - from);
    const spike = (s: number, w: number) => Math.exp(-(((k - s) / w) ** 2));
    const cpu = 12 + 8 * Math.sin(t / 900) ** 2 + 70 * spike(0.33, 0.015) + 30 * spike(0.97, 0.02);
    const disk = 6 + 85 * spike(0.97, 0.025) + 60 * spike(0.62, 0.01);
    points.push([t, Math.min(100, Math.round(cpu)), Math.round(58 + 10 * k), Math.min(100, Math.round(disk))]);
  }
  const at = (k: number) => Math.round(from + k * (now - from));
  return {
    from, to: now, points,
    moments: [
      { start: at(0.955), end: at(0.99), resource: "disk", peak: 93, culprits: ["Microsoft Defender", "Windows Update"] },
      { start: at(0.615), end: at(0.628), resource: "disk", peak: 71, culprits: ["Windows Search indexing"] },
      { start: at(0.32), end: at(0.345), resource: "cpu", peak: 91, culprits: ["Microsoft Teams", "Google Chrome"] },
    ],
  };
}

function netTimeline(hours: number) {
  const from = now - hours * 3600, step = Math.max(3, Math.floor((hours * 3600) / 720));
  const pts: number[][] = [];
  for (let t = from; t <= now; t += step) {
    const k = (t - from) / (now - from);
    const bump = (c: number, w: number) => Math.exp(-(((k - c) / w) ** 2));
    const isp = bump(0.42, 0.02), wifi = bump(0.78, 0.015);
    const inet = Math.round(19 + 4 * Math.sin(t / 700) ** 2 + 170 * isp + 60 * wifi);
    const router = Math.round(3 + 2 * Math.sin(t / 300) ** 2 + 90 * wifi);
    const loss = Math.round(45 * isp + 18 * wifi);
    pts.push([t, isp > 0.35 || wifi > 0.35 ? 1 : 0, router, inet, loss, Math.round(14 + 6 * Math.random()), Math.round(84 - 55 * wifi), 12]);
  }
  const at = (k: number) => Math.round(from + k * (now - from));
  return {
    from, to: now, points: pts,
    moments: [
      { start: at(0.77), end: at(0.79), layer: "link", severity: "degraded", worst: "signal down to 29 %" },
      { start: at(0.41), end: at(0.435), layer: "internet", severity: "degraded", worst: "up to 46 % packet loss" },
    ],
  };
}

export async function invoke(cmd: string, args?: any): Promise<any> {
  if (cmd === "get_now") return nowFor(0);
  if (cmd === "get_timeline") return timeline(args.hours);
  if (cmd === "get_net_timeline") return netTimeline(args.hours);
  if (cmd === "get_overlay_prefs") return { opacity: 0.78, clickThrough: false };
  if (cmd === "speed_test") {
    await new Promise((r) => setTimeout(r, 2600));
    return { mbps: 87.4, bytes: 25_000_000, seconds: 2.29, server: "speed.cloudflare.com" };
  }
  return null;
}
