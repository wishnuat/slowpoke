//! Plain-language catalogue: executable / service name → what it is, and whether heavy use is normal.

pub struct Entry {
    pub name: &'static str,
    pub what: &'static str,
    pub advice: &'static str,
    /// Never offer "End task".
    pub critical: bool,
}

const fn e(name: &'static str, what: &'static str, advice: &'static str, critical: bool) -> Entry {
    Entry { name, what, advice, critical }
}

/// By lower-case executable name.
pub fn by_exe(exe: &str) -> Option<Entry> {
    Some(match exe {
        // ---- Windows core (critical)
        "system" => e("Windows kernel", "The core of Windows, including drivers and file-system work.", "High use usually means a driver or heavy disk activity. A restart often helps if it stays high.", true),
        "registry" | "memory compression" => e("Windows memory manager", "Compresses memory so more apps fit in RAM.", "Busy when RAM is nearly full. Closing a few apps or tabs helps.", true),
        "smss.exe" | "csrss.exe" | "wininit.exe" | "winlogon.exe" | "services.exe" | "lsass.exe" | "lsaiso.exe" | "fontdrvhost.exe" => {
            e("Windows system process", "A core part of Windows that has to keep running.", "Normally near zero. Sustained load can point to malware or a broken driver.", true)
        }
        "dwm.exe" => e("Desktop Window Manager", "Draws every window, animation and transparency effect.", "Busy with high refresh rates, several monitors or games in windowed mode.", true),
        "audiodg.exe" => e("Windows audio engine", "Mixes and processes all sound.", "Heavy use usually comes from audio enhancements or effects.", true),
        "svchost.exe" => e("Windows services", "A host for several background Windows services.", "Culprit names the services inside, which matters more than the host.", true),
        "explorer.exe" => e("File Explorer & taskbar", "The desktop, taskbar, Start menu and file windows.", "Busy while copying files or showing folders with many thumbnails.", false),
        "searchindexer.exe" | "searchprotocolhost.exe" | "searchfilterhost.exe" => {
            e("Windows Search indexing", "Reads your files so that Start-menu search can find them.", "Normal after many new files. If it never stops, exclude large data folders from indexing.", false)
        }
        "msmpeng.exe" | "mpdefendercoreservice.exe" | "nissrv.exe" | "mpcmdrun.exe" => {
            e("Microsoft Defender", "Windows' built-in antivirus, scanning files.", "Normal during scheduled scans and after installing or extracting many files. It slows down by itself.", true)
        }
        "tiworker.exe" | "trustedinstaller.exe" | "wuauclt.exe" | "usocoreworker.exe" | "mousocoreworker.exe" | "sihclient.exe" => {
            e("Windows Update", "Downloads and installs Windows updates.", "Normal for a while after updates arrive. Letting it finish (and restarting) makes it go away.", false)
        }
        "compattelrunner.exe" | "devicecensus.exe" => e("Windows telemetry", "Collects compatibility and diagnostics data for Microsoft.", "Runs occasionally and finishes by itself.", false),
        "wmiprvse.exe" => e("WMI provider", "Answers system-information requests from other apps.", "Constant load means some app keeps asking. Look at what else is busy.", false),
        "runtimebroker.exe" => e("Runtime Broker", "Handles permissions for Store apps.", "Usually small. Heavy use points to a misbehaving Store app.", false),
        "sppsvc.exe" => e("Windows licensing", "Checks Windows activation.", "Runs briefly and stops.", false),
        "dllhost.exe" => e("COM surrogate", "Runs helpers such as thumbnail generators.", "Busy while Explorer generates thumbnails for photo or video folders.", false),
        "ctfmon.exe" | "textinputhost.exe" => e("Text input", "Keyboard, emoji panel and input methods.", "Normally near zero.", false),
        "sgrmbroker.exe" | "securityhealthservice.exe" => e("Windows Security", "System integrity and security health.", "Normally near zero.", true),
        "vmmem" | "vmmemwsl" | "vmcompute.exe" => e("Virtual machines / WSL", "Linux (WSL), Docker or other VMs running in the background.", "Can hold several GB of memory. `wsl --shutdown` frees it when you don't need it.", false),
        "onedrive.exe" => e("OneDrive", "Syncs your files with the cloud.", "Busy while uploading or downloading many files. You can pause syncing from its tray icon.", false),
        "msedgewebview2.exe" => e("Edge WebView", "The browser engine embedded in apps such as Teams, Widgets and Outlook.", "Its load belongs to the app that uses it.", false),
        // ---- apps
        "chrome.exe" => e("Google Chrome", "Every tab and extension runs as its own process.", "Close heavy tabs. Chrome's own Task Manager (Shift+Esc) shows which tab is busy.", false),
        "msedge.exe" => e("Microsoft Edge", "Every tab and extension runs as its own process.", "Close heavy tabs. Edge's Task Manager (Shift+Esc) shows which tab is busy.", false),
        "firefox.exe" => e("Firefox", "The Firefox browser.", "about:processes in Firefox shows which tab is busy.", false),
        "brave.exe" => e("Brave", "The Brave browser.", "Close heavy tabs. Shift+Esc shows which tab is busy.", false),
        "opera.exe" => e("Opera", "The Opera browser.", "Close heavy tabs.", false),
        "ms-teams.exe" | "teams.exe" => e("Microsoft Teams", "Chats, calls and meetings.", "Video calls are heavy by nature. Turning off background effects helps.", false),
        "zoom.exe" => e("Zoom", "Video meetings.", "Video and virtual backgrounds use a lot of CPU.", false),
        "discord.exe" => e("Discord", "Chat and voice.", "Hardware acceleration and streaming increase load.", false),
        "slack.exe" => e("Slack", "Team chat.", "Many workspaces and channels increase memory use.", false),
        "whatsapp.exe" | "whatsapp.root.exe" => e("WhatsApp", "WhatsApp for Windows.", "Busy while syncing media.", false),
        "spotify.exe" => e("Spotify", "Music streaming.", "Normally light.", false),
        "outlook.exe" | "olk.exe" => e("Outlook", "Email and calendar.", "Busy while syncing or indexing a large mailbox.", false),
        "excel.exe" => e("Excel", "Spreadsheets.", "Large workbooks with many formulas recalculate on every change. Set calculation to Manual for huge files.", false),
        "winword.exe" => e("Word", "Documents.", "Large documents with many images can be slow.", false),
        "powerpnt.exe" => e("PowerPoint", "Presentations.", "Embedded videos and big images increase load.", false),
        "code.exe" => e("Visual Studio Code", "Code editor; extensions run in their own processes.", "Busy language servers or extensions are the usual cause. Try 'Developer: Show Running Extensions'.", false),
        "devenv.exe" => e("Visual Studio", "IDE.", "Busy while building or indexing a solution.", false),
        "java.exe" | "javaw.exe" => e("Java app", "A program running on Java (for example Android Studio, Minecraft or a build tool).", "Check which Java app it is from its folder.", false),
        "studio64.exe" => e("Android Studio", "Android IDE.", "Gradle syncs and the emulator are heavy.", false),
        "qemu-system-x86_64.exe" => e("Android emulator", "A virtual Android phone.", "Very heavy by nature. Close it when not in use.", false),
        "node.exe" => e("Node.js", "A JavaScript program or dev server.", "Usually a dev server or build tool you started.", false),
        "python.exe" | "pythonw.exe" => e("Python", "A Python script or app.", "Usually a script you started.", false),
        "docker desktop.exe" | "com.docker.backend.exe" => e("Docker Desktop", "Container platform (uses WSL).", "Stop Docker when you don't need containers.", false),
        "steam.exe" | "steamwebhelper.exe" => e("Steam", "Game store and launcher.", "Busy while downloading or updating games.", false),
        "epicgameslauncher.exe" => e("Epic Games Launcher", "Game store and launcher.", "Busy while downloading or updating games.", false),
        "obs64.exe" => e("OBS Studio", "Recording and streaming.", "Encoding video is heavy. A hardware encoder (NVENC/QuickSync) lowers CPU use.", false),
        "anydesk.exe" | "rustdesk.exe" | "teamviewer.exe" => e("Remote desktop", "Remote access tool.", "Busy while someone is connected.", false),
        "culprit.exe" => e("Culprit", "This app.", "It should stay under 1 % CPU. If not, please report it.", true),
        _ => return None,
    })
}

/// Friendly names for common services hosted in svchost.exe.
pub fn service(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match name.to_ascii_lowercase().as_str() {
        "wuauserv" | "usosvc" | "bits" | "dosvc" => ("Windows Update", "Downloads and installs updates."),
        "wsearch" => ("Windows Search", "Indexes files for search."),
        "sysmain" => ("SysMain (Superfetch)", "Preloads apps into memory. It can cause heavy disk use on slow drives."),
        "diagtrack" | "dps" | "wdisystemhost" | "wdiservicehost" => ("Diagnostics & telemetry", "Collects diagnostic data."),
        "winmgmt" => ("WMI", "Answers system-information requests from other apps."),
        "eventlog" => ("Event Log", "Writes Windows logs."),
        "cryptsvc" => ("Cryptographic Services", "Verifies signatures and certificates; busy during updates."),
        "themes" | "uxsms" => ("Themes", "Visual styles."),
        "audiosrv" | "audioendpointbuilder" => ("Windows Audio", "Sound devices."),
        "dnscache" | "nlasvc" | "netprofm" | "dhcp" | "wlansvc" => ("Networking", "Network connection services."),
        "schedule" => ("Task Scheduler", "Runs scheduled maintenance tasks."),
        "spooler" => ("Print Spooler", "Printing."),
        "cdpsvc" | "cdpusersvc" => ("Connected Devices Platform", "Phone Link and nearby sharing."),
        "lfsvc" => ("Location", "Location services."),
        "bfe" | "mpssvc" => ("Windows Firewall", "Filters network traffic."),
        "storsvc" => ("Storage Service", "Storage maintenance, including Storage Sense."),
        "trkwks" => ("Distributed Link Tracking", "Keeps shortcuts working when files move."),
        "fontcache" => ("Font Cache", "Speeds up font loading."),
        _ => return None,
    })
}

pub fn is_critical(exe: &str) -> bool {
    by_exe(exe).is_some_and(|e| e.critical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_common_processes() {
        assert_eq!(by_exe("msmpeng.exe").unwrap().name, "Microsoft Defender");
        assert_eq!(by_exe("tiworker.exe").unwrap().name, "Windows Update");
        assert!(by_exe("chrome.exe").is_some());
        assert!(by_exe("mystery.exe").is_none());
        assert_eq!(service("WSearch").unwrap().0, "Windows Search");
    }

    #[test]
    fn protects_critical_processes() {
        for p in ["csrss.exe", "lsass.exe", "svchost.exe", "dwm.exe", "system", "msmpeng.exe", "culprit.exe"] {
            assert!(is_critical(p), "{p}");
        }
        assert!(!is_critical("chrome.exe"));
        assert!(!is_critical("unknown.exe"), "unknown apps may be ended (after confirmation)");
    }
}
