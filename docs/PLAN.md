# Culprit: Product Plan

> "Why is my PC slow?" Culprit answers in one sentence, **including for the slowdown that already ended** before
> you could open Task Manager.

## Problem

- **Task Manager is heavy and arrives late.** Opening it at the worst moment takes seconds, and by the time it
  renders, the spike is often over
- **Numbers, not answers.** "MsMpEng.exe 38 %" means nothing to most people. "Microsoft Defender is scanning files"
  does. `svchost.exe` hides dozens of different Windows services behind one name
- **No memory.** Task Manager shows *now*. Nobody can answer "why did it freeze at 10:14?"
- **Resource Monitor and Performance Monitor** have the data, but they're expert tools with expert UIs

## Who it's for

Everyone with a Windows PC: office workers, students, gamers, and the person who does IT support for their family.

## Principles

1. **Featherweight.** The sampler is native Rust using the same counters Windows itself uses. Budget: **< 0.5 % CPU
   and < 30 MB RAM** while running in the background. The UI (WebView) only exists while the window is open
2. **One sentence first.** Every view leads with a plain-language verdict, and details come after
3. **Remember.** A rolling 24-hour history of system load and the top processes per sample, kept small on disk
4. **Explain, don't scare.** Known processes get a friendly name, what they do, and whether it's normal

## MVP scope

| # | Capability | Acceptance criteria |
|---|---|---|
| 1 | Sampler | Every 2 s: total CPU, memory used/committed, disk active time, and per process CPU %, private memory and disk I/O. Self-cost is measured and shown in the app; it must stay under budget |
| 2 | Verdict engine | Turns a sample into "Your PC is busy: **Windows Update** is installing updates (CPU 46 %, disk 92 %)" or "All calm". Covers CPU, memory pressure (low free memory, high commit) and disk saturation |
| 3 | Plain-language catalogue | ~80 common processes (Defender, Windows Update, Search indexer, OneDrive, Teams, browsers, antivirus, game launchers…) with friendly name, explanation, and "normal?" advice. `svchost` is resolved to the services it hosts. Apps are grouped by executable, so Chrome's 30 processes count as one |
| 4 | Tray indicator | Tray icon colored calm / busy / strained, with a tooltip verdict. Left click opens the window |
| 5 | "Right now" | Verdict, live CPU/memory/disk gauges, top culprits with share of load, and actions: open file location, search online, **End task** |
| 6 | Timeline | Last hour / 24 h chart of CPU, memory and disk. **Slow moments** (sustained high load) are detected and marked; clicking one shows what caused it, e.g. "10:14–10:16 · Microsoft Defender scan + Windows Update" |
| 7 | History on disk | Compact binary ring buffer, ~1 week ≈ a few MB. Survives restarts |

### Non-goals (MVP)
Killing or suspending things automatically, "boost" or "RAM cleaner" gimmicks, GPU/network per process (later),
and startup-app management.

## Architecture

```
sampler thread (Rust, 2 s)                 history (ring file)        verdict
  GetSystemTimes · GlobalMemoryStatusEx  ──▶ samples + top-N per sample ──▶ plain language
  PDH "% Disk Time"                                                   │
  per process: CreateToolhelp32Snapshot, GetProcessTimes,             ▼
  GetProcessMemoryInfo, GetProcessIoCounters                     tray tooltip / icon color
  svchost → services: EnumServicesStatusExW (by PID)                  │
                                                                      ▼
                                                    React UI (only while the window is open)
```

## Risks

| Risk | Mitigation |
|---|---|
| Becoming the thing that slows the PC | Self-cost measured every minute and shown. Adaptive interval (5 s when calm). Per-process handles cached between samples |
| Protected processes (no access to times or memory) | Fall back to what's readable and mark it. Never require admin for the core view |
| Wrong blame (correlation ≠ cause) | The verdict names the top consumers of the scarce resource (CPU *or* disk *or* memory), never a single guess |
| End task on something important | Disabled for critical system processes. Confirmation that names what will happen |

## Milestones

1. Sampler + self-cost measurement (+ benchmarks)
2. Catalogue, svchost resolution, verdict engine (unit-tested with recorded samples)
3. History ring file + slow-moment detection
4. Tray + UI (now, timeline, moment details)
5. README, demo video, release

## 0.3: Slowpoke (Culprit + Netdoctor)

Users can't tell whether "slow" means the PC or the connection, so the two apps merged into one tray app.
- The network half (probes, verdict, history) moved in unchanged as `src-tauri/src/net/`, on its own thread
- `overall.rs` weighs the two verdicts on one scale (no internet > struggling PC > degraded network > busy PC) and
  always says the other side is fine when it is
- UI: one verdict header, four summary cards (processor, memory, disk, internet), and "This PC" / "Network" tabs
  that follow the problem. The overlay gains a NET row (internet latency)
- Budget check after the merge: 0.13 % CPU and 8.7 MB in the tray (was 0.12 % + 0.01 % as two apps)
