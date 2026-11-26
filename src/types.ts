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
