import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Culprit, fmtBytes, fmtDay, fmtRate, fmtTime, Moment, Now, RES_LABEL, Timeline } from "./types";
import "./App.css";

const RANGES = [{ h: 1, label: "1 hour" }, { h: 24, label: "24 hours" }, { h: 168, label: "7 days" }];

export default function App() {
  const [now, setNow] = useState<Now | null>(null);
  const [range, setRange] = useState(1);
  const [tl, setTl] = useState<Timeline | null>(null);
  const [moment, setMoment] = useState<Moment | null>(null);
  const [confirm, setConfirm] = useState<Culprit | null>(null);
  const [toast, setToast] = useState("");

  useEffect(() => {
    invoke<Now | null>("get_now").then((n) => n && setNow(n));
    const un = listen<Now>("now", (e) => setNow(e.payload));
    return () => { un.then((f) => f()); };
  }, []);

  useEffect(() => {
    const load = () => invoke<Timeline>("get_timeline", { hours: range }).then(setTl);
    load();
    const id = setInterval(load, range === 1 ? 10_000 : 60_000);
    return () => clearInterval(id);
  }, [range]);

  const flash = (m: string) => { setToast(m); setTimeout(() => setToast(""), 3000); };
  const endTask = async (c: Culprit) => {
    setConfirm(null);
    try { const n = await invoke<number>("end_task", { key: c.key }); flash(`${c.name} ended (${n} process${n > 1 ? "es" : ""}).`); }
    catch (e) { flash(String(e)); }
  };

  if (!now) return <div className="loading"><div className="spinner" />Measuring…</div>;
  const v = now.verdict;
  const res = v.resource ?? "cpu";

  return (
    <div className={`app lvl-${v.level}`}>
      <header className="verdict">
        <div className="badge">{v.level === "calm" ? "All calm" : v.level === "busy" ? "Busy" : "Struggling"}</div>
        <h1>{v.headline}</h1>
        <p className="muted">{v.detail}</p>
      </header>

      <section className="gauges">
        <Gauge label="Processor" value={v.cpu} sub={`across ${navigator.hardwareConcurrency} logical cores`} hot={v.resource === "cpu"} />
        <Gauge label="Memory" value={v.memPct} sub={`${fmtBytes(v.memUsed)} of ${fmtBytes(v.memTotal)}`} hot={v.resource === "memory"} />
        <Gauge label="Disk" value={v.disk} sub={`busy ${v.disk.toFixed(0)} % of the time`} hot={v.resource === "disk"} />
      </section>

      <main className="cols">
        <section className="panel">
          <div className="panelhead">
            <h2>{v.level === "calm" ? "Using the most right now" : `Using the most ${RES_LABEL[res].toLowerCase()}`}</h2>
          </div>
          <ul className="culprits">
            {v.culprits.slice(0, 6).map((c) => <CulpritRow key={c.key} c={c} res={res} onEnd={() => setConfirm(c)} flash={flash} />)}
          </ul>
        </section>

        <section className="panel">
          <div className="panelhead">
            <h2>History</h2>
            <div className="seg">
              {RANGES.map((r) => <button key={r.h} className={range === r.h ? "on" : ""} onClick={() => { setRange(r.h); setMoment(null); }}>{r.label}</button>)}
            </div>
          </div>
          {tl && <Chart tl={tl} selected={moment} onPick={setMoment} />}
          <div className="legend"><span className="l-cpu">Processor</span><span className="l-mem">Memory</span><span className="l-disk">Disk</span><span className="l-moment">Slow moment</span></div>
          <h3 className="sub">Slow moments</h3>
          {tl && tl.moments.length === 0 && <p className="muted small">No slowdowns in this period. 🎉 Culprit records every moment, so next time you can look back and see why.</p>}
          <ul className="moments">
            {tl?.moments.slice(0, 30).map((m) => (
              <li key={m.start} className={moment?.start === m.start ? "on" : ""} onClick={() => setMoment(m)}>
                <span className="when">{range > 24 ? `${fmtDay(m.start)} ` : ""}{fmtTime(m.start)}–{fmtTime(m.end)}</span>
                <span className={`res r-${m.resource}`}>{RES_LABEL[m.resource]} {m.peak} %</span>
                <span className="who">{m.culprits.join(" + ") || "unknown"}</span>
              </li>
            ))}
          </ul>
        </section>
      </main>

      <footer className="foot">
        <span>Culprit itself: <b>{now.cost.cpu.toFixed(1)} % CPU</b> · {fmtBytes(now.cost.mem)}{now.cost.windowOpen ? " (with this window open; it drops when you close it)" : ""}</span>
        <span className="muted">Updates every 2 s · history kept 7 days, on this PC only</span>
        <button className="ghost overlaybtn" onClick={() => invoke("toggle_overlay")} title="A tiny always-on-top graph of CPU, memory and disk">▦ Mini overlay</button>
      </footer>

      {confirm && (
        <div className="overlay" onClick={() => setConfirm(null)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h2>End {confirm.name}?</h2>
            <p>{confirm.pids.length > 1 ? `All ${confirm.pids.length} of its processes will be closed immediately.` : "It will be closed immediately."} <b>Unsaved work in {confirm.name} will be lost.</b></p>
            <p className="muted">If it's frozen, this is the way out. If it's just busy, waiting a minute or closing it normally is safer.</p>
            <div className="btns">
              <button className="ghost" onClick={() => setConfirm(null)}>Cancel</button>
              <button className="danger" onClick={() => endTask(confirm)}>End task</button>
            </div>
          </div>
        </div>
      )}
      {toast && <div className="toast">{toast}</div>}
    </div>
  );
}

function Gauge({ label, value, sub, hot }: { label: string; value: number; sub: string; hot: boolean }) {
  const lvl = value >= 90 ? "strained" : value >= 70 ? "busy" : "calm";
  return (
    <div className={`gauge ${hot ? "hot" : ""}`}>
      <div className="glabel"><span>{label}</span><b>{value.toFixed(0)} %</b></div>
      <div className="gbar"><div className={`g-${lvl}`} style={{ width: `${Math.min(100, value)}%` }} /></div>
      <div className="muted small">{sub}</div>
    </div>
  );
}

const iconCache = new Map<string, string | null>();

function CulpritRow({ c, res, onEnd, flash }: { c: Culprit; res: string; onEnd: () => void; flash: (m: string) => void }) {
  const [icon, setIcon] = useState<string | null>(iconCache.get(c.exe) ?? null);
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (iconCache.has(c.exe)) return;
    invoke<string | null>("get_icon", { exe: c.exe }).then((i) => { iconCache.set(c.exe, i); setIcon(i); });
  }, [c.exe]);
  const value = res === "disk" ? fmtRate(c.io) : res === "memory" ? fmtBytes(c.mem) : `${c.cpu.toFixed(1)} %`;
  return (
    <li className={`culprit ${open ? "open" : ""}`}>
      <div className="crow" onClick={() => setOpen(!open)}>
        {icon ? <img src={icon} alt="" /> : <span className="cinit">{c.name.slice(0, 1)}</span>}
        <div className="cbody">
          <div className="cname"><b>{c.name}</b>{c.pids.length > 1 && <span className="muted small"> · {c.pids.length} processes</span>}</div>
          <div className="share"><div style={{ width: `${Math.max(2, c.share * 100)}%` }} /></div>
        </div>
        <div className="cval"><b>{value}</b><span className="muted small">{Math.round(c.share * 100)} % of load</span></div>
      </div>
      {open && (
        <div className="cdetail">
          <p>{c.what}</p>
          <p className="muted">{c.advice}</p>
          {c.services.length > 0 && <p className="muted small">Services: {c.services.join(", ")}</p>}
          <div className="stats muted small">CPU {c.cpu.toFixed(1)} % · memory {fmtBytes(c.mem)} · disk {fmtRate(c.io)}</div>
          <div className="actions">
            <button className="ghost" onClick={() => invoke("open_location", { exe: c.exe }).catch((e) => flash(String(e)))}>Open file location</button>
            <button className="ghost" onClick={() => invoke("search_web", { query: c.exe })}>What is this?</button>
            {c.critical
              ? <span className="muted small">Part of Windows: can't be ended safely</span>
              : <button className="danger-ghost" onClick={onEnd}>End task…</button>}
          </div>
        </div>
      )}
    </li>
  );
}

function Chart({ tl, selected, onPick }: { tl: Timeline; selected: Moment | null; onPick: (m: Moment) => void }) {
  const W = 560, H = 170;
  const ref = useRef<SVGSVGElement>(null);
  const x = (t: number) => ((t - tl.from) / Math.max(1, tl.to - tl.from)) * W;
  const y = (v: number) => H - (Math.min(100, v) / 100) * (H - 8) - 2;
  const path = (i: 1 | 2 | 3) => tl.points.map((p, k) => `${k ? "L" : "M"}${x(p[0]).toFixed(1)},${y(p[i]).toFixed(1)}`).join("");
  const ticks = useMemo(() => {
    const n = 4;
    return Array.from({ length: n + 1 }, (_, i) => tl.from + ((tl.to - tl.from) * i) / n);
  }, [tl.from, tl.to]);
  const long = tl.to - tl.from > 86400 * 1.5;
  return (
    <svg ref={ref} className="chart" viewBox={`0 0 ${W} ${H + 18}`} preserveAspectRatio="none">
      {[25, 50, 75].map((g) => <line key={g} x1={0} x2={W} y1={y(g)} y2={y(g)} className="grid" />)}
      {tl.moments.map((m) => (
        <rect key={m.start} x={x(m.start)} width={Math.max(3, x(m.end) - x(m.start))} y={0} height={H}
          className={`mband ${selected?.start === m.start ? "on" : ""}`} onClick={() => onPick(m)} />
      ))}
      <path d={path(2)} className="ln-mem" />
      <path d={path(3)} className="ln-disk" />
      <path d={path(1)} className="ln-cpu" />
      {ticks.map((t) => <text key={t} x={Math.min(W - 30, Math.max(0, x(t) - 14))} y={H + 14} className="tick">{long ? fmtDay(t) : fmtTime(t)}</text>)}
      {tl.points.length < 2 && <text x={W / 2 - 110} y={H / 2} className="tick">Collecting history: give it a minute…</text>}
    </svg>
  );
}
