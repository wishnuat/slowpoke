import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Area, fmtBytes, fmtMs, NetVerdict, Now, SpeedResult } from "./types";
import PcView from "./PcView";
import NetView, { LIVE } from "./NetView";
import "./App.css";

type Speed = { state: "idle" | "running" | "done" | "error"; r?: SpeedResult; err?: string };

export default function App() {
  const [now, setNow] = useState<Now | null>(null);
  const [tab, setTab] = useState<Area>("pc");
  const manual = useRef(false); // once the user picks a tab, stop following the verdict
  const seen = useRef<Area | null>(null);
  const live = useRef<NetVerdict[]>([]);
  const [speed, setSpeed] = useState<Speed>({ state: "idle" });
  const [toast, setToast] = useState("");

  useEffect(() => {
    const push = (n: Now) => {
      // the PC side updates every 2 s and the network every 3 s: keep each network sample once
      if (n.net && live.current[live.current.length - 1]?.t !== n.net.t) live.current = [...live.current, n.net].slice(-LIVE);
      // follow the problem only once it shows up in two updates in a row (start-up blips shouldn't flip the tab)
      const a = n.overall.area;
      if (!manual.current && a && a === seen.current) setTab(a);
      seen.current = a;
      setNow(n);
    };
    invoke<Now | null>("get_now").then((n) => n && push(n));
    const un = listen<Now>("now", (e) => push(e.payload));
    return () => { un.then((f) => f()); };
  }, []);

  const flash = (m: string) => { setToast(m); setTimeout(() => setToast(""), 3000); };
  const pick = (a: Area) => { manual.current = true; setTab(a); };

  if (!now) return <div className="loading"><div className="spinner" />Measuring…</div>;
  const v = now.verdict, o = now.overall, net = now.net;
  const netBad = net != null && net.health !== "good";
  const pcBad = v.level !== "calm";

  return (
    <div className={`app lvl-${o.level}`}>
      <header className="verdict">
        <div className="vtop">
          <div className="badge">{o.level === "calm" ? "All good" : o.level === "busy" ? (o.area === "network" ? "Having trouble" : "Busy") : (o.area === "network" ? "Not working" : "Struggling")}</div>
          {o.area && <span className="area">{o.area === "pc" ? "This PC" : "Network"}</span>}
        </div>
        <h1>{o.headline}</h1>
        <p className="muted">{o.detail}</p>
        {o.also && <p className="also">{o.also}</p>}
      </header>

      <section className="gauges">
        <Gauge label="Processor" value={v.cpu} sub={`across ${navigator.hardwareConcurrency} logical cores`} hot={v.resource === "cpu"} onClick={() => pick("pc")} />
        <Gauge label="Memory" value={v.memPct} sub={`${fmtBytes(v.memUsed)} of ${fmtBytes(v.memTotal)}`} hot={v.resource === "memory"} onClick={() => pick("pc")} />
        <Gauge label="Disk" value={v.disk} sub={`busy ${v.disk.toFixed(0)} % of the time`} hot={v.resource === "disk"} onClick={() => pick("pc")} />
        <NetCard net={net} onClick={() => pick("network")} />
      </section>

      <nav className="tabs" role="tablist">
        <button role="tab" aria-selected={tab === "pc"} className={tab === "pc" ? "on" : ""} onClick={() => pick("pc")}>
          This PC{pcBad && <i className={`tdot d-${v.level}`} />}
        </button>
        <button role="tab" aria-selected={tab === "network"} className={tab === "network" ? "on" : ""} onClick={() => pick("network")}>
          Network{netBad && <i className={`tdot d-${net!.health === "down" ? "strained" : "busy"}`} />}
        </button>
      </nav>

      {tab === "pc" ? <PcView v={v} flash={flash} /> : <NetView v={net} live={live.current} speed={speed} setSpeed={setSpeed} />}

      <footer className="foot">
        <span>Slowpoke itself: <b>{now.cost.cpu.toFixed(1)} % CPU</b> · {fmtBytes(now.cost.mem)}{now.cost.windowOpen ? " (with this window open; it drops when you close it)" : ""}</span>
        <span className="muted">PC every 2 s · network every 3 s · history kept 7 days, on this PC only</span>
        <button className="ghost overlaybtn" onClick={() => invoke("toggle_overlay")} title="A tiny always-on-top graph of CPU, memory, disk and internet">▦ Mini overlay</button>
      </footer>
      {toast && <div className="toast">{toast}</div>}
    </div>
  );
}

function Gauge({ label, value, sub, hot, onClick }: { label: string; value: number; sub: string; hot: boolean; onClick: () => void }) {
  const lvl = value >= 90 ? "strained" : value >= 70 ? "busy" : "calm";
  return (
    <button className={`gauge ${hot ? "hot" : ""}`} onClick={onClick}>
      <div className="glabel"><span>{label}</span><b>{value.toFixed(0)} %</b></div>
      <div className="gbar"><div className={`g-${lvl}`} style={{ width: `${Math.min(100, value)}%` }} /></div>
      <div className="muted small">{sub}</div>
    </button>
  );
}

/** The fourth card: the internet at a glance, with one dot per hop. */
function NetCard({ net, onClick }: { net: NetVerdict | null; onClick: () => void }) {
  const lat = net?.snapshot.internet.latency;
  const tone = !net ? "calm" : net.health === "good" ? "calm" : net.health === "degraded" ? "busy" : "strained";
  return (
    <button className={`gauge netcard ${net && net.health !== "good" ? "hot" : ""}`} onClick={onClick}>
      <div className="glabel"><span>Internet</span><b className={`t-${tone}`}>{!net ? "…" : !net.snapshot.connected ? "offline" : fmtMs(lat)}</b></div>
      <div className="hops">
        {(net?.hops ?? []).map((h) => <span key={h.layer} className={`hop h-${h.health}`} title={`${h.label}: ${h.value}`}>{h.layer === "link" ? (net!.snapshot.wireless ? "Wi-Fi" : "Link") : h.layer === "dns" ? "DNS" : h.layer[0].toUpperCase() + h.layer.slice(1)}</span>)}
      </div>
      <div className="muted small">{!net ? "checking the connection…" : net.snapshot.wifi ? `Wi-Fi ${net.snapshot.wifi.quality} % · router ${fmtMs(net.snapshot.router.latency)}` : net.snapshot.connected ? `${net.snapshot.interface} · router ${fmtMs(net.snapshot.router.latency)}` : "not connected"}</div>
    </button>
  );
}
