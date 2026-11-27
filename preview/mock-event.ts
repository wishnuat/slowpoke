// Mocked @tauri-apps/api/event: streams fake "now" samples so live views move.
// The overlay (#overlay) also plays a calm → storm → calm loop so its colours can be shown.
import { nowFor, overall } from "./mock-core";

let t = 0;
const subs = new Set<(e: { payload: unknown }) => void>();
const speed = Number(new URLSearchParams(location.search).get("speed") || "1");
(window as any).__setScenario = (s: string) => { (window as any).__scenario = s; };

setInterval(() => {
  t += 1;
  let payload: any = nowFor(t);
  if (location.hash === "#overlay") {
    const k = t % 50;
    const storm = k > 18 && k < 34 ? Math.sin(((k - 18) / 16) * Math.PI) : 0;
    const wob = (f: number, p: number) => 0.5 + 0.5 * Math.sin(t / f + p);
    const cpu = Math.min(100, 14 + 10 * wob(3, 0) + 78 * storm);
    const disk = Math.min(100, 4 + 6 * wob(2, 1) + 92 * storm);
    const mem = 62 + 6 * wob(9, 2) + 14 * storm;
    const level = cpu >= 90 || disk >= 95 ? "strained" : cpu >= 60 || disk >= 70 ? "busy" : "calm";
    const verdict = { ...payload.verdict, cpu, disk, memPct: mem, level };
    const lat = 20 + 6 * wob(2.5, 0) + 40 * storm;
    const net = { ...payload.net, health: "good", snapshot: { ...payload.net.snapshot, internet: { ...payload.net.snapshot.internet, latency: lat } } };
    payload = { ...payload, verdict, net, overall: overall(verdict, net) };
  }
  subs.forEach((cb) => cb({ payload }));
}, 2000 / speed);

export async function listen(event: string, cb: (e: { payload: unknown }) => void) {
  if (event !== "now") return () => {};
  subs.add(cb);
  return () => subs.delete(cb);
}
