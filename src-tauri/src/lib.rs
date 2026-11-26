pub mod catalog;
pub mod history;
pub mod sampler;
pub mod services;
pub mod verdict;
pub mod winutil;

use history::{History, Moment};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use verdict::{Level, Verdict};

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

const INTERVAL: Duration = Duration::from_secs(2);

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SelfCost {
    /// Culprit's own CPU (sampler + UI), % of the machine
    pub cpu: f32,
    /// private memory of Culprit and its WebView processes, bytes
    pub mem: u64,
    pub window_open: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Now {
    pub verdict: Verdict,
    pub cost: SelfCost,
}

struct Shared {
    latest: Mutex<Option<Verdict>>,
    cost: Mutex<SelfCost>,
    history: Mutex<History>,
    /// exe → path of one running instance (for icons / open location)
    paths: Mutex<HashMap<String, String>>,
    level: AtomicU8,
}

fn level_u8(l: Level) -> u8 {
    match l {
        Level::Calm => 0,
        Level::Busy => 1,
        Level::Strained => 2,
    }
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
    // created on demand and destroyed on close, so the WebView costs nothing while Culprit sits in the tray
    let _ = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Culprit")
        .inner_size(1040.0, 720.0)
        .min_inner_size(760.0, 520.0)
        .build();
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
            window_open: app.get_webview_window("main").is_some(),
        };

        // tray
        let lv = level_u8(v.level);
        if let Some(tray) = app.tray_by_id("tray") {
            if shared.level.swap(lv, Ordering::Relaxed) != lv || n == 0 {
                let _ = tray.set_icon(Some(tray_icon(lv)));
            }
            let tip: String = format!("Culprit · {}", v.headline).chars().take(120).collect();
            let _ = tray.set_tooltip(Some(tip));
        }

        if cost.window_open {
            let _ = app.emit_to("main", "now", Now { verdict: v.clone(), cost: cost.clone() });
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

// ---------------------------------------------------------------- commands

#[tauri::command]
fn get_now(state: tauri::State<'_, Arc<Shared>>) -> Option<Now> {
    let v = state.latest.lock().unwrap().clone()?;
    Some(Now { verdict: v, cost: state.cost.lock().unwrap().clone() })
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

#[tauri::command]
fn quit(app: AppHandle) {
    if let Some(s) = app.try_state::<Arc<Shared>>() {
        s.history.lock().unwrap().flush();
    }
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| open_window(app)))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("Culprit"));
            let shared = Arc::new(Shared {
                latest: Mutex::new(None),
                cost: Mutex::new(SelfCost::default()),
                history: Mutex::new(History::open(&dir.join("history"))),
                paths: Mutex::new(HashMap::new()),
                level: AtomicU8::new(0),
            });
            app.manage(shared.clone());

            let open_i = MenuItem::with_id(app, "open", "Open Culprit", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            TrayIconBuilder::with_id("tray")
                .icon(tray_icon(0))
                .tooltip("Culprit")
                .menu(&Menu::with_items(app, &[&open_i, &quit_i])?)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "open" => open_window(app),
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
            std::thread::Builder::new().name("sampler".into()).spawn(move || sampler_loop(h, shared))?;
            if !std::env::args().any(|a| a == "--minimized") {
                open_window(app.handle());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_now, get_timeline, get_icon, end_task, open_location, search_web, quit])
        .build(tauri::generate_context!())
        .expect("error while building Culprit")
        .run(|_app, e| {
            // closing the window destroys it (freeing the WebView) but Culprit keeps running in the tray
            if let tauri::RunEvent::ExitRequested { api, code: None, .. } = e {
                api.prevent_exit();
            }
        });
}
