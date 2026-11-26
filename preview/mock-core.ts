// Browser-only stand-in for @tauri-apps/api/core (UI review, screenshots). Fictional but realistic data.
const GB = 1024 ** 3, MB = 1024 ** 2;
const now = Math.floor(Date.now() / 1000);
const c = (key: string, name: string, what: string, advice: string, pids: number, cpu: number, mem: number, io: number, share: number, critical = false, services: string[] = []) =>
  ({ key, exe: key, name, what, advice, pids: Array.from({ length: pids }, (_, i) => 1000 + i), cpu, mem, io, share, critical, services });

export const NOW = {
  verdict: {
    t: Date.now(), level: "busy", resource: "disk",
    headline: "Your PC is busy: Microsoft Defender and Windows Update are keeping the disk busy.",
    detail: "Disk busy 88 % of the time", cpu: 41, memPct: 71, commitPct: 55, disk: 88, memUsed: 11.4 * GB, memTotal: 16 * GB,
    culprits: [
      c("msmpeng.exe", "Microsoft Defender", "Windows' built-in antivirus, scanning files.", "Normal during scheduled scans and after installing or extracting many files. It slows down by itself.", 1, 14.2, 310 * MB, 61 * MB, 0.52, true),
      c("svc:Windows Update", "Windows Update", "Downloads and installs updates.", "A Windows service. It normally settles down by itself.", 2, 9.1, 180 * MB, 38 * MB, 0.32, true, ["wuauserv", "BITS"]),
      c("chrome.exe", "Google Chrome", "Every tab and extension runs as its own process.", "Close heavy tabs. Chrome's own Task Manager (Shift+Esc) shows which tab is busy.", 27, 8.3, 2.9 * GB, 6 * MB, 0.05),
      c("onedrive.exe", "OneDrive", "Syncs your files with the cloud.", "Busy while uploading or downloading many files. You can pause syncing from its tray icon.", 1, 2.1, 140 * MB, 5 * MB, 0.04),
      c("ms-teams.exe", "Microsoft Teams", "Chats, calls and meetings.", "Video calls are heavy by nature. Turning off background effects helps.", 6, 3.4, 820 * MB, 1 * MB, 0.01),
    ],
  },
  cost: { cpu: 0.6, mem: 96 * MB, windowOpen: true },
};

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

export async function invoke(cmd: string, args?: any): Promise<any> {
  if (cmd === "get_now") return NOW;
  if (cmd === "get_timeline") return timeline(args.hours);
  return null;
}
