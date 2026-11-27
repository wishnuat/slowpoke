export type Level = "calm" | "busy" | "strained";
export type Resource = "cpu" | "memory" | "disk";

export interface Culprit {
  key: string;
  exe: string;
  name: string;
  what: string;
  advice: string;
  pids: number[];
  cpu: number;
  mem: number;
  io: number;
  share: number;
  critical: boolean;
  services: string[];
}

export interface Verdict {
  t: number;
  level: Level;
  resource: Resource | null;
  headline: string;
  detail: string;
  cpu: number;
  memPct: number;
  commitPct: number;
  disk: number;
  memUsed: number;
  memTotal: number;
  culprits: Culprit[];
}

export interface SelfCost {
  cpu: number;
  mem: number;
  windowOpen: boolean;
}

export interface Now {
  verdict: Verdict;
  net: NetVerdict | null;
  overall: Overall;
  cost: SelfCost;
}

export interface Moment {
  start: number;
  end: number;
  resource: Resource;
  peak: number;
  culprits: string[];
}

export interface Timeline {
  points: [number, number, number, number][];
  moments: Moment[];
  from: number;
  to: number;
}

export const fmtBytes = (b: number) => {
  if (b >= 1024 ** 3) return `${(b / 1024 ** 3).toFixed(1)} GB`;
  if (b >= 1024 ** 2) return `${Math.round(b / 1024 ** 2)} MB`;
  return `${Math.round(b / 1024)} KB`;
};
export const fmtRate = (b: number) => (b >= 1024 ** 2 ? `${(b / 1024 ** 2).toFixed(1)} MB/s` : b >= 1024 ? `${Math.round(b / 1024)} KB/s` : "–");
export const fmtTime = (s: number) => new Date(s * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
export const fmtDay = (s: number) => new Date(s * 1000).toLocaleDateString([], { weekday: "short", day: "numeric", month: "short" });
export const RES_LABEL: Record<Resource, string> = { cpu: "Processor", memory: "Memory", disk: "Disk" };

// ---------------------------------------------------------------- network (shapes match src-tauri/src/net)
export type Health = "good" | "degraded" | "down";
export type Layer = "link" | "router" | "internet" | "dns" | "traffic";
export interface PathStats { latency: number | null; jitter: number | null; loss: number; samples: number }
export interface Wifi { ssid: string; quality: number; rssi: number | null; rxRate: number; txRate: number; channel: number | null; band: string | null }
export interface Snapshot {
  t: number; connected: boolean; interface: string; wireless: boolean; linkSpeed: number; wifi: Wifi | null; gateway: string | null;
  router: PathStats; internet: PathStats; dnsMs: number | null; dnsFail: number; dnsSamples: number; rxBps: number; txBps: number;
}
export interface Hop { layer: Layer; label: string; health: Health; value: string }
export interface NetVerdict { t: number; health: Health; layer: Layer | null; headline: string; advice: string; hops: Hop[]; snapshot: Snapshot }
export interface NetMoment { start: number; end: number; layer: Layer | null; severity: "degraded" | "down"; worst: string }
export interface NetTimeline {
  /** [t, health, routerMs, internetMs, internetLoss, dnsMs, signal, rx(0.1 Mbps)]; 65535 / 255 = none */
  points: number[][];
  moments: NetMoment[];
  from: number;
  to: number;
}
export interface SpeedResult { mbps: number; bytes: number; seconds: number; server: string }

export type Area = "pc" | "network";
export interface Overall { level: Level; area: Area | null; headline: string; detail: string; also: string | null }

export const LAYER_LABEL: Record<Layer, string> = { link: "Wi-Fi / link", router: "Router", internet: "Internet", dns: "DNS", traffic: "Busy connection" };
export const fmtMs = (v: number | null | undefined) => (v == null ? "–" : v < 1 ? "<1 ms" : `${Math.round(v)} ms`);
/** bits per second */
export const fmtBits = (bps: number) => (bps >= 1e6 ? `${(bps / 1e6).toFixed(bps >= 1e7 ? 0 : 1)} Mbps` : bps >= 1e3 ? `${Math.round(bps / 1e3)} kbps` : "0 kbps");
