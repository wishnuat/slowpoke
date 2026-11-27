import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { fmtBits, fmtDay, fmtMs, fmtTime, Hop, LAYER_LABEL, NetMoment, NetTimeline, NetVerdict, SpeedResult } from "./types";
import "./Net.css";

const RANGES = [{ h: 1, label: "1 hour" }, { h: 24, label: "24 hours" }, { h: 168, label: "7 days" }];
export const LIVE = 40; // network samples kept for the sparklines (2 minutes at 3 s)

type Speed = { state: "idle" | "running" | "done" | "error"; r?: SpeedResult; err?: string };

/** The network half: the path hop by hop, live tiles, history of bad moments and a speed test. */
export default function NetView({ v, live, speed, setSpeed }: { v: NetVerdict | null; live: NetVerdict[]; speed: Speed; setSpeed: (s: Speed) => void }) {
  const [range, setRange] = useState(1);
  const [tl, setTl] = useState<NetTimeline | null>(null);

  useEffect(() => {
    const load = () => invoke<NetTimeline>("get_net_timeline", { hours: range }).then(setTl);
    load();
    const id = setInterval(load, range === 1 ? 15_000 : 60_000);
    return () => clearInterval(id);
  }, [range]);

  const runSpeed = async () => {
    setSpeed({ state: "running" });
    try { setSpeed({ state: "done", r: await invoke<SpeedResult>("speed_test") }); }
    catch (e) { setSpeed({ state: "error", err: String(e) }); }
  };

  if (!v) return <div className="loading small"><div className="spinner" />Checking your connection…</div>;
  const s = v.snapshot;
  const series = (f: (n: NetVerdict) => number | null) => live.map(f);

  return (
    <div className="netview">
      <section className="path" aria-label="Network path">
        <Node icon="pc" label="This PC" value={s.connected ? s.interface : "offline"} health="good" />
        {v.hops.map((h) => (
          <PathStep key={h.layer} hop={h} culprit={v.layer === h.layer || (v.layer === "traffic" && h.layer === "internet")} />
        ))}
      </section>

      <section className="tiles">
        {s.wireless && s.wifi ? (
          <Tile title="Wi-Fi signal" value={`${s.wifi.quality} %`} sub={`${s.wifi.rssi != null ? `${s.wifi.rssi} dBm · ` : ""}${s.wifi.band ?? ""}${s.wifi.channel ? ` ch ${s.wifi.channel}` : ""} · ${s.wifi.rxRate} Mbps link`}
            data={series((n) => n.snapshot.wifi?.quality ?? null)} max={100} warn={45} invert />
        ) : (
          <Tile title="Link" value={s.connected ? fmtBits(s.linkSpeed) : "–"} sub={s.connected ? `${s.interface} (wired)` : "not connected"} data={[]} max={1} />
        )}
        <Tile title="Router" value={fmtMs(s.router.latency)} sub={s.gateway ? `${s.gateway} · ${Math.round(s.router.loss)} % loss` : "no gateway"}
          data={series((n) => n.snapshot.router.latency)} max={60} warn={100} />
        <Tile title="Internet" value={fmtMs(s.internet.latency)} sub={`1.1.1.1 · ${Math.round(s.internet.loss)} % loss · jitter ${fmtMs(s.internet.jitter)}`}
          data={series((n) => n.snapshot.internet.latency)} max={120} warn={150} />
        <Tile title="DNS" value={fmtMs(s.dnsMs)} sub={s.dnsFail > 0 ? `${Math.round(s.dnsFail)} % failed` : "answering"} data={series((n) => n.snapshot.dnsMs)} max={100} warn={300} />
        <Tile title="Traffic" value={`↓ ${fmtBits(s.rxBps)}`} sub={`↑ ${fmtBits(s.txBps)} · all apps combined`} data={series((n) => n.snapshot.rxBps / 1e6)} max={20} />
      </section>

      <main className="cols net-cols">
        <section className="panel">
          <div className="panelhead">
            <h2>Connection history</h2>
            <div className="seg">{RANGES.map((r) => <button key={r.h} className={range === r.h ? "on" : ""} onClick={() => setRange(r.h)}>{r.label}</button>)}</div>
          </div>
          {tl && <NetChart tl={tl} />}
          <div className="legend"><span className="l-inet">Internet latency</span><span className="l-router">Router latency</span><span className="l-loss">Packet loss</span><span className="l-bad">Bad moment</span></div>
          <h3 className="sub">Bad moments</h3>
          {tl && tl.moments.length === 0 && <p className="muted small">No connection problems in this period. When one happens, Slowpoke records which link was to blame, so you can look back later.</p>}
          <ul className="moments">
            {tl?.moments.slice(0, 30).map((m) => <MomentRow key={m.start} m={m} long={range > 24} />)}
          </ul>
        </section>

        <section className="panel speed">
          <h2>Speed test</h2>
          <p className="muted small">Downloads about 25 MB from Cloudflare's speed-test server to measure your real download speed. On a metered or mobile connection, that uses data.</p>
          {speed.state === "done" && speed.r && (
            <div className="speedres">
              <div className="big">{speed.r.mbps >= 100 ? speed.r.mbps.toFixed(0) : speed.r.mbps.toFixed(1)}<span>Mbps</span></div>
              <div className="muted small">{Math.round(speed.r.bytes / 1024 ** 2)} MB in {speed.r.seconds.toFixed(1)} s · {speed.r.server}</div>
              <p className="small">{speedHint(speed.r.mbps)}</p>
            </div>
          )}
          {speed.state === "error" && <p className="err small">{speed.err}</p>}
          <button className="primary" disabled={speed.state === "running"} onClick={runSpeed}>
            {speed.state === "running" ? <><span className="mini" /> Testing…</> : speed.state === "done" ? "Run again" : "Run speed test (~25 MB)"}
          </button>
          <div className="netinfo small">
            <div><span className="muted">Connection</span><b>{s.connected ? s.interface : "offline"}</b></div>
            {s.wifi && <div><span className="muted">Network</span><b>{s.wifi.ssid || "hidden"}</b></div>}
            <div><span className="muted">Router</span><b>{s.gateway ?? "–"}</b></div>
            <button className="link" onClick={() => invoke("open_network_settings")}>Windows network settings</button>
          </div>
        </section>
      </main>
    </div>
  );
}

function speedHint(m: number) {
  if (m >= 100) return "Excellent: 4K streaming, big downloads and video calls on several devices.";
  if (m >= 25) return "Good: HD streaming and video calls are comfortable.";
  if (m >= 8) return "Okay for browsing, music and one video call. Several people at once will feel it.";
  return "Slow: expect buffering on video. Check the Wi-Fi signal and router above first.";
}

const ICONS: Record<string, React.ReactElement> = {
  pc: <path d="M4 5h16v10H4zM9 19h6M12 15v4" />,
  link: <><path d="M5 10a10 10 0 0 1 14 0M8 13a6 6 0 0 1 8 0" /><circle cx="12" cy="16.5" r="1.3" fill="currentColor" /></>,
  ethernet: <path d="M7 4h10v9H7zM9 13v3M12 13v3M15 13v3M6 20h12" />,
  router: <><rect x="4" y="12" width="16" height="7" rx="2" /><path d="M8 12V8M16 12V8M8 16h.01M11 16h.01" /></>,
  internet: <><circle cx="12" cy="12" r="8" /><path d="M4 12h16M12 4c2.5 2.3 2.5 13.7 0 16M12 4c-2.5 2.3-2.5 13.7 0 16" /></>,
  dns: <><path d="M5 6h14v5H5zM5 13h14v5H5z" /><path d="M8 8.5h.01M8 15.5h.01" /></>,
};

function Node({ icon, label, value, health, culprit }: { icon: string; label: string; value: string; health: string; culprit?: boolean }) {
  return (
    <div className={`node n-${health} ${culprit ? "culprit-hop" : ""}`}>
      <div className="nicon"><svg viewBox="0 0 24 24">{ICONS[icon]}</svg></div>
      <div className="nlabel" title={label}>{label}</div>
      <div className="nvalue" title={value}>{value}</div>
      {culprit && <div className="ntag">here</div>}
    </div>
  );
}

function PathStep({ hop, culprit }: { hop: Hop; culprit: boolean }) {
  const icon = hop.layer === "link" ? (hop.label === "Ethernet" ? "ethernet" : "link") : hop.layer;
  return (
    <>
      <div className={`wire w-${hop.health}`}><span /></div>
      <Node icon={icon} label={hop.label} value={hop.value} health={hop.health} culprit={culprit && hop.health !== "good"} />
    </>
  );
}

function Tile({ title, value, sub, data, max, warn, invert }: { title: string; value: string; sub: string; data: (number | null)[]; max: number; warn?: number; invert?: boolean }) {
  const W = 120, H = 34;
  const top = Math.max(max, ...data.map((d) => d ?? 0));
  // right-aligned: the newest sample is always at the right edge
  const step = W / Math.max(1, LIVE - 1), off = W - (data.length - 1) * step;
  const pts = data.map((d, i) => (d == null ? null : [off + i * step, H - (d / top) * (H - 3) - 1.5]));
  const segs: string[] = [];
  let cur = "";
  pts.forEach((p) => { if (!p) { if (cur) segs.push(cur); cur = ""; } else cur += `${cur ? "L" : "M"}${p[0].toFixed(1)},${p[1].toFixed(1)}`; });
  if (cur) segs.push(cur);
  const last = data[data.length - 1];
  const bad = warn != null && last != null && (invert ? last < warn : last > warn);
  return (
    <div className={`tile ${bad ? "bad" : ""}`}>
      <div className="ttitle">{title}</div>
      <div className="tvalue">{value}</div>
      <svg className="spark" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none">{segs.map((d, i) => <path key={i} d={d} />)}</svg>
      <div className="tsub" title={sub}>{sub}</div>
    </div>
  );
}

function MomentRow({ m, long }: { m: NetMoment; long: boolean }) {
  const mins = Math.max(1, Math.round((m.end - m.start) / 60));
  return (
    <li>
      <span className="when">{long ? `${fmtDay(m.start)} ` : ""}{fmtTime(m.start)}</span>
      <span className={`sev s-${m.severity}`}>{m.layer ? LAYER_LABEL[m.layer] : "Connection"}</span>
      <span className="what">{m.worst}</span>
      <span className="muted small">{mins} min</span>
    </li>
  );
}

function NetChart({ tl }: { tl: NetTimeline }) {
  const W = 600, H = 150;
  const x = (t: number) => ((t - tl.from) / Math.max(1, tl.to - tl.from)) * W;
  const maxLat = Math.max(80, ...tl.points.map((p) => (p[3] !== 65535 ? p[3] : 0))) * 1.1;
  const y = (v: number) => H - (Math.min(v, maxLat) / maxLat) * (H - 8) - 2;
  const line = (i: number) => {
    let d = "", gap = true;
    for (const p of tl.points) {
      if (p[i] === 65535) { gap = true; continue; }
      d += `${gap ? "M" : "L"}${x(p[0]).toFixed(1)},${y(p[i]).toFixed(1)}`;
      gap = false;
    }
    return d;
  };
  const bw = Math.max(1.5, W / Math.max(60, tl.points.length));
  const long = tl.to - tl.from > 86400 * 1.5;
  const ticks = [0, 1, 2, 3, 4].map((i) => tl.from + ((tl.to - tl.from) * i) / 4);
  return (
    <svg className="chart netchart" viewBox={`0 0 ${W} ${H + 18}`} preserveAspectRatio="none">
      {tl.moments.map((m) => <rect key={m.start} x={x(m.start)} y={0} width={Math.max(3, x(m.end) - x(m.start))} height={H} className={`band b-${m.severity}`} />)}
      {tl.points.filter((p) => p[4] > 0).map((p) => <rect key={p[0]} x={x(p[0]) - bw / 2} width={bw} y={H - (p[4] / 100) * (H - 8)} height={(p[4] / 100) * (H - 8)} className="lossbar" />)}
      <path d={line(2)} className="ln-router" />
      <path d={line(3)} className="ln-inet" />
      <text x={W - 4} y={12} textAnchor="end" className="tick">{Math.round(maxLat)} ms</text>
      {ticks.map((t, i) => (
        <text key={t} x={i === 0 ? 0 : i === ticks.length - 1 ? W - 2 : x(t)} y={H + 14} textAnchor={i === 0 ? "start" : i === ticks.length - 1 ? "end" : "middle"} className="tick">
          {long ? fmtDay(t) : fmtTime(t)}
        </text>
      ))}
      {tl.points.length < 3 && <text x={W / 2 - 120} y={H / 2} className="tick">Collecting history: give it a minute…</text>}
    </svg>
  );
}
