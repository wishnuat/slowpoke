import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Now } from "./types";
import "./Overlay.css";

const N = 30; // 30 samples × 2 s = the last minute
type Point = { cpu: number; mem: number; disk: number };
interface Prefs { opacity: number; clickThrough: boolean }

const tone = (v: number) => (v >= 90 ? "strained" : v >= 70 ? "busy" : "calm");
const OPACITIES = [0.78, 0.55, 0.95];

export default function Overlay() {
  const [pts, setPts] = useState<Point[]>([]);
  const [level, setLevel] = useState<string>("calm");
  const [headline, setHeadline] = useState("Measuring…");
  const [prefs, setPrefs] = useState<Prefs>({ opacity: 0.78, clickThrough: false });
  const last = useRef(0);

  useEffect(() => {
    const push = (n: Now) => {
      if (n.verdict.t === last.current) return;
      last.current = n.verdict.t;
      const v = n.verdict;
      setPts((p) => [...p.slice(-(N - 1)), { cpu: v.cpu, mem: v.memPct, disk: v.disk }]);
      setLevel(v.level);
      setHeadline(v.headline);
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
        <button title="Click-through: clicks pass to the window below. Turn it off from the Culprit tray menu." onClick={() => invoke("set_click_through", { on: true })}>⇣</button>
        <button title="Open Culprit" onClick={() => invoke("open_main")}>↗</button>
        <button title="Close overlay" className="x" onClick={() => invoke("toggle_overlay")}>×</button>
      </div>
      <span className="dot" />
      <Row label="CPU" k="cpu" pts={pts} value={cur?.cpu} />
      <Row label="MEM" k="mem" pts={pts} value={cur?.mem} />
      <Row label="DISK" k="disk" pts={pts} value={cur?.disk} />
    </div>
  );
}

function Row({ label, k, pts, value }: { label: string; k: keyof Point; pts: Point[]; value?: number }) {
  const W = 100, H = 24;
  const v = value ?? 0;
  const xs = (i: number) => (i / (N - 1)) * W;
  const start = N - pts.length;
  const line = pts.map((p, i) => `${i ? "L" : "M"}${xs(start + i).toFixed(2)},${(H - (Math.min(100, p[k]) / 100) * (H - 2) - 1).toFixed(2)}`).join("");
  const area = pts.length > 1 ? `${line}L${xs(N - 1)},${H}L${xs(start)},${H}Z` : "";
  const t = tone(v);
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
      <span className="val">{value === undefined ? "–" : Math.round(v)}<small>%</small></span>
    </div>
  );
}
