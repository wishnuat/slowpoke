import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Now } from "./types";
import "./Overlay.css";

const N = 30; // 30 samples × 2 s = the last minute
type Point = { cpu: number; mem: number; disk: number; net: number | null };
interface Prefs { opacity: number; clickThrough: boolean }

const tone = (v: number) => (v >= 90 ? "strained" : v >= 70 ? "busy" : "calm");
const OPACITIES = [0.78, 0.55, 0.95];

export default function Overlay() {
  const [pts, setPts] = useState<Point[]>([]);
  const [level, setLevel] = useState<string>("calm");
  const [headline, setHeadline] = useState("Measuring…");
  const [netTone, setNetTone] = useState("calm");
  const [offline, setOffline] = useState(false);
  const [prefs, setPrefs] = useState<Prefs>({ opacity: 0.78, clickThrough: false });
  const last = useRef(0);

  useEffect(() => {
    const push = (n: Now) => {
      if (n.verdict.t === last.current) return;
      last.current = n.verdict.t;
      const v = n.verdict;
      const ms = n.net?.snapshot.connected ? n.net.snapshot.internet.latency : null;
      setPts((p) => [...p.slice(-(N - 1)), { cpu: v.cpu, mem: v.memPct, disk: v.disk, net: ms }]);
      setNetTone(!n.net ? "calm" : n.net.health === "good" ? "calm" : n.net.health === "degraded" ? "busy" : "strained");
      setOffline(n.net != null && !n.net.snapshot.connected);
      setLevel(n.overall.level);
      setHeadline(n.overall.headline);
    };
    invoke<Now | null>("get_now").then((n) => n && push(n));
    invoke<Prefs>("get_overlay_prefs").then((p) => p && setPrefs(p));
    const a = listen<Now>("now", (e) => push(e.payload));
    const b = listen<Prefs>("overlay-prefs", (e) => setPrefs(e.payload));
    return () => { a.then((f) => f()); b.then((f) => f()); };
  }, []);

  const onMouseDown = (e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest("button")) return;
    if (e.detail === 2) { invoke("open_main"); return; } // double-click: open the full window
    getCurrentWindow().startDragging().catch(() => {});
  };
  const cycleOpacity = () => {
    const i = OPACITIES.findIndex((o) => Math.abs(o - prefs.opacity) < 0.01);
    const next = OPACITIES[(i + 1) % OPACITIES.length];
    setPrefs({ ...prefs, opacity: next });
    invoke("set_overlay_opacity", { opacity: next });
  };

  const cur = pts[pts.length - 1];
  return (
    <div className={`ov lvl-${level}`} style={{ "--a": prefs.opacity } as React.CSSProperties} onMouseDown={onMouseDown} title={headline}>
      <div className="ovtools">
        <button title="Background opacity" onClick={cycleOpacity}>◐</button>
        <button title="Click-through: clicks pass to the window below. Turn it off from the Slowpoke tray menu." onClick={() => invoke("set_click_through", { on: true })}>⇣</button>
        <button title="Open Slowpoke" onClick={() => invoke("open_main")}>↗</button>
        <button title="Close overlay" className="x" onClick={() => invoke("toggle_overlay")}>×</button>
      </div>
      <span className="dot" />
      <Row label="CPU" k="cpu" pts={pts} value={cur?.cpu} />
      <Row label="MEM" k="mem" pts={pts} value={cur?.mem} />
      <Row label="DISK" k="disk" pts={pts} value={cur?.disk} />
      <Row label="NET" k="net" pts={pts} value={cur?.net ?? undefined} max={NET_MAX} unit="ms" tone={netTone} text={offline ? "off" : undefined} />
    </div>
  );
}

// internet latency is drawn on a 0–200 ms scale, so the same colour bands mean 140 ms+ amber and 180 ms+ red
const NET_MAX = 200;

function Row({ label, k, pts, value, max = 100, unit = "%", tone: forced, text }: { label: string; k: keyof Point; pts: Point[]; value?: number; max?: number; unit?: string; tone?: string; text?: string }) {
  const W = 100, H = 24;
  const v = value ?? 0;
  const pct = (x: number | null) => (x == null ? null : (Math.min(max, x) / max) * 100);
  const xs = (i: number) => (i / (N - 1)) * W;
  const start = N - pts.length;
  // a missing value (no reply / offline) breaks the line instead of dropping it to zero
  let line = "", gap = true, first = -1, lastI = -1;
  pts.forEach((p, i) => {
    const y = pct(p[k]);
    if (y == null) { gap = true; return; }
    if (first < 0) first = i;
    lastI = i;
    line += `${gap ? "M" : "L"}${xs(start + i).toFixed(2)},${(H - (y / 100) * (H - 2) - 1).toFixed(2)}`;
    gap = false;
  });
  const area = line && !line.slice(1).includes("M") && lastI > first ? `${line}L${xs(start + lastI)},${H}L${xs(start + first)},${H}Z` : "";
  const t = forced ?? tone(v);
  return (
    <div className={`row t-${t}`}>
      <span className="lbl">{label}</span>
      <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" className="spark">
        <defs>
          {/* colour by height: anything above 90 % is red and 70–90 % amber, so past spikes stay visible */}
          <linearGradient id={`h-${k}`} gradientUnits="userSpaceOnUse" x1="0" y1="1" x2="0" y2={H - 1}>
            <stop offset="0.08" stopColor="#f87171" />
            <stop offset="0.22" stopColor="#fbbf24" />
            <stop offset="0.36" stopColor="#4ade80" />
            <stop offset="1" stopColor="#4ade80" />
          </linearGradient>
          <linearGradient id={`a-${k}`} gradientUnits="userSpaceOnUse" x1="0" y1="1" x2="0" y2={H}>
            <stop offset="0.08" stopColor="#f87171" stopOpacity="0.45" />
            <stop offset="0.3" stopColor="#fbbf24" stopOpacity="0.3" />
            <stop offset="0.5" stopColor="#4ade80" stopOpacity="0.22" />
            <stop offset="1" stopColor="#4ade80" stopOpacity="0" />
          </linearGradient>
        </defs>
        {area && <path d={area} fill={`url(#a-${k})`} />}
        {line && <path d={line} className="ln" stroke={`url(#h-${k})`} />}
      </svg>
      <span className="val">{text ?? (value === undefined ? "–" : Math.round(v))}{!text && <small>{unit === "%" ? "%" : ` ${unit}`}</small>}</span>
    </div>
  );
}
