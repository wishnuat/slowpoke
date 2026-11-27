pub mod catalog;
pub mod history;
pub mod net;
pub mod overall;
pub mod sampler;
pub mod services;
pub mod verdict;
pub mod winutil;

use history::{History, Moment};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use overall::Overall;
use verdict::{Level, Verdict};

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

const INTERVAL: Duration = Duration::from_secs(2);

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SelfCost {
    /// Slowpoke's own CPU (sampler + UI), % of the machine
    pub cpu: f32,
    /// private memory of Slowpoke and its WebView processes, bytes
    pub mem: u64,
    pub window_open: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Now {
    pub verdict: Verdict,
    /// latest network diagnosis (None until the first network check)
    pub net: Option<net::verdict::Verdict>,
    pub overall: Overall,
    pub cost: SelfCost,
}

struct Shared {
    latest: Mutex<Option<Verdict>>,
    cost: Mutex<SelfCost>,
    history: Mutex<History>,
    /// exe → path of one running instance (for icons / open location)
    paths: Mutex<HashMap<String, String>>,
    level: AtomicU8,
    overlay: Mutex<OverlayPrefs>,
    net: Mutex<Option<net::verdict::Verdict>>,
    net_history: Mutex<net::history::History>,
    speed_running: AtomicBool,
}

fn level_u8(l: Level) -> u8 {
    overall::rank(l)
}

/// 32×32 tray icon: a dot coloured by load.
fn tray_icon(level: u8) -> Image<'static> {
    let (r, g, b) = match level {
        0 => (34u8, 197u8, 94u8),
        1 => (245, 158, 11),
        _ => (239, 68, 68),
    };
    let mut px = vec![0u8; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let (dx, dy) = (x as f32 - 15.5, y as f32 - 15.5);
            let d = (dx * dx + dy * dy).sqrt();
            let i = (y * 32 + x) * 4;
            if d <= 13.5 {
                let ring = d > 10.5;
                let k = if ring { 0.75 } else { 1.0 };
                px[i] = (r as f32 * k) as u8;
                px[i + 1] = (g as f32 * k) as u8;
                px[i + 2] = (b as f32 * k) as u8;
                px[i + 3] = 255;
                // magnifier handle hint: a white highlight
                if (dx + 4.0).powi(2) + (dy + 4.0).powi(2) < 9.0 {
                    px[i] = 255;
                    px[i + 1] = 255;
                    px[i + 2] = 255;
                    px[i + 3] = 200;
                }
            } else if d <= 14.5 {
                let a = ((14.5 - d) * 255.0) as u8;
                px[i] = r / 2;
                px[i + 1] = g / 2;
                px[i + 2] = b / 2;
                px[i + 3] = a;
            }
        }
    }
    Image::new_owned(px, 32, 32)
}

fn open_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    // created on demand and destroyed on close, so the WebView costs nothing while Slowpoke sits in the tray
    let _ = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Slowpoke")
        .inner_size(1100.0, 800.0)
        .min_inner_size(860.0, 600.0)
        .build();
}

// ---------------------------------------------------------------- mini overlay

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct OverlayPrefs {
    /// physical position of the top-left corner
    pub x: Option<i32>,
    pub y: Option<i32>,
    /// logical size
    pub w: f64,
    pub h: f64,
    /// background tint opacity, 0.3..1
    pub opacity: f64,
    pub click_through: bool,
}

impl Default for OverlayPrefs {
    fn default() -> Self {
        Self { x: None, y: None, w: 272.0, h: 166.0, opacity: 0.78, click_through: false }
    }
}

fn prefs_path(app: &AppHandle) -> std::path::PathBuf {
    app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("Slowpoke")).join("overlay.json")
}

fn save_prefs(app: &AppHandle) {
    if let Some(s) = app.try_state::<Arc<Shared>>() {
        let p = s.overlay.lock().unwrap().clone();
        let path = prefs_path(app);
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(path, serde_json::to_vec_pretty(&p).unwrap_or_default());
    }
}

/// Glass look: Acrylic blur behind the window plus Windows 11 rounded corners.
fn glass(w: &tauri::WebviewWindow) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
    let _ = window_vibrancy::apply_acrylic(w, Some((12, 14, 20, 90)));
    if let Ok(h) = w.window_handle() {
        if let RawWindowHandle::Win32(h) = h.as_raw() {
            let pref = DWMWCP_ROUND;
            unsafe {
                let _ = DwmSetWindowAttribute(HWND(h.hwnd.get() as *mut core::ffi::c_void), DWMWA_WINDOW_CORNER_PREFERENCE, &pref as *const _ as *const core::ffi::c_void, std::mem::size_of_val(&pref) as u32);
            }
        }
    }
}

fn open_overlay(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.show();
        return;
    }
    let Some(shared) = app.try_state::<Arc<Shared>>() else { return };
    let p = shared.overlay.lock().unwrap().clone();
    let Ok(w) = WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("index.html#overlay".into()))
        .title("Slowpoke overlay")
        .inner_size(p.w, p.h)
        .min_inner_size(200.0, 100.0)
        .max_inner_size(560.0, 340.0)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .resizable(true)
        .focused(false)
        .visible(false)
        .build()
    else {
        return;
    };
    // restore the last position if it's still on a screen, else top-right of the primary work area
    let on_screen = |x: i32, y: i32| w.available_monitors().map(|ms| ms.iter().any(|m| {
        let (mp, ms) = (m.position(), m.size());
        x >= mp.x - 40 && y >= mp.y - 10 && x < mp.x + ms.width as i32 - 40 && y < mp.y + ms.height as i32 - 40
    })).unwrap_or(false);
    match (p.x, p.y) {
        (Some(x), Some(y)) if on_screen(x, y) => {
            let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
        }
        _ => {
            if let (Ok(Some(m)), Ok(size)) = (w.primary_monitor(), w.outer_size()) {
                let wa = m.work_area();
                let margin = (20.0 * m.scale_factor()) as i32;
                let _ = w.set_position(tauri::PhysicalPosition::new(wa.position.x + wa.size.width as i32 - size.width as i32 - margin, wa.position.y + margin));
            }
        }
    }
    glass(&w);
    if p.click_through {
        let _ = w.set_ignore_cursor_events(true);
    }
    let h = app.clone();
    w.on_window_event(move |e| {
        let Some(s) = h.try_state::<Arc<Shared>>() else { return };
        match e {
            tauri::WindowEvent::Moved(pos) => {
                let mut p = s.overlay.lock().unwrap();
                p.x = Some(pos.x);
                p.y = Some(pos.y);
            }
            tauri::WindowEvent::Resized(size) => {
                if let Some(win) = h.get_webview_window("overlay") {
                    let f = win.scale_factor().unwrap_or(1.0);
                    let mut p = s.overlay.lock().unwrap();
                    p.w = (size.width as f64 / f).round();
                    p.h = (size.height as f64 / f).round();
                }
            }
            tauri::WindowEvent::Destroyed => save_prefs(&h),
            _ => {}
        }
    });
    let _ = w.show();
}

fn toggle_overlay_inner(app: &AppHandle) {
    match app.get_webview_window("overlay") {
        Some(w) => {
            save_prefs(app);
            let _ = w.destroy(); // frees its WebView
        }
        None => open_overlay(app),
    }
}

fn set_click_through_inner(app: &AppHandle, on: bool) {
    if let Some(s) = app.try_state::<Arc<Shared>>() {
        s.overlay.lock().unwrap().click_through = on;
    }
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.set_ignore_cursor_events(on);
        let _ = app.emit_to("overlay", "overlay-prefs", app.state::<Arc<Shared>>().overlay.lock().unwrap().clone());
    }
    if let Some(item) = app.try_state::<MenuItem<tauri::Wry>>() {
        let _ = item.set_enabled(on);
    }
    save_prefs(app);
}

fn sampler_loop(app: AppHandle, shared: Arc<Shared>) {
    let mut s = sampler::Sampler::new();
    let mut svc = services::services_by_pid();
    let mut svc_at = Instant::now();
    let mut labels: HashMap<String, Option<String>> = HashMap::new();
    let me = std::process::id();
    let mut n: u64 = 0;
    loop {
        let t0 = Instant::now();
        let sample = s.sample();
        if svc_at.elapsed() > Duration::from_secs(30) {
            svc = services::services_by_pid();
            svc_at = Instant::now();
        }
        // remember one path per exe (for icons, product names and "open location")
        {
            let mut paths = shared.paths.lock().unwrap();
            for p in sample.procs.iter().filter(|p| p.cpu > 0.5 || p.io > 0 || p.mem > 200 << 20) {
                if !paths.contains_key(&p.name) {
                    if let Some(path) = winutil::process_path(p.pid) {
                        paths.insert(p.name.clone(), path);
                    }
                }
            }
        }
        let paths = shared.paths.lock().unwrap().clone();
        let v = verdict::judge(&sample, &svc, &|exe, _pid| {
            labels.get(exe).cloned().unwrap_or_else(|| paths.get(exe).and_then(|p| winutil::product_name(p)))
        });
        for c in &v.culprits {
            labels.entry(c.exe.clone()).or_insert_with(|| paths.get(&c.exe).and_then(|p| winutil::product_name(p)));
        }

        // self-cost: our process and the WebView2 processes we spawned
        let kids: Vec<&sampler::Proc> = sample.procs.iter().filter(|p| p.pid == me || (p.ppid == me && p.name.starts_with("msedgewebview2"))).collect();
        let mut kid_tree: Vec<u32> = kids.iter().map(|p| p.pid).collect();
        for p in &sample.procs {
            if kid_tree.contains(&p.ppid) && p.name.starts_with("msedgewebview2") && !kid_tree.contains(&p.pid) {
                kid_tree.push(p.pid);
            }
        }
        let mine: Vec<&sampler::Proc> = sample.procs.iter().filter(|p| kid_tree.contains(&p.pid)).collect();
        let cost = SelfCost {
            cpu: mine.iter().map(|p| p.cpu).sum(),
            mem: mine.iter().map(|p| p.mem).sum(),
            window_open: app.get_webview_window("main").is_some() || app.get_webview_window("overlay").is_some(),
        };

        let netv = shared.net.lock().unwrap().clone();
        let o = overall::combine(&v, netv.as_ref());

        // tray
        let lv = level_u8(o.level);
        if let Some(tray) = app.tray_by_id("tray") {
            if shared.level.swap(lv, Ordering::Relaxed) != lv || n == 0 {
                let _ = tray.set_icon(Some(tray_icon(lv)));
            }
            let tip: String = format!("Slowpoke · {}", o.headline).chars().take(120).collect();
            let _ = tray.set_tooltip(Some(tip));
        }

        if cost.window_open {
            let payload = Now { verdict: v.clone(), net: netv.clone(), overall: o.clone(), cost: cost.clone() };
            for label in ["main", "overlay"] {
                if app.get_webview_window(label).is_some() {
                    let _ = app.emit_to(label, "now", payload.clone());
                }
            }
        }
        {
            let mut h = shared.history.lock().unwrap();
            h.push(&v);
            if n % 30 == 0 {
                h.flush();
            }
        }
        *shared.latest.lock().unwrap() = Some(v);
        *shared.cost.lock().unwrap() = cost;
        n += 1;
        std::thread::sleep(INTERVAL.saturating_sub(t0.elapsed()));
    }
}

/// The network checks run on their own thread: a ping can wait for its timeout, and that must never delay
/// the PC sampler. Every 3 s: probe each hop, judge, record.
fn net_loop(shared: Arc<Shared>) {
    let mut mon = net::monitor::Monitor::new();
    let mut n: u64 = 0;
    loop {
        let t0 = Instant::now();
        let v = net::verdict::judge(&mon.tick());
        {
            let mut h = shared.net_history.lock().unwrap();
            h.push(net::history::Rec::from(&v));
            if n % 20 == 0 {
                h.flush();
            }
        }
        *shared.net.lock().unwrap() = Some(v);
        n += 1;
        std::thread::sleep(net::monitor::TICK.saturating_sub(t0.elapsed()));
    }
}

// ---------------------------------------------------------------- commands

#[tauri::command]
fn get_now(state: tauri::State<'_, Arc<Shared>>) -> Option<Now> {
    let v = state.latest.lock().unwrap().clone()?;
    let net = state.net.lock().unwrap().clone();
    let overall = overall::combine(&v, net.as_ref());
    Some(Now { verdict: v, net, overall, cost: state.cost.lock().unwrap().clone() })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Timeline {
    /// [t (unix s), cpu, mem, disk]
    points: Vec<[u32; 4]>,
    moments: Vec<Moment>,
    from: u32,
    to: u32,
}

#[tauri::command]
fn get_timeline(hours: u32, state: tauri::State<'_, Arc<Shared>>) -> Timeline {
    let to = (now_ms() / 1000) as u32;
    let from = to.saturating_sub(hours.clamp(1, 168) * 3600);
    let h = state.history.lock().unwrap();
    let recs: Vec<_> = h.range(from).collect();
    // at most ~720 points: bucket by max (spikes must stay visible)
    let bucket = ((to - from) / 720).max(2);
    let mut points: Vec<[u32; 4]> = Vec::new();
    for r in recs {
        let b = r.t - r.t % bucket;
        match points.last_mut() {
            Some(p) if p[0] == b => {
                p[1] = p[1].max(r.cpu as u32);
                p[2] = p[2].max(r.mem as u32);
                p[3] = p[3].max(r.disk as u32);
            }
            _ => points.push([b, r.cpu as u32, r.mem as u32, r.disk as u32]),
        }
    }
    let mut moments = h.moments(from, 10);
    moments.reverse(); // newest first
    Timeline { points, moments, from, to }
}

#[tauri::command]
fn get_icon(exe: String, state: tauri::State<'_, Arc<Shared>>) -> Option<String> {
    let path = state.paths.lock().unwrap().get(&exe).cloned()?;
    winutil::icon_data_url(&path, 32)
}

/// End every process of a culprit group. Refuses critical processes, and anything not in the current verdict.
#[tauri::command]
fn end_task(key: String, state: tauri::State<'_, Arc<Shared>>) -> Result<usize, String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    let latest = state.latest.lock().unwrap().clone().ok_or("No data yet")?;
    let c = latest.culprits.iter().find(|c| c.key == key).ok_or("That app isn't running any more.")?;
    if c.critical || catalog::is_critical(&c.exe) {
        return Err(format!("{} is part of Windows and can't be ended safely.", c.name));
    }
    let me = std::process::id();
    let mut ended = 0;
    for pid in c.pids.iter().filter(|p| **p != me && **p > 4) {
        unsafe {
            if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, *pid) {
                if TerminateProcess(h, 1).is_ok() {
                    ended += 1;
                }
                let _ = CloseHandle(h);
            }
        }
    }
    if ended == 0 {
        return Err(format!("Windows didn't allow ending {}. It may need administrator rights.", c.name));
    }
    Ok(ended)
}

#[tauri::command]
fn open_location(exe: String, state: tauri::State<'_, Arc<Shared>>) -> Result<(), String> {
    let path = state.paths.lock().unwrap().get(&exe).cloned().ok_or("Location unknown (protected process).")?;
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| e.to_string())
}

#[tauri::command]
fn search_web(query: String) -> Result<(), String> {
    let q: String = query.chars().filter(|c| c.is_alphanumeric() || " .-_".contains(*c)).collect();
    tauri_plugin_opener::open_url(format!("https://www.bing.com/search?q={}", q.replace(' ', "+") + "+what+is+it+windows+process"), None::<&str>)
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NetTimeline {
    /// [t, health, routerMs, internetMs, internetLoss, dnsMs, signal, rx (tenths of Mbps)]; 65535/255 = none
    points: Vec<[u32; 8]>,
    moments: Vec<net::history::Moment>,
    from: u32,
    to: u32,
}

#[tauri::command]
fn get_net_timeline(hours: u32, state: tauri::State<'_, Arc<Shared>>) -> NetTimeline {
    let to = (now_ms() / 1000) as u32;
    let from = to.saturating_sub(hours.clamp(1, 168) * 3600);
    let h = state.net_history.lock().unwrap();
    let bucket = ((to - from) / 720).max(3);
    let mut points: Vec<[u32; 8]> = Vec::new();
    for r in h.range(from) {
        let b = r.t - r.t % bucket;
        let p = [b, r.health as u32, r.router_ms as u32, r.inet_ms as u32, r.inet_loss as u32, r.dns_ms as u32, r.signal as u32, r.rx as u32];
        match points.last_mut() {
            // keep the worst value per bucket so problems stay visible
            Some(last) if last[0] == b => {
                last[1] = last[1].max(p[1]);
                for i in [2, 3, 5] {
                    last[i] = if last[i] == 65535 { p[i] } else if p[i] == 65535 { last[i] } else { last[i].max(p[i]) };
                }
                last[4] = last[4].max(p[4]);
                last[6] = if last[6] == 255 { p[6] } else if p[6] == 255 { last[6] } else { last[6].min(p[6]) };
                last[7] = last[7].max(p[7]);
            }
            _ => points.push(p),
        }
    }
    NetTimeline { points, moments: h.moments(from, 15), from, to }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SpeedResult {
    mbps: f64,
    bytes: u64,
    seconds: f64,
    server: String,
}

/// Manual download test: ~25 MB from Cloudflare's speed endpoint (stops early after 12 s).
#[tauri::command]
async fn speed_test(state: tauri::State<'_, Arc<Shared>>) -> Result<SpeedResult, String> {
    if state.speed_running.swap(true, Ordering::SeqCst) {
        return Err("A speed test is already running.".into());
    }
    let res = tauri::async_runtime::spawn_blocking(|| -> Result<SpeedResult, String> {
        use std::io::Read;
        let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(5)).timeout_read(Duration::from_secs(10)).build();
        // warm-up connection (TLS handshake) so it doesn't count against the measurement
        let _ = agent.get("https://speed.cloudflare.com/__down?bytes=1000").call();
        let resp = agent.get("https://speed.cloudflare.com/__down?bytes=25000000").call().map_err(|e| format!("Couldn't reach the speed-test server: {e}"))?;
        let mut reader = resp.into_reader();
        let mut buf = vec![0u8; 64 * 1024];
        let t0 = Instant::now();
        let mut total = 0u64;
        loop {
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            total += n as u64;
            if n == 0 || t0.elapsed().as_secs_f64() > 12.0 {
                break;
            }
        }
        let secs = t0.elapsed().as_secs_f64().max(0.001);
        Ok(SpeedResult { mbps: total as f64 * 8.0 / secs / 1e6, bytes: total, seconds: secs, server: "speed.cloudflare.com".into() })
    })
    .await;
    // always release the flag, even if the worker panicked
    state.speed_running.store(false, Ordering::SeqCst);
    res.map_err(|e| e.to_string())?
}

#[tauri::command]
fn open_network_settings() {
    let _ = tauri_plugin_opener::open_url("ms-settings:network-status", None::<&str>);
}

#[tauri::command]
fn quit(app: AppHandle) {
    if let Some(s) = app.try_state::<Arc<Shared>>() {
        s.history.lock().unwrap().flush();
        s.net_history.lock().unwrap().flush();
    }
    save_prefs(&app);
    app.exit(0);
}

#[tauri::command]
fn toggle_overlay(app: AppHandle) {
    toggle_overlay_inner(&app);
}

#[tauri::command]
fn open_main(app: AppHandle) {
    open_window(&app);
}

#[tauri::command]
fn get_overlay_prefs(state: tauri::State<'_, Arc<Shared>>) -> OverlayPrefs {
    state.overlay.lock().unwrap().clone()
}

#[tauri::command]
fn set_overlay_opacity(app: AppHandle, opacity: f64, state: tauri::State<'_, Arc<Shared>>) {
    state.overlay.lock().unwrap().opacity = opacity.clamp(0.3, 1.0);
    save_prefs(&app);
}

#[tauri::command]
fn set_click_through(app: AppHandle, on: bool) {
    set_click_through_inner(&app, on);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| open_window(app)))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("Slowpoke"));
            let shared = Arc::new(Shared {
                latest: Mutex::new(None),
                cost: Mutex::new(SelfCost::default()),
                history: Mutex::new(History::open(&dir.join("history"))),
                paths: Mutex::new(HashMap::new()),
                level: AtomicU8::new(0),
                overlay: Mutex::new(
                    std::fs::read(dir.join("overlay.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default(),
                ),
                net: Mutex::new(None),
                net_history: Mutex::new(net::history::History::open(&dir.join("net-history"))),
                speed_running: AtomicBool::new(false),
            });
            app.manage(shared.clone());

            let click_through_on = shared.overlay.lock().unwrap().click_through;
            let open_i = MenuItem::with_id(app, "open", "Open Slowpoke", true, None::<&str>)?;
            let overlay_i = MenuItem::with_id(app, "overlay", "Show / hide mini overlay", true, None::<&str>)?;
            let ct_i = MenuItem::with_id(app, "click-through-off", "Overlay: turn click-through off", click_through_on, None::<&str>)?;
            let net_i = MenuItem::with_id(app, "net", "Windows network settings", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            app.manage(ct_i.clone());
            TrayIconBuilder::with_id("tray")
                .icon(tray_icon(0))
                .tooltip("Slowpoke")
                .menu(&Menu::with_items(app, &[&open_i, &overlay_i, &ct_i, &net_i, &PredefinedMenuItem::separator(app)?, &quit_i])?)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "open" => open_window(app),
                    "overlay" => toggle_overlay_inner(app),
                    "click-through-off" => set_click_through_inner(app, false),
                    "net" => open_network_settings(),
                    "quit" => quit(app.clone()),
                    _ => {}
                })
                .on_tray_icon_event(|tray, e| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                        open_window(tray.app_handle());
                    }
                })
                .build(app)?;

            let h = app.handle().clone();
            let ns = shared.clone();
            std::thread::Builder::new().name("sampler".into()).spawn(move || sampler_loop(h, shared))?;
            std::thread::Builder::new().name("network".into()).spawn(move || net_loop(ns))?;
            let args: Vec<String> = std::env::args().collect();
            if args.iter().any(|a| a == "--overlay") {
                open_overlay(app.handle()); // start straight into the mini overlay
            } else if !args.iter().any(|a| a == "--minimized") {
                open_window(app.handle());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_now, get_timeline, get_icon, end_task, open_location, search_web, quit,
            toggle_overlay, open_main, get_overlay_prefs, set_overlay_opacity, set_click_through,
            get_net_timeline, speed_test, open_network_settings
        ])
        .build(tauri::generate_context!())
        .expect("error while building Slowpoke")
        .run(|_app, e| {
            // closing the window destroys it (freeing the WebView) but Slowpoke keeps running in the tray
            if let tauri::RunEvent::ExitRequested { api, code: None, .. } = e {
                api.prevent_exit();
            }
        });
}
