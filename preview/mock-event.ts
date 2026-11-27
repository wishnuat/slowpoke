// Mocked @tauri-apps/api/event: streams fake "now" samples so live views (the overlay) move.
import { NOW } from "./mock-core";

let t = 0;
const subs = new Set<(e: { payload: unknown }) => void>();
const speed = Number(new URLSearchParams(location.search).get("speed") || "1");

setInterval(() => {
  t += 1;
  // calm → a disk + CPU storm around t≈18..34 → calm again (loops every 50 ticks)
  const k = t % 50;
  const storm = k > 18 && k < 34 ? Math.sin(((k - 18) / 16) * Math.PI) : 0;
  const wob = (f: number, p: number) => 0.5 + 0.5 * Math.sin(t / f + p);
  const cpu = Math.min(100, 14 + 10 * wob(3, 0) + 78 * storm);
  const disk = Math.min(100, 4 + 6 * wob(2, 1) + 92 * storm);
  const mem = 62 + 6 * wob(9, 2) + 14 * storm;
  const level = cpu >= 90 || disk >= 95 ? "strained" : cpu >= 60 || disk >= 70 ? "busy" : "calm";
  const payload = { ...NOW, verdict: { ...NOW.verdict, t: Date.now() + t, cpu, disk, memPct: mem, level } };
  subs.forEach((cb) => cb({ payload }));
}, 2000 / speed);

export async function listen(event: string, cb: (e: { payload: unknown }) => void) {
  if (event !== "now") return () => {};
  subs.add(cb);
  return () => subs.delete(cb);
}
