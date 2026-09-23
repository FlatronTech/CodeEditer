#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

#[derive(Clone, Copy, PartialEq)]
enum NewTemplate {
    Plain,
    Python,
    Cpp,
    Rust,
    C,
    CSharp,
    JavaScript,
    Java,
    Go,
    Html,
    Batch,
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
enum AppTheme {
    Dark,
    Light,
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
enum AppLanguage {
    English,
    Polish,
}

#[derive(Serialize, Deserialize)]
struct EditorSettings {
    theme: AppTheme,
    language: AppLanguage,
}

#[derive(Clone, Serialize, Deserialize)]
struct Extension {
    id: String,
    name: String,
    description: String,
    long_description: String,
    author: String,
    version: String,
    installed: bool,
    #[serde(default)]
    verified: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct ExtensionsState {
    installed_ids: Vec<String>,
}

struct LiveServerShared {
    version: AtomicU64,
    active_content: Mutex<String>,
    active_path: Mutex<Option<PathBuf>>,
    workspace_folder: Mutex<Option<PathBuf>>,
    tabs: Mutex<HashMap<String, String>>,
}

#[derive(Clone, PartialEq)]
enum ProblemSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Clone)]
struct CodeProblem {
    line: usize,
    message: String,
    severity: ProblemSeverity,
}

#[derive(Clone)]
struct CachedDirNode {
    path: PathBuf,
    name: String,
    is_dir: bool,
    children: Vec<CachedDirNode>,
}

#[derive(Clone)]
enum UpdateState {
    Idle,
    Checking,
    Available(String, String), // Version, URL to 7z
    Downloading(u32),          // Progress %
    ReadyToInstall(PathBuf),   // Path to unzipped temp dir
    Error(String),
}

#[derive(Clone)]
struct IntellisenseState {
    active: bool,
    prefix: String,
    suggestions: Vec<String>,
    selected_index: usize,
    replace_range: std::ops::Range<usize>,
    pos: egui::Pos2,
}

impl Default for IntellisenseState {
    fn default() -> Self {
        Self {
            active: false,
            prefix: String::new(),
            suggestions: vec![],
            selected_index: 0,
            replace_range: 0..0,
            pos: egui::Pos2::ZERO,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct FileTab {
    path: Option<PathBuf>,
    name: String,
    content: String,
    is_modified: bool,
    #[serde(skip)]
    undo_stack: Vec<String>,
    #[serde(skip)]
    redo_stack: Vec<String>,
    #[serde(skip)]
    snapshot: String,
    #[serde(skip)]
    cached_layout: Option<(String, String, egui::text::LayoutJob)>,
    #[serde(skip)]
    cached_line_numbers: String,
    #[serde(skip)]
    cached_line_count: usize,
}

impl FileTab {
    fn new(path: Option<PathBuf>, name: String, content: String) -> Self {
        let snapshot = content.clone();
        Self {
            path,
            name,
            content,
            is_modified: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            snapshot,
            cached_layout: None,
            cached_line_numbers: String::new(),
            cached_line_count: 0,
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
struct Session {
    tabs: Vec<FileTab>,
    active_tab_index: usize,
    workspace_folder: Option<PathBuf>,
}

struct ResponseData {
    body: Vec<u8>,
    mime: &'static str,
}

/// Resolves path relative to the executable's directory, fixing Windows file association issues.
fn get_asset_path(relative: &str) -> PathBuf {
    if let Ok(mut path) = env::current_exe() {
        path.pop(); // Remove executable name
        let target = path.join(relative);
        if target.exists() {
            return target;
        }
        // Fallback for development (cargo run)
        if let Ok(mut dev_path) = env::current_dir() {
            dev_path.push(relative);
            if dev_path.exists() {
                return dev_path;
            }
        }
    }
    PathBuf::from(relative)
}

fn get_exe_dir() -> PathBuf {
    if let Ok(mut path) = env::current_exe() {
        path.pop();
        path
    } else {
        PathBuf::from(".")
    }
}

fn start_live_server(shared: Arc<LiveServerShared>, running_flag: Arc<AtomicBool>) {
    if running_flag.swap(true, Ordering::SeqCst) {
        open_browser("http://localhost:8080");
        return;
    }
    thread::spawn(move || {
        let listener = match TcpListener::bind("127.0.0.1:8080") {
            Ok(l) => l,
            Err(_) => {
                running_flag.store(false, Ordering::SeqCst);
                return;
            }
        };
        listener.set_nonblocking(true).ok();
        loop {
            if !running_flag.load(Ordering::SeqCst) {
                break;
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let shared = shared.clone();
                    thread::spawn(move || {
                        let mut buffer = [0u8; 8192];
                        if let Ok(n) = stream.read(&mut buffer) {
                            if n == 0 {
                                return;
                            }
                            let req = String::from_utf8_lossy(&buffer[..n]);
                            let mut lines = req.lines();
                            if let Some(first_line) = lines.next() {
                                let parts: Vec<&str> = first_line.split_whitespace().collect();
                                if parts.len() >= 2 {
                                    let path = parts[1];
                                    if path == "/__livereload" {
                                        let ver = shared.version.load(Ordering::SeqCst);
                                        let response = format!(
                                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nAccess-Control-Allow-Origin: *\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n{}",
                                            ver
                                        );
                                        let _ = stream.write_all(response.as_bytes());
                                        return;
                                    }
                                    let clean_path = path.trim_start_matches('/');
                                    let mut res_data = get_response_bytes(&shared, clean_path);
                                    if res_data.mime.starts_with("text/html") {
                                        let mut body_str = String::from_utf8_lossy(&res_data.body).into_owned();
                                        let script = r#"
<script>
(function() {
let lastVer = null;
setInterval(async () => {
try {
let res = await fetch('/__livereload');
if (res.ok) {
let ver = await res.text();
if (lastVer !== null && lastVer !== ver) {
location.reload();
}
lastVer = ver;
}
} catch(e) {}
}, 400);
})();
</script>
"#;
                                        if let Some(idx) = body_str.rfind("</body>") {
                                            body_str.insert_str(idx, script);
                                        } else {
                                            body_str.push_str(script);
                                        }
                                        res_data.body = body_str.into_bytes();
                                    }
                                    let header = format!(
                                        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
                                        res_data.mime,
                                        res_data.body.len()
                                    );
                                    let _ = stream.write_all(header.as_bytes());
                                    let _ = stream.write_all(&res_data.body);
                                }
                            }
                        }
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(_) => {
                    break;
                }
            }
        }
    });
    open_browser("http://localhost:8080");
}

fn get_response_bytes(shared: &LiveServerShared, req_path: &str) -> ResponseData {
    let tabs = shared.tabs.lock().unwrap();
    let active_content = shared.active_content.lock().unwrap().clone();
    let active_path = shared.active_path.lock().unwrap().clone();
    let workspace_folder = shared.workspace_folder.lock().unwrap().clone();
    let clean_path = req_path.trim_start_matches('/');
    let is_index = clean_path.is_empty() || clean_path == "index.html";
    if is_index {
        if let Some(path) = active_path.as_ref() {
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            if ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm") {
                return ResponseData {
                    body: active_content.into_bytes(),
                    mime: "text/html; charset=utf-8",
                };
            }
        }
        if let Some(ref wf) = workspace_folder {
            let index_path = wf.join("index.html");
            if index_path.is_file() {
                if let Ok(bytes) = fs::read(&index_path) {
                    return ResponseData {
                        body: bytes,
                        mime: "text/html; charset=utf-8",
                    };
                }
            }
        }
        for (name, content) in tabs.iter() {
            if name.ends_with(".html") || name.ends_with(".htm") {
                return ResponseData {
                    body: content.as_bytes().to_vec(),
                    mime: "text/html; charset=utf-8",
                };
            }
        }
        return ResponseData {
            body: b"<!DOCTYPE html><html><head><title>Live Server</title></head><body><h1>Live Server Running</h1><p>No HTML file active or index.html found in open folder.</p></body></html>".to_vec(),
            mime: "text/html; charset=utf-8",
        };
    }
    if let Some(content) = tabs.get(clean_path) {
        return ResponseData {
            body: content.as_bytes().to_vec(),
            mime: get_mime(clean_path),
        };
    }
    for (name, content) in tabs.iter() {
        if Path::new(name).file_name().and_then(|s| s.to_str()) == Some(clean_path) {
            return ResponseData {
                body: content.as_bytes().to_vec(),
                mime: get_mime(clean_path),
            };
        }
    }
    if let Some(ref wf) = workspace_folder {
        let file_path = wf.join(clean_path);
        if file_path.is_file() {
            if let Ok(bytes) = fs::read(&file_path) {
                return ResponseData {
                    body: bytes,
                    mime: get_mime(clean_path),
                };
            }
        }
    }
    if let Some(parent) = active_path.as_ref().and_then(|p| p.parent()) {
        let file_path = parent.join(clean_path);
        if file_path.is_file() {
            if let Ok(bytes) = fs::read(&file_path) {
                return ResponseData {
                    body: bytes,
                    mime: get_mime(clean_path),
                };
            }
        }
    }
    ResponseData {
        body: format!("<h1>404 Not Found: {}</h1>", clean_path).into_bytes(),
        mime: "text/html; charset=utf-8",
    }
}

fn get_mime(path_str: &str) -> &'static str {
    let ext = Path::new(path_str)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" | "cjs" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        _ => "text/plain; charset=utf-8",
    }
}

fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", url])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

struct FlatronixEditor {
    tabs: Vec<FileTab>,
    active_tab: usize,
    workspace_folder: Option<PathBuf>,
    dir_cache: Vec<CachedDirNode>,
    last_dir_refresh: Instant,
    syntax_set: SyntaxSet,
    theme: Theme,
    icons: HashMap<String, egui::TextureHandle>,
    editing_tab: Option<usize>,
    editing_name: String,
    rename_focus_requested: bool,
    show_search: bool,
    search_query: String,
    discord_client: Option<DiscordIpcClient>,
    discord_start_time: i64,
    last_discord_update: Instant,
    extensions: Vec<Extension>,
    show_extensions_window: bool,
    extension_page: Option<String>,
    show_settings_window: bool,
    app_theme: AppTheme,
    app_lang: AppLanguage,
    was_focused: bool,
    live_server_shared: Arc<LiveServerShared>,
    live_server_running: Arc<AtomicBool>,
    problems: Vec<CodeProblem>,
    problems_cache_content: String,
    problems_cache_ext: String,
    show_problems_panel: bool,
    intellisense: IntellisenseState,
    update_state: Arc<Mutex<UpdateState>>,
    show_updater_window: bool,
}

impl FlatronixEditor {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);
        set_optimized_visuals(&cc.egui_ctx, AppTheme::Dark);

        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        let theme = theme_set.themes["base16-ocean.dark"].clone();
        let shared = Arc::new(LiveServerShared {
            version: AtomicU64::new(1),
            active_content: Mutex::new(String::new()),
            active_path: Mutex::new(None),
            workspace_folder: Mutex::new(None),
            tabs: Mutex::new(HashMap::new()),
        });
        let mut app = Self {
            tabs: vec![],
            active_tab: 0,
            workspace_folder: None,
            dir_cache: vec![],
            last_dir_refresh: Instant::now() - Duration::from_secs(10),
            syntax_set,
            theme,
            icons: HashMap::new(),
            editing_tab: None,
            editing_name: String::new(),
            rename_focus_requested: false,
            show_search: false,
            search_query: String::new(),
            discord_client: None,
            discord_start_time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
            last_discord_update: Instant::now() - Duration::from_secs(10),
            extensions: vec![
                Extension {
                    id: "discord_rpc".to_string(),
                    name: "Discord RPC".to_string(),
                    description: "Show your friends on Discord what are you coding!".to_string(),
                    long_description: "This extension connects CodeEditer with your Discord!\n\nYour friends will see:\n• What file you're editing\n• What language you're using\n• How many lines/characters you wrote\n\nRequires Discord desktop app to be running.".to_string(),
                    author: "Flatronix".to_string(),
                    version: "1.0.0".to_string(),
                    installed: false,
                    verified: true,
                },
                Extension {
                    id: "live_server".to_string(),
                    name: "Live Server".to_string(),
                    description: "Live preview server for HTML/CSS/JS on port 8080".to_string(),
                    long_description: "Provides a live preview server on http://localhost:8080.\n\nFeatures:\n• Auto-reloads on file save or when leaving application window\n• Opens default browser automatically\n• Supports HTML, CSS, JavaScript".to_string(),
                    author: "Flatronix".to_string(),
                    version: "1.0.0".to_string(),
                    installed: true,
                    verified: true,
                },
            ],
            show_extensions_window: false,
            extension_page: None,
            show_settings_window: false,
            app_theme: AppTheme::Dark,
            app_lang: AppLanguage::English,
            was_focused: true,
            live_server_shared: shared,
            live_server_running: Arc::new(AtomicBool::new(false)),
            problems: vec![],
            problems_cache_content: String::new(),
            problems_cache_ext: String::new(),
            show_problems_panel: false,
            intellisense: IntellisenseState::default(),
            update_state: Arc::new(Mutex::new(UpdateState::Idle)),
            show_updater_window: false,
        };
        app.load_settings();
        app.load_icons(&cc.egui_ctx);
        app.load_session();
        app.load_extensions_state();
        set_optimized_visuals(&cc.egui_ctx, app.app_theme);
        if app.is_extension_installed("discord_rpc") {
            app.init_discord();
        }
        let args: Vec<PathBuf> = env::args().skip(1).map(PathBuf::from).collect();
        for arg in args {
            if arg.is_file() {
                app.open_path(&arg);
            } else if arg.is_dir() {
                app.workspace_folder = Some(arg.clone());
                *app.live_server_shared.workspace_folder.lock().unwrap() = Some(arg);
            }
        }
        if app.tabs.is_empty() {
            app.tabs.push(FileTab::new(
                None,
                "new_file.txt".to_string(),
                "Hi! To start try creating a new file using the File menu!\n".to_string(),
            ));
        }
        app
    }

    fn check_for_updates(state: Arc<Mutex<UpdateState>>) {
        *state.lock().unwrap() = UpdateState::Checking;
        thread::spawn(move || {
            let url = "https://api.github.com/repos/FlatronTech/CodeEditer/releases/latest";
            let req = ureq::get(url)
                .set("User-Agent", "CodeEditer-Updater")
                .call();
            if let Ok(response) = req {
                if let Ok(json) = response.into_json::<serde_json::Value>() {
                    let version = json["tag_name"].as_str().unwrap_or("").to_string();
                    let current_version = env!("CARGO_PKG_VERSION");
                    if !version.is_empty() && version != format!("v{}", current_version) && version != current_version {
                        if let Some(assets) = json["assets"].as_array() {
                            for asset in assets {
                                if asset["name"].as_str().unwrap_or("") == "update.7z" {
                                    if let Some(dl_url) = asset["browser_download_url"].as_str() {
                                        *state.lock().unwrap() = UpdateState::Available(version, dl_url.to_string());
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    *state.lock().unwrap() = UpdateState::Error("Brak nowych aktualizacji.".to_string());
                } else {
                    *state.lock().unwrap() = UpdateState::Error("Błąd parsowania odpowiedzi API.".to_string());
                }
            } else {
                *state.lock().unwrap() = UpdateState::Error("Błąd połączenia z GitHub API.".to_string());
            }
        });
    }

    fn download_and_prepare_update(state: Arc<Mutex<UpdateState>>, download_url: String) {
        *state.lock().unwrap() = UpdateState::Downloading(0);
        thread::spawn(move || {
            let temp_dir = env::temp_dir();
            let archive_path = temp_dir.join("update.7z");
            let extract_path = temp_dir.join("CodeEditerUpdate");
            let response = match ureq::get(&download_url).set("User-Agent", "CodeEditer-Updater").call() {
                Ok(r) => r,
                Err(_) => {
                    *state.lock().unwrap() = UpdateState::Error("Nie udało się pobrać pliku.".to_string());
                    return;
                }
            };
            let total_size = response.header("Content-Length").and_then(|s| s.parse::<u64>().ok()).unwrap_or(1);
            let mut reader = response.into_reader();
            let mut file = match std::fs::File::create(&archive_path) {
                Ok(f) => f,
                Err(_) => {
                    *state.lock().unwrap() = UpdateState::Error("Nie można utworzyć pliku tymczasowego.".to_string());
                    return;
                }
            };
            let mut buffer = [0; 8192];
            let mut downloaded: u64 = 0;
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        let _ = file.write_all(&buffer[..n]);
                        downloaded += n as u64;
                        let progress = ((downloaded as f64 / total_size as f64) * 100.0) as u32;
                        *state.lock().unwrap() = UpdateState::Downloading(progress.min(99));
                    }
                    Err(_) => {
                        *state.lock().unwrap() = UpdateState::Error("Błąd podczas pobierania strumienia.".to_string());
                        return;
                    }
                }
            }
            drop(file);
            *state.lock().unwrap() = UpdateState::Downloading(100);
            let _ = fs::remove_dir_all(&extract_path);
            if let Err(_) = sevenz_rust::decompress_file(&archive_path, &extract_path) {
                *state.lock().unwrap() = UpdateState::Error("Błąd rozpakowywania pliku 7z (upewnij się, że używasz sevenz-rust).".to_string());
                return;
            }
            *state.lock().unwrap() = UpdateState::ReadyToInstall(extract_path);
        });
    }

    fn execute_update_script(extract_path: &Path) {
        let temp_dir = env::temp_dir();
        let script_path = temp_dir.join("update_codeediter.ps1");
        let exe_dir = get_exe_dir();
        let script_content = format!(
            r#"
param([string]$targetDir, [string]$pidToWait, [string]$extractDir, [string]$archivePath)
Wait-Process -Id $pidToWait -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2
Copy-Item -Path "$extractDir*" -Destination $targetDir -Recurse -Force
Remove-Item -Path $extractDir -Recurse -Force
Remove-Item -Path $archivePath -Force
Start-Process "$targetDir\CodeEditer.exe"
"#
        );
        if fs::write(&script_path, script_content).is_ok() {
            let current_pid = std::process::id().to_string();
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                let _ = std::process::Command::new("powershell")
                    .arg("-WindowStyle").arg("Hidden")
                    .arg("-ExecutionPolicy").arg("Bypass")
                    .arg("-File").arg(script_path)
                    .arg("-targetDir").arg(exe_dir)
                    .arg("-pidToWait").arg(current_pid)
                    .arg("-extractDir").arg(extract_path)
                    .arg("-archivePath").arg(temp_dir.join("update.7z"))
                    .creation_flags(0x08000000)
                    .spawn();
                std::process::exit(0);
            }
        }
    }

    fn save_settings(&self) {
        if let Some(proj_dir) = directories::ProjectDirs::from("com", "folder", "editor") {
            let dir = proj_dir.data_dir();
            if fs::create_dir_all(dir).is_ok() {
                let settings = EditorSettings {
                    theme: self.app_theme,
                    language: self.app_lang,
                };
                if let Ok(json) = serde_json::to_string_pretty(&settings) {
                    let _ = fs::write(dir.join("settings.json"), json);
                }
            }
        }
    }

    fn load_settings(&mut self) {
        if let Some(proj_dir) = directories::ProjectDirs::from("com", "folder", "editor") {
            let path = proj_dir.data_dir().join("settings.json");
            if let Ok(data) = fs::read_to_string(path) {
                if let Ok(settings) = serde_json::from_str::<EditorSettings>(&data) {
                    self.app_theme = settings.theme;
                    self.app_lang = settings.language;
                }
            }
        }
    }

    fn is_extension_installed(&self, id: &str) -> bool {
        self.extensions.iter().any(|e| e.id == id && e.installed)
    }

    fn save_extensions_state(&self) {
        if let Some(proj_dir) = directories::ProjectDirs::from("com", "folder", "editor") {
            let dir = proj_dir.data_dir();
            if fs::create_dir_all(dir).is_ok() {
                let state = ExtensionsState {
                    installed_ids: self
                        .extensions
                        .iter()
                        .filter(|e| e.installed)
                        .map(|e| e.id.clone())
                        .collect(),
                };
                if let Ok(json) = serde_json::to_string_pretty(&state) {
                    let _ = fs::write(dir.join("extensions.json"), json);
                }
            }
        }
    }

    fn load_extensions_state(&mut self) {
        if let Some(proj_dir) = directories::ProjectDirs::from("com", "folder", "editor") {
            let path = proj_dir.data_dir().join("extensions.json");
            if let Ok(data) = fs::read_to_string(path) {
                if let Ok(state) = serde_json::from_str::<ExtensionsState>(&data) {
                    for ext in &mut self.extensions {
                        ext.installed = state.installed_ids.contains(&ext.id);
                    }
                }
            }
        }
    }

    fn init_discord(&mut self) {
        let app_id = "1545533503519596594";
        let mut client = DiscordIpcClient::new(app_id);
        if client.connect().is_ok() {
            self.discord_client = Some(client);
        }
    }

    fn disconnect_discord(&mut self) {
        if let Some(mut client) = self.discord_client.take() {
            let _ = client.close();
        }
    }

    fn update_discord_presence(&mut self) {
        if !self.is_extension_installed("discord_rpc") {
            return;
        }
        if self.last_discord_update.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.last_discord_update = Instant::now();
        let client = match &mut self.discord_client {
            Some(c) => c,
            None => return,
        };
        let (details, state, large_image, large_text, small_image, small_text) =
            if let Some(tab) = self.tabs.get(self.active_tab) {
                let ext = Path::new(&tab.name)
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("txt")
                    .to_lowercase();
                let lang = language_name(&ext);
                let lines = tab.content.lines().count().max(1);
                let chars = tab.content.chars().count();
                let details = format!("Editing: {}", tab.name);
                let state = format!("{} | {} lines | {} characters", lang, lines, chars);
                let large_image = match ext.as_str() {
                    "py" => "python",
                    "cpp" | "cxx" | "cc" | "hpp" | "hxx" => "cpp",
                    "rs" => "rust",
                    "c" | "h" => "c",
                    "cs" | "csproj" => "csharp",
                    "js" | "mjs" | "cjs" | "ts" => "javascript",
                    "java" => "java",
                    "go" => "golang",
                    "html" | "htm" => "html",
                    "bat" | "cmd" => "batch",
                    "css" => "css",
                    "php" => "php",
                    _ => "editor",
                }
                .to_string();
                let large_text = lang.to_string();
                let (small_image, small_text) = if tab.is_modified {
                    ("unsaved".to_string(), "Unsaved changes".to_string())
                } else {
                    ("saved".to_string(), "Saved".to_string())
                };
                (details, state, large_image, large_text, small_image, small_text)
            } else {
                (
                    "No open files".to_string(),
                    "Waiting for a file...".to_string(),
                    "editor".to_string(),
                    "CodeEditer".to_string(),
                    "idle".to_string(),
                    "Idle".to_string(),
                )
            };
        let activity = activity::Activity::new()
            .details(&details)
            .state(&state)
            .assets(
                activity::Assets::new()
                    .large_image(&large_image)
                    .large_text(&large_text)
                    .small_image(&small_image)
                    .small_text(&small_text),
            )
            .timestamps(activity::Timestamps::new().start(self.discord_start_time));
        if client.set_activity(activity).is_err() {
            let _ = client.connect();
        }
    }

    fn is_active_tab_html(&self) -> bool {
        if let Some(tab) = self.tabs.get(self.active_tab) {
            let ext = Path::new(&tab.name)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            ext == "html" || ext == "htm"
        } else {
            false
        }
    }

    fn update_problems(&mut self) {
        let (content, ext) = if let Some(tab) = self.tabs.get(self.active_tab) {
            let ext = Path::new(&tab.name)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("txt")
                .to_lowercase();
            (tab.content.clone(), ext)
        } else {
            self.problems.clear();
            return;
        };
        if self.problems_cache_content == content && self.problems_cache_ext == ext {
            return;
        }
        self.problems_cache_content = content.clone();
        self.problems_cache_ext = ext.clone();
        self.problems = check_code_problems(&content, &ext);
    }

    fn load_icons(&mut self, ctx: &egui::Context) {
        let icon_map: [(&str, &str); 13] = [
            ("py", "icons/py.ico"),
            ("cplus", "icons/cplus.ico"),
            ("html", "icons/html.ico"),
            ("rs", "icons/rs.ico"),
            ("bat", "icons/bat.ico"),
            ("js", "icons/js.ico"),
            ("go", "icons/go.ico"),
            ("c", "icons/c.ico"),
            ("csharp", "icons/csharp.ico"),
            ("css", "icons/css.ico"),
            ("txt", "icons/txt.ico"),
            ("php", "icons/php.ico"),
            ("verified", "icons/verified.ico"),
        ];
        for (key, path) in icon_map.iter() {
            let resolved_path = get_asset_path(path);
            let fallback_path = get_asset_path(&path.replace("icons/", ""));
            if let Ok(img) = image::open(&resolved_path).or_else(|_| image::open(&fallback_path)) {
                let size = [img.width() as usize, img.height() as usize];
                let rgba = img.into_rgba8().into_raw();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                let texture = ctx.load_texture(*key, color_image, egui::TextureOptions::LINEAR);
                self.icons.insert((*key).to_string(), texture);
            }
        }
    }

    fn save_session(&self) {
        if let Some(proj_dir) = directories::ProjectDirs::from("com", "folder", "editor") {
            let dir = proj_dir.data_dir();
            if fs::create_dir_all(dir).is_ok() {
                let session = Session {
                    tabs: self.tabs.clone(),
                    active_tab_index: self.active_tab,
                    workspace_folder: self.workspace_folder.clone(),
                };
                if let Ok(json) = serde_json::to_string_pretty(&session) {
                    let _ = fs::write(dir.join("session.json"), json);
                }
            }
        }
    }

    fn load_session(&mut self) {
        if let Some(proj_dir) = directories::ProjectDirs::from("com", "folder", "editor") {
            let path = proj_dir.data_dir().join("session.json");
            if let Ok(data) = fs::read_to_string(path) {
                if let Ok(session) = serde_json::from_str::<Session>(&data) {
                    if !session.tabs.is_empty() {
                        self.active_tab = session
                            .active_tab_index
                            .min(session.tabs.len().saturating_sub(1));
                        self.tabs = session.tabs;
                        for tab in &mut self.tabs {
                            if tab.snapshot.is_empty() {
                                tab.snapshot = tab.content.clone();
                            }
                        }
                    }
                    self.workspace_folder = session.workspace_folder.clone();
                    *self.live_server_shared.workspace_folder.lock().unwrap() =
                        session.workspace_folder;
                }
            }
        }
    }

    fn get_icon_for_file<'a>(
        icons: &'a HashMap<String, egui::TextureHandle>,
        name: &str,
    ) -> Option<&'a egui::TextureHandle> {
        let ext = Path::new(name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        let key = match ext.as_str() {
            "py" => "py",
            "cpp" | "cxx" | "cc" | "hpp" | "hxx" => "cplus",
            "html" | "htm" => "html",
            "rs" => "rs",
            "bat" | "cmd" => "bat",
            "js" | "mjs" | "cjs" | "ts" => "js",
            "go" => "go",
            "c" | "h" => "c",
            "cs" | "csproj" => "csharp",
            "css" => "css",
            "txt" => "txt",
            "php" => "php",
            _ => return None,
        };
        icons.get(key)
    }

    fn open_path(&mut self, path: &Path) {
        if !path.is_file() {
            return;
        }
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if let Some(pos) = self.tabs.iter().position(|tab| {
            if let Some(existing) = &tab.path {
                if existing == &canonical || existing.as_path() == path {
                    return true;
                }
                if let Ok(existing_canonical) = existing.canonicalize() {
                    return existing_canonical == canonical;
                }
            }
            false
        }) {
            self.active_tab = pos;
            self.editing_tab = None;
            self.rename_focus_requested = false;
            return;
        }
        let content = match fs::read_to_string(&canonical) {
            Ok(s) => s,
            Err(_) => match fs::read(&canonical) {
                Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(_) => return,
            },
        };
        let name = canonical
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("new_file")
            .to_string();
        self.tabs.push(FileTab::new(Some(canonical), name, content));
        self.active_tab = self.tabs.len().saturating_sub(1);
        self.editing_tab = None;
        self.rename_focus_requested = false;
    }

    fn create_template(&mut self, template: NewTemplate) {
        let (base, ext) = match template {
            NewTemplate::Plain => ("new", "txt"),
            NewTemplate::Python => ("main", "py"),
            NewTemplate::Cpp => ("main", "cpp"),
            NewTemplate::Rust => ("main", "rs"),
            NewTemplate::C => ("main", "c"),
            NewTemplate::CSharp => ("Program", "cs"),
            NewTemplate::JavaScript => ("script", "js"),
            NewTemplate::Java => ("Main", "java"),
            NewTemplate::Go => ("main", "go"),
            NewTemplate::Html => ("index", "html"),
            NewTemplate::Batch => ("script", "bat"),
        };
        let name = self.unique_name(base, ext);
        let stem = Path::new(&name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Main")
            .to_string();
        let content = match template {
            NewTemplate::Plain => "\n".to_string(),
            NewTemplate::Python => "print(\"Hello World!\")\n".to_string(),
            NewTemplate::Cpp => {
                "#include <iostream>\n\nint main() {\n    std::cout << \"Hello World!\" << std::endl;\n    return 0;\n}\n".to_string()
            }
            NewTemplate::Rust => {
                "fn main() {\n    println!(\"Hello World!\");\n}\n".to_string()
            }
            NewTemplate::C => {
                "#include <stdio.h>\n\nint main() {\n    printf(\"Hello World!\\n\");\n    return 0;\n}\n".to_string()
            }
            NewTemplate::CSharp => {
                format!(
                    "using System;\n\nclass {}\n{{\n    static void Main()\n    {{\n        Console.WriteLine(\"Hello World!\");\n    }}\n}}\n",
                    stem
                )
            }
            NewTemplate::JavaScript => "console.log(\"Hello World!\");\n".to_string(),
            NewTemplate::Java => {
                format!(
                    "public class {} {{\n    public static void main(String[] args) {{\n        System.out.println(\"Hello World!\");\n    }}\n}}\n",
                    stem
                )
            }
            NewTemplate::Go => {
                "package main\n\nimport \"fmt\"\n\nfunc main() {\n    fmt.Println(\"Hello World!\")\n}\n".to_string()
            }
            NewTemplate::Html => {
                "<!DOCTYPE html>\n<html>\n<head>\n    <meta charset=\"UTF-8\">\n    <title>Flatronix</title>\n</head>\n<body>\n    <h1>Hello World!</h1>\n</body>\n</html>\n".to_string()
            }
            NewTemplate::Batch => "@echo off\r\necho Hello World!\r\npause\r\n".to_string(),
        };
        self.tabs.push(FileTab::new(None, name, content));
        self.active_tab = self.tabs.len().saturating_sub(1);
        self.editing_tab = None;
        self.rename_focus_requested = false;
        self.last_dir_refresh = Instant::now() - Duration::from_secs(10);
    }

    fn unique_name(&self, base: &str, ext: &str) -> String {
        let make = |i: usize| -> String {
            if ext.is_empty() {
                if i == 1 {
                    base.to_string()
                } else {
                    format!("{}_{}", base, i)
                }
            } else if i == 1 {
                format!("{}.{}", base, ext)
            } else {
                format!("{}_{}.{}", base, i, ext)
            }
        };
        let mut i = 1;
        let mut name = make(i);
        while self.tabs.iter().any(|t| t.name == name) {
            i += 1;
            name = make(i);
        }
        name
    }

    fn rename_tab(&mut self, index: usize, new_name: String) {
        self.editing_tab = None;
        self.rename_focus_requested = false;
        let new_name = new_name.trim().to_string();
        if new_name.is_empty() {
            return;
        }
        if let Some(tab) = self.tabs.get_mut(index) {
            match &tab.path {
                None => {
                    tab.name = new_name;
                }
                Some(old_path) => {
                    if old_path.exists() {
                        let new_path = old_path
                            .parent()
                            .map(|p| p.join(&new_name))
                            .unwrap_or_else(|| PathBuf::from(&new_name));
                        if new_path == *old_path {
                            tab.name = new_name;
                        } else if !new_path.exists() && fs::rename(old_path, &new_path).is_ok() {
                            tab.path = Some(new_path);
                            tab.name = new_name;
                        }
                    } else {
                        let new_path = old_path
                            .parent()
                            .map(|p| p.join(&new_name))
                            .unwrap_or_else(|| PathBuf::from(&new_name));
                        tab.path = Some(new_path);
                        tab.name = new_name;
                    }
                }
            }
        }
        self.last_dir_refresh = Instant::now() - Duration::from_secs(10);
    }

    fn save_active(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            let path = tab
                .path
                .clone()
                .unwrap_or_else(|| PathBuf::from(&tab.name));
            if fs::write(&path, &tab.content).is_ok() {
                tab.path = Some(path);
                tab.is_modified = false;
                tab.snapshot = tab.content.clone();
                self.live_server_shared.version.fetch_add(1, Ordering::SeqCst);
                self.last_dir_refresh = Instant::now() - Duration::from_secs(10);
            }
        }
    }

    fn undo_active(&mut self) {
        if self.editing_tab.is_some() {
            return;
        }
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            if let Some(prev) = tab.undo_stack.pop() {
                tab.redo_stack.push(tab.snapshot.clone());
                tab.content = prev.clone();
                tab.snapshot = prev;
                tab.is_modified = true;
            }
        }
    }

    fn redo_active(&mut self) {
        if self.editing_tab.is_some() {
            return;
        }
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            if let Some(next) = tab.redo_stack.pop() {
                tab.undo_stack.push(tab.snapshot.clone());
                tab.content = next.clone();
                tab.snapshot = next;
                tab.is_modified = true;
            }
        }
    }

    fn paste_replace_active(&mut self, text: &str) {
        if self.editing_tab.is_some() {
            return;
        }
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            if tab.content != text {
                tab.undo_stack.push(tab.snapshot.clone());
                if tab.undo_stack.len() > 1000 {
                    tab.undo_stack.remove(0);
                }
                tab.redo_stack.clear();
                tab.content = text.to_string();
                tab.snapshot = tab.content.clone();
                tab.is_modified = true;
            }
        }
    }

    fn register_change(tab: &mut FileTab) {
        if tab.content != tab.snapshot {
            tab.undo_stack.push(tab.snapshot.clone());
            if tab.undo_stack.len() > 1000 {
                tab.undo_stack.remove(0);
            }
            tab.redo_stack.clear();
            tab.snapshot = tab.content.clone();
            tab.is_modified = true;
        }
    }

    fn calculate_intellisense(&mut self, text: &str, ext: &str, cursor_idx: usize) {
        if cursor_idx == 0 || cursor_idx > text.len() {
            self.intellisense.active = false;
            return;
        }
        // Find current word before cursor
        let chars: Vec<char> = text.chars().collect();
        let mut bytes_acc = 0;
        let mut cursor_char_idx = chars.len();
        for (i, c) in chars.iter().enumerate() {
            if bytes_acc >= cursor_idx {
                cursor_char_idx = i;
                break;
            }
            bytes_acc += c.len_utf8();
        }
        while cursor_char_idx > 0 {
            let prev_c = chars[cursor_char_idx - 1];
            if prev_c.is_alphanumeric() || prev_c == '_' {
                cursor_char_idx -= 1;
            } else {
                break;
            }
        }
        let prefix: String = chars[cursor_char_idx..].iter().take_while(|c| c.is_alphanumeric() || **c == '_').collect();
        if prefix.len() < 2 {
            self.intellisense.active = false;
            return;
        }
        let mut words = HashSet::new();
        for kw in language_keywords(ext) {
            if kw.starts_with(&prefix) && *kw != prefix {
                words.insert(kw.to_string());
            }
        }
        let doc_words = text.split(|c: char| !c.is_alphanumeric() && c != '_');
        for w in doc_words {
            if w.len() > 2 && w.starts_with(&prefix) && w != prefix {
                words.insert(w.to_string());
            }
        }
        let mut suggestions: Vec<String> = words.into_iter().collect();
        suggestions.sort();
        if suggestions.is_empty() {
            self.intellisense.active = false;
        } else {
            self.intellisense.active = true;
            self.intellisense.prefix = prefix.clone();
            self.intellisense.suggestions = suggestions.into_iter().take(8).collect();
            self.intellisense.selected_index = 0;
            let prefix_len_bytes = prefix.len();
            self.intellisense.replace_range = (cursor_idx - prefix_len_bytes)..cursor_idx;
        }
    }
}

impl eframe::App for FlatronixEditor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        set_optimized_visuals(ctx, self.app_theme);

        let is_focused = ctx.input(|i| i.raw.focused);
        if self.was_focused && !is_focused {
            self.save_active();
        }
        self.was_focused = is_focused;

        {
            let mut tabs_map = self.live_server_shared.tabs.lock().unwrap();
            tabs_map.clear();
            for tab in &self.tabs {
                tabs_map.insert(tab.name.clone(), tab.content.clone());
            }
            if let Some(tab) = self.tabs.get(self.active_tab) {
                *self.live_server_shared.active_content.lock().unwrap() = tab.content.clone();
                *self.live_server_shared.active_path.lock().unwrap() = tab.path.clone();
            } else {
                self.live_server_shared.active_content.lock().unwrap().clear();
                *self.live_server_shared.active_path.lock().unwrap() = None;
            }
        }

        self.update_discord_presence();
        self.update_problems();

        let mut dropped_paths: Vec<PathBuf> = Vec::new();
        ctx.input(|i| {
            for file in &i.raw.dropped_files {
                if let Some(path) = &file.path {
                    dropped_paths.push(path.clone());
                }
            }
        });
        for path in dropped_paths {
            if path.is_dir() {
                self.workspace_folder = Some(path.clone());
                *self.live_server_shared.workspace_folder.lock().unwrap() = Some(path);
                self.last_dir_refresh = Instant::now() - Duration::from_secs(10);
            } else {
                self.open_path(&path);
            }
        }

        let mut create_template_cmd: Option<NewTemplate> = None;
        let mut save_current = false;
        let mut close_current = false;
        let mut open_file_dialog = false;
        let mut open_folder_dialog = false;
        let mut new_shortcut = false;
        let mut undo_shortcut = false;
        let mut redo_shortcut = false;
        let mut copy_all_shortcut = false;
        let mut toggle_search = false;
        let mut paste_all_text: Option<String> = None;
        let editing_name_mode = self.editing_tab.is_some();
        let ui_has_focus = ctx.memory(|mem| mem.focused().is_some());
        let mut focus_search_requested = false;

        ctx.input_mut(|i| {
            if i.modifiers.ctrl {
                if i.key_pressed(egui::Key::S) {
                    save_current = true;
                    i.consume_key(egui::Modifiers::CTRL, egui::Key::S);
                }
                if i.key_pressed(egui::Key::N) {
                    new_shortcut = true;
                    i.consume_key(egui::Modifiers::CTRL, egui::Key::N);
                }
                if i.key_pressed(egui::Key::F) {
                    toggle_search = true;
                    focus_search_requested = true;
                    i.consume_key(egui::Modifiers::CTRL, egui::Key::F);
                }
                if i.key_pressed(egui::Key::Z) {
                    if i.modifiers.shift {
                        redo_shortcut = true;
                        i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::Z);
                    } else {
                        undo_shortcut = true;
                        i.consume_key(egui::Modifiers::CTRL, egui::Key::Z);
                    }
                }
                if i.key_pressed(egui::Key::Y) {
                    redo_shortcut = true;
                    i.consume_key(egui::Modifiers::CTRL, egui::Key::Y);
                }
                if !ui_has_focus && !editing_name_mode && i.key_pressed(egui::Key::C) {
                    copy_all_shortcut = true;
                    i.consume_key(egui::Modifiers::CTRL, egui::Key::C);
                }
            }
            if self.intellisense.active {
                if i.key_pressed(egui::Key::ArrowDown) {
                    self.intellisense.selected_index = (self.intellisense.selected_index + 1) % self.intellisense.suggestions.len();
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);
                }
                if i.key_pressed(egui::Key::ArrowUp) {
                    if self.intellisense.selected_index == 0 {
                        self.intellisense.selected_index = self.intellisense.suggestions.len() - 1;
                    } else {
                        self.intellisense.selected_index -= 1;
                    }
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);
                }
                if i.key_pressed(egui::Key::Escape) {
                    self.intellisense.active = false;
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
                }
            }
            if !ui_has_focus && !editing_name_mode {
                for ev in &i.raw.events {
                    if let egui::Event::Paste(text) = ev {
                        paste_all_text = Some(text.clone());
                    }
                }
            }
        });

        if toggle_search {
            self.show_search = true;
        }
        if new_shortcut {
            create_template_cmd = Some(NewTemplate::Plain);
        }
        if undo_shortcut && !editing_name_mode {
            self.undo_active();
        }
        if redo_shortcut && !editing_name_mode {
            self.redo_active();
        }
        if copy_all_shortcut {
            if let Some(tab) = self.tabs.get(self.active_tab) {
                ctx.copy_text(tab.content.clone());
            }
        }
        if let Some(text) = paste_all_text {
            if !editing_name_mode {
                self.paste_replace_active(&text);
            }
        }

        let is_pl = self.app_lang == AppLanguage::Polish;

        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button(if is_pl { "Plik" } else { "File" }, |ui| {
                    if ui.button(if is_pl { "Nowy Plik" } else { "New File" }).clicked() {
                        create_template_cmd = Some(NewTemplate::Plain);
                        ui.close_menu();
                    }
                    if ui.button(if is_pl { "Otwórz Plik" } else { "Open File" }).clicked() {
                        open_file_dialog = true;
                        ui.close_menu();
                    }
                    if ui.button(if is_pl { "Otwórz Folder" } else { "Open Folder" }).clicked() {
                        open_folder_dialog = true;
                        ui.close_menu();
                    }
                    ui.menu_button(if is_pl { "Szablony" } else { "Templates" }, |ui| {
                        if ui.button("TXT").clicked() {
                            create_template_cmd = Some(NewTemplate::Plain);
                            ui.close_menu();
                        }
                        if ui.button("Python").clicked() {
                            create_template_cmd = Some(NewTemplate::Python);
                            ui.close_menu();
                        }
                        if ui.button("C++").clicked() {
                            create_template_cmd = Some(NewTemplate::Cpp);
                            ui.close_menu();
                        }
                        if ui.button("Rust").clicked() {
                            create_template_cmd = Some(NewTemplate::Rust);
                            ui.close_menu();
                        }
                        if ui.button("C").clicked() {
                            create_template_cmd = Some(NewTemplate::C);
                            ui.close_menu();
                        }
                        if ui.button("C#").clicked() {
                            create_template_cmd = Some(NewTemplate::CSharp);
                            ui.close_menu();
                        }
                        if ui.button("JavaScript").clicked() {
                            create_template_cmd = Some(NewTemplate::JavaScript);
                            ui.close_menu();
                        }
                        if ui.button("Java").clicked() {
                            create_template_cmd = Some(NewTemplate::Java);
                            ui.close_menu();
                        }
                        if ui.button("Go").clicked() {
                            create_template_cmd = Some(NewTemplate::Go);
                            ui.close_menu();
                        }
                        if ui.button("HTML").clicked() {
                            create_template_cmd = Some(NewTemplate::Html);
                            ui.close_menu();
                        }
                        if ui.button("Batch").clicked() {
                            create_template_cmd = Some(NewTemplate::Batch);
                            ui.close_menu();
                        }
                    });
                    ui.separator();
                    if ui.button(if is_pl { "Zapisz" } else { "Save" }).clicked() {
                        save_current = true;
                        ui.close_menu();
                    }
                    if ui.button(if is_pl { "Zamknij kartę" } else { "Close Active Tab" }).clicked() {
                        close_current = true;
                        ui.close_menu();
                    }
                });
                ui.menu_button(if is_pl { "Edycja" } else { "Edit" }, |ui| {
                    if ui.button(if is_pl { "Szukaj (Ctrl+F)" } else { "Search (Ctrl+F)" }).clicked() {
                        self.show_search = true;
                        focus_search_requested = true;
                        ui.close_menu();
                    }
                });
                if ui.button(if is_pl { "Rozszerzenia" } else { "Extensions" }).clicked() {
                    self.show_extensions_window = !self.show_extensions_window;
                }
                if ui.button(if is_pl { "Ustawienia" } else { "Settings" }).clicked() {
                    self.show_settings_window = !self.show_settings_window;
                }
                if ui.button(if is_pl { "Aktualizuj" } else { "Update" }).clicked() {
                    self.show_updater_window = true;
                    Self::check_for_updates(self.update_state.clone());
                }
                if self.is_extension_installed("live_server")
                    && self.workspace_folder.is_some()
                    && self.is_active_tab_html()
                {
                    let ls_label = if self.live_server_running.load(Ordering::SeqCst) {
                        if is_pl { "⏹ Zatrzymaj Live Server" } else { "⏹ Stop Live Server" }
                    } else if is_pl {
                        "▶ Uruchom Live Server"
                    } else {
                        "▶ Start Live Server"
                    };
                    if ui.button(ls_label).clicked() {
                        if self.live_server_running.load(Ordering::SeqCst) {
                            self.live_server_running.store(false, Ordering::SeqCst);
                        } else {
                            start_live_server(
                                self.live_server_shared.clone(),
                                self.live_server_running.clone(),
                            );
                        }
                    }
                }
                ui.separator();
                ui.label("");
            });
        });

        if open_file_dialog {
            if let Some(path) = rfd::FileDialog::new()
                .set_title(if is_pl { "Otwórz Plik" } else { "Open File" })
                .pick_file()
            {
                self.open_path(&path);
            }
        }
        if open_folder_dialog {
            if let Some(path) = rfd::FileDialog::new()
                .set_title(if is_pl { "Otwórz Folder" } else { "Open Folder" })
                .pick_folder()
            {
                self.workspace_folder = Some(path.clone());
                *self.live_server_shared.workspace_folder.lock().unwrap() = Some(path);
                self.last_dir_refresh = Instant::now() - Duration::from_secs(10);
            }
        }

        if self.show_updater_window {
            let mut close_requested = false;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("updater"),
                egui::ViewportBuilder::default()
                    .with_title(if is_pl { "Aktualizacja CodeEditer" } else { "CodeEditer Update" })
                    .with_inner_size([400.0, 200.0])
                    .with_resizable(false),
                |ctx, _class| {
                    if ctx.input(|i| i.viewport().close_requested()) {
                        close_requested = true;
                    }
                    set_optimized_visuals(ctx, self.app_theme);
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.heading(if is_pl { "Autoupdater" } else { "Auto Updater" });
                        ui.separator();
                        ui.add_space(10.0);
                        let state_clone = { self.update_state.lock().unwrap().clone() };
                        match state_clone {
                            UpdateState::Idle | UpdateState::Checking => {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(if is_pl { "Sprawdzanie dostępności aktualizacji..." } else { "Checking for updates..." });
                                });
                            }
                            UpdateState::Available(ver, url) => {
                                ui.label(format!("{} {}", if is_pl { "Dostępna nowa wersja:" } else { "New version available:" }, ver));
                                ui.add_space(10.0);
                                if ui.button(if is_pl { "Pobierz i zaktualizuj" } else { "Download and update" }).clicked() {
                                    Self::download_and_prepare_update(self.update_state.clone(), url);
                                }
                            }
                            UpdateState::Downloading(prog) => {
                                ui.label(if is_pl { "Pobieranie i rozpakowywanie aktualizacji..." } else { "Downloading and extracting update..." });
                                ui.add(egui::ProgressBar::new(prog as f32 / 100.0).show_percentage());
                            }
                            UpdateState::ReadyToInstall(path) => {
                                ui.label(if is_pl { "Gotowe do instalacji!" } else { "Ready to install!" });
                                ui.label(if is_pl { "Program zostanie zamknięty, zaktualizowany w tle i uruchomiony ponownie." } else { "App will close, update in background, and restart." });
                                ui.add_space(10.0);
                                if ui.button(if is_pl { "Instaluj teraz" } else { "Install now" }).clicked() {
                                    Self::execute_update_script(&path);
                                }
                            }
                            UpdateState::Error(err) => {
                                ui.colored_label(egui::Color32::from_rgb(255, 100, 100), format!("Błąd: {}", err));
                                ui.add_space(10.0);
                                if ui.button(if is_pl { "Spróbuj ponownie" } else { "Try again" }).clicked() {
                                    Self::check_for_updates(self.update_state.clone());
                                }
                            }
                        }
                    });
                }
            );
            if close_requested {
                self.show_updater_window = false;
            }
        }

        if self.show_extensions_window {
            let mut extension_action: Option<(String, bool)> = None;
            let mut open_page: Option<String> = None;
            let mut go_back = false;
            let mut close_requested = false;
            let extensions_clone = self.extensions.clone();
            let current_page = self.extension_page.clone();
            let verified_icon = self.icons.get("verified").cloned();
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("extensions"),
                egui::ViewportBuilder::default()
                    .with_title(if is_pl { "Rozszerzenia" } else { "Extensions" })
                    .with_inner_size([480.0, 560.0])
                    .with_min_inner_size([380.0, 420.0]),
                |ctx, _class| {
                    if ctx.input(|i| i.viewport().close_requested()) {
                        close_requested = true;
                    }
                    set_optimized_visuals(ctx, self.app_theme);
                    ctx.style_mut(|style| {
                        style.spacing.item_spacing = egui::vec2(6.0, 6.0);
                        style.spacing.button_padding = egui::vec2(8.0, 4.0);
                    });
                    egui::CentralPanel::default().show(ctx, |ui| {
                        match &current_page {
                            Some(page_id) => {
                                let ext_data = extensions_clone
                                    .iter()
                                    .find(|e| &e.id == page_id)
                                    .cloned();
                                if let Some(ext) = ext_data {
                                    if ui.button(if is_pl { "<- Powrót" } else { "<- Back to list" }).clicked() {
                                        go_back = true;
                                    }
                                    ui.separator();
                                    ui.heading(&ext.name);
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        ui.label(if is_pl { "Wersja:" } else { "Version:" });
                                        ui.label(egui::RichText::new(&ext.version).strong());
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(if is_pl { "Wydawca:" } else { "Publisher:" });
                                        ui.label(egui::RichText::new(&ext.author).strong());
                                        if ext.verified {
                                            if let Some(tex) = &verified_icon {
                                                let response = ui.add(
                                                    egui::Image::new(
                                                        egui::load::SizedTexture::new(
                                                            tex.id(),
                                                            egui::vec2(16.0, 16.0),
                                                        ),
                                                    ),
                                                );
                                                response.on_hover_text("Verified publisher");
                                            } else {
                                                let response = ui.label(
                                                    egui::RichText::new("✔")
                                                        .color(egui::Color32::from_rgb(80, 200, 255)),
                                                );
                                                response.on_hover_text("Verified publisher");
                                            }
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Status:");
                                        if ext.installed {
                                            ui.label(
                                                egui::RichText::new(if is_pl { "Zainstalowano" } else { "Installed" })
                                                    .color(egui::Color32::from_rgb(100, 220, 100)),
                                            );
                                        } else {
                                            ui.label(
                                                egui::RichText::new(if is_pl { "Niezainstalowano" } else { "Not installed" })
                                                    .color(egui::Color32::from_rgb(220, 100, 100)),
                                            );
                                        }
                                    });
                                    ui.separator();
                                    ui.add_space(4.0);
                                    ui.label(&ext.long_description);
                                    ui.add_space(8.0);
                                    ui.separator();
                                    if ext.installed {
                                        if ui.button(if is_pl { "Odinstaluj" } else { "Uninstall" }).clicked() {
                                            extension_action = Some((ext.id.clone(), false));
                                        }
                                    } else if ui.button(if is_pl { "Zainstaluj" } else { "Install" }).clicked() {
                                        extension_action = Some((ext.id.clone(), true));
                                    }
                                } else if ui.button(if is_pl { "<- Powrót" } else { "<- Back to list" }).clicked() {
                                    go_back = true;
                                }
                            }
                            None => {
                                ui.label(if is_pl { "Rozszerzenia" } else { "Extensions" });
                                ui.separator();
                                for ext in extensions_clone.iter() {
                                    ui.group(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new(&ext.name)
                                                    .strong()
                                                    .size(16.0),
                                            );
                                            if ext.installed {
                                                ui.label("✅");
                                            }
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui.button(if is_pl { "Szczegóły ->" } else { "Details ->" }).clicked() {
                                                        open_page = Some(ext.id.clone());
                                                    }
                                                },
                                            );
                                        });
                                        ui.label(egui::RichText::new(&ext.description).weak());
                                    });
                                    ui.add_space(4.0);
                                }
                            }
                        }
                    });
                },
            );
            if go_back {
                self.extension_page = None;
            }
            if let Some(id) = open_page {
                self.extension_page = Some(id);
            }
            let mut state_changed = false;
            let mut discord_action: Option<bool> = None;
            if let Some((id, install)) = extension_action {
                if let Some(ext) = self.extensions.iter_mut().find(|e| e.id == id) {
                    ext.installed = install;
                    state_changed = true;
                    if id == "discord_rpc" {
                        discord_action = Some(install);
                    }
                }
            }
            if close_requested {
                self.show_extensions_window = false;
            }
            if let Some(install) = discord_action {
                if install {
                    self.init_discord();
                } else {
                    self.disconnect_discord();
                }
            }
            if state_changed {
                self.save_extensions_state();
            }
        }

        if self.show_settings_window {
            let mut close_requested = false;
            let mut current_theme = self.app_theme;
            let mut current_lang = self.app_lang;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("settings"),
                egui::ViewportBuilder::default()
                    .with_title(if is_pl { "Ustawienia" } else { "Settings" })
                    .with_inner_size([380.0, 240.0])
                    .with_resizable(false),
                |ctx, _class| {
                    if ctx.input(|i| i.viewport().close_requested()) {
                        close_requested = true;
                    }
                    set_optimized_visuals(ctx, current_theme);
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.heading(if current_lang == AppLanguage::Polish { "Ustawienia" } else { "Settings" });
                        ui.separator();
                        ui.add_space(8.0);
                        ui.label(if current_lang == AppLanguage::Polish { "Motyw:" } else { "Theme:" });
                        ui.horizontal(|ui| {
                            ui.radio_value(
                                &mut current_theme,
                                AppTheme::Dark,
                                if current_lang == AppLanguage::Polish { "Ciemny (domyślny)" } else { "Dark (default)" },
                            );
                            ui.radio_value(
                                &mut current_theme,
                                AppTheme::Light,
                                if current_lang == AppLanguage::Polish { "Jasny" } else { "Light" },
                            );
                        });
                        ui.add_space(12.0);
                        ui.label(if current_lang == AppLanguage::Polish { "Język:" } else { "Language:" });
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut current_lang, AppLanguage::English, "English");
                            ui.radio_value(&mut current_lang, AppLanguage::Polish, "Polski");
                        });
                    });
                },
            );
            if close_requested {
                self.show_settings_window = false;
            }
            if current_theme != self.app_theme || current_lang != self.app_lang {
                self.app_theme = current_theme;
                self.app_lang = current_lang;
                self.save_settings();
            }
        }

        if let Some(ref wf) = self.workspace_folder {
            let mut file_to_open: Option<PathBuf> = None;
            let mut close_folder = false;
            if self.last_dir_refresh.elapsed() > Duration::from_secs(3) {
                self.dir_cache = build_dir_tree(wf);
                self.last_dir_refresh = Instant::now();
            }
            egui::SidePanel::left("file_explorer_panel")
                .default_width(220.0)
                .min_width(160.0)
                .max_width(350.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let folder_name = wf
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("Folder");
                        ui.heading(format!("📂 {}", folder_name));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button("❌")
                                .on_hover_text(if is_pl { "Zamknij folder" } else { "Close folder" })
                                .clicked()
                            {
                                close_folder = true;
                            }
                        });
                    });
                    ui.separator();
                    egui::ScrollArea::both()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            render_cached_dir_tree(ui, &self.dir_cache, &self.icons, &mut file_to_open);
                        });
                });
            if close_folder {
                self.workspace_folder = None;
                *self.live_server_shared.workspace_folder.lock().unwrap() = None;
                self.dir_cache.clear();
            }
            if let Some(path) = file_to_open {
                self.open_path(&path);
            }
        }

        if self.show_search {
            egui::TopBottomPanel::top("search_panel").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(if is_pl { "Szukaj:" } else { "Search:" });
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.search_query)
                            .desired_width(200.0)
                    );
                    if focus_search_requested {
                        response.request_focus();
                    }
                    if ui.button(if is_pl { "Wyczyść" } else { "Clear" }).clicked() {
                        self.search_query.clear();
                        response.request_focus();
                    }
                    if ui.button(if is_pl { "Zamknij" } else { "Close" }).clicked() {
                        self.show_search = false;
                        self.search_query.clear();
                    }
                });
            });
        }

        let mut selected_tab: Option<usize> = None;
        let mut tab_to_close: Option<usize> = None;
        let mut start_rename: Option<usize> = None;
        let mut commit_rename: Option<(usize, String)> = None;
        let mut cancel_rename = false;
        let current_active = self.active_tab;
        let editing_tab = self.editing_tab;

        egui::TopBottomPanel::top("tabs_panel").show(ctx, |ui| {
            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (i, tab) in self.tabs.iter().enumerate() {
                        let is_active = i == current_active;
                        if editing_tab == Some(i) {
                            if let Some(tex) = Self::get_icon_for_file(&self.icons, &self.editing_name) {
                                ui.add(egui::Image::new(egui::load::SizedTexture::new(
                                    tex.id(),
                                    egui::vec2(16.0, 16.0),
                                )));
                            } else {
                                ui.label("");
                            }
                            let response = ui.add(
                                egui::TextEdit::singleline(&mut self.editing_name)
                                    .desired_width(140.0),
                            );
                            if self.rename_focus_requested {
                                response.request_focus();
                                self.rename_focus_requested = false;
                            }
                            if response.lost_focus() {
                                commit_rename = Some((i, self.editing_name.clone()));
                            }
                            if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                                commit_rename = Some((i, self.editing_name.clone()));
                            }
                            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                                cancel_rename = true;
                            }
                            if ui.small_button("X").clicked() {
                                tab_to_close = Some(i);
                            }
                            ui.separator();
                            continue;
                        }
                        if let Some(tex) = Self::get_icon_for_file(&self.icons, &tab.name) {
                            ui.add(egui::Image::new(egui::load::SizedTexture::new(
                                tex.id(),
                                egui::vec2(16.0, 16.0),
                            )));
                        } else {
                            ui.label("");
                        }
                        let label_base = if let Some(path) = &tab.path {
                            let folder = path
                                .parent()
                                .and_then(|p| p.file_name())
                                .and_then(|s| s.to_str());
                            match folder {
                                Some(folder) if !folder.is_empty() => {
                                    format!("{}/{}", folder, tab.name)
                                }
                                _ => tab.name.clone(),
                            }
                        } else {
                            tab.name.clone()
                        };
                        let label = if tab.is_modified {
                            format!("{} *", label_base)
                        } else {
                            label_base
                        };
                        let hover = if let Some(p) = &tab.path {
                            format!("{}", p.display())
                        } else {
                            format!("{}", tab.name)
                        };
                        let response = ui
                            .selectable_label(is_active, label)
                            .on_hover_text(hover);
                        if response.clicked() {
                            selected_tab = Some(i);
                        }
                        if response.double_clicked() {
                            start_rename = Some(i);
                        }
                        if ui.small_button("X").clicked() {
                            tab_to_close = Some(i);
                        }
                        ui.separator();
                    }
                });
            });
        });

        if self.show_problems_panel {
            egui::TopBottomPanel::bottom("problems_panel")
                .min_height(120.0)
                .max_height(250.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(if is_pl { "Problemy" } else { "Problems" }).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("X").clicked() {
                                self.show_problems_panel = false;
                            }
                        });
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            if self.problems.is_empty() {
                                ui.label(
                                    egui::RichText::new(if is_pl { "Nie wykryto problemów" } else { "No problems detected" })
                                        .color(egui::Color32::from_rgb(100, 220, 100)),
                                );
                            } else {
                                for problem in &self.problems {
                                    let (icon, color) = match problem.severity {
                                        ProblemSeverity::Error => {
                                            ("[E]", egui::Color32::from_rgb(255, 100, 100))
                                        }
                                        ProblemSeverity::Warning => {
                                            ("[W]", egui::Color32::from_rgb(255, 200, 80))
                                        }
                                        ProblemSeverity::Info => {
                                            ("[I]", egui::Color32::from_rgb(100, 180, 255))
                                        }
                                    };
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{} {} {}: {}",
                                            icon,
                                            if is_pl { "Linia" } else { "Line" },
                                            problem.line,
                                            problem.message
                                        ))
                                        .color(color),
                                    );
                                }
                            }
                        });
                });
        }

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(tab) = self.tabs.get(self.active_tab) {
                    let ext = Path::new(&tab.name)
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("txt")
                        .to_lowercase();
                    let lang = language_name(&ext);
                    let lines = tab.content.lines().count().max(1);
                    let chars = tab.content.chars().count();
                    ui.label(format!("{}", tab.name));
                    ui.separator();
                    ui.label(format!("{}", lang));
                    ui.separator();
                    ui.label(format!("{}: {}", if is_pl { "Linii" } else { "Lines" }, lines));
                    ui.separator();
                    ui.label(format!("{}: {}", if is_pl { "Znaków" } else { "Characters" }, chars));
                    ui.separator();
                    if tab.is_modified {
                        ui.label(if is_pl { "Zmodyfikowano" } else { "Modified" });
                    } else {
                        ui.label(if is_pl { "Zapisano" } else { "Saved" });
                    }
                } else {
                    ui.label(if is_pl { "Brak otwartych plików" } else { "No opened files" });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let problems_count = self.problems.len();
                    let problems_label = if problems_count == 0 {
                        if is_pl { "Brak problemów".to_string() } else { "No problems".to_string() }
                    } else {
                        format!("{} {}", problems_count, if is_pl { "problem(ów)" } else { "problem(s)" })
                    };
                    if ui.button(problems_label).clicked() {
                        self.show_problems_panel = !self.show_problems_panel;
                    }
                    ui.separator();
                    ui.label("Ctrl+S | Ctrl+N | Ctrl+Z | Ctrl+Y | Ctrl+F");
                });
            });
        });

        if close_current && self.active_tab < self.tabs.len() {
            tab_to_close = Some(self.active_tab);
        }
        if let Some(i) = selected_tab {
            if i < self.tabs.len() {
                self.active_tab = i;
            }
        }
        if let Some(i) = start_rename {
            if let Some(tab) = self.tabs.get(i) {
                self.editing_name = tab.name.clone();
                self.editing_tab = Some(i);
                self.rename_focus_requested = true;
            }
        }
        if cancel_rename {
            self.editing_tab = None;
            self.rename_focus_requested = false;
        }
        if let Some((i, name)) = commit_rename {
            if !cancel_rename {
                self.rename_tab(i, name);
            }
        }
        if let Some(index) = tab_to_close {
            if index < self.tabs.len() {
                self.tabs.remove(index);
                if self.tabs.is_empty() {
                    self.active_tab = 0;
                } else if self.active_tab >= self.tabs.len() {
                    self.active_tab = self.tabs.len() - 1;
                } else if self.active_tab > index {
                    self.active_tab -= 1;
                }
                if let Some(e) = self.editing_tab {
                    if e == index {
                        self.editing_tab = None;
                        self.rename_focus_requested = false;
                    } else if e > index {
                        self.editing_tab = Some(e - 1);
                    }
                    if let Some(e2) = self.editing_tab {
                        if e2 >= self.tabs.len() {
                            self.editing_tab = None;
                            self.rename_focus_requested = false;
                        }
                    }
                }
            }
        }
        if save_current {
            self.save_active();
        }
        if let Some(template) = create_template_cmd {
            self.create_template(template);
        }

        {
            let active_tab = self.active_tab;
            let syntax_set = &self.syntax_set;
            let theme = &self.theme;
            let search_query = &self.search_query;
            let mut insert_intellisense = None;

            egui::CentralPanel::default().show(ctx, |ui| {
                if let Some(tab) = self.tabs.get_mut(active_tab) {
                    let ext = Path::new(&tab.name)
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("txt")
                        .to_lowercase();
                    let needs_regen = match &tab.cached_layout {
                        Some((cached_content, cached_query, _)) => *cached_content != tab.content || *cached_query != *search_query,
                        None => true,
                    };
                    if needs_regen {
                        let mut layout = highlight_code(syntax_set, theme, &tab.content, &ext, search_query);
                        layout.wrap.max_width = f32::INFINITY;
                        tab.cached_layout = Some((tab.content.clone(), search_query.clone(), layout));
                    }
                    let layout_clone = tab.cached_layout.as_ref().unwrap().2.clone();
                    let line_count = tab.content.lines().count().max(1);
                    if tab.cached_line_count != line_count {
                        tab.cached_line_numbers = (1..=line_count)
                            .map(|n| format!("{:>4}", n))
                            .collect::<Vec<_>>()
                            .join("\n");
                        tab.cached_line_count = line_count;
                    }
                    let mut line_numbers = tab.cached_line_numbers.clone();

                    egui::ScrollArea::both()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add_enabled(
                                    false,
                                    egui::TextEdit::multiline(&mut line_numbers)
                                        .font(egui::FontId::new(
                                            14.0,
                                            egui::FontFamily::Monospace,
                                        ))
                                        .desired_width(50.0)
                                        .text_color(egui::Color32::from_rgb(120, 120, 120))
                                        .frame(false),
                                );
                                ui.separator();
                                let mut layouter =
                                    move |ui: &egui::Ui, _string: &str, _wrap_width: f32| {
                                        let job = layout_clone.clone();
                                        ui.fonts(|f| f.layout_job(job))
                                    };
                                let text_edit_output = egui::TextEdit::multiline(&mut tab.content)
                                    .font(egui::FontId::new(14.0, egui::FontFamily::Monospace))
                                    .desired_width(f32::INFINITY)
                                    .frame(false)
                                    .layouter(&mut layouter)
                                    .show(ui);
                                let response = text_edit_output.response;
                                if response.changed() {
                                    Self::register_change(tab);
                                }
                                
                                // Intellisense trigger
                                if response.has_focus() {
                                    if let Some(cursor_range) = text_edit_output.cursor_range {
                                        let c_idx = cursor_range.primary.ccursor.index;
                                        self.calculate_intellisense(&tab.content, &ext, c_idx);
                                        // ✅ POPRAWKA 1: Usunięto "if let Some", ponieważ pos_from_cursor zwraca bezpośrednio Rect
                                        let rect = text_edit_output.galley.pos_from_cursor(&cursor_range.primary);
                                        self.intellisense.pos = rect.min + response.rect.min.to_vec2() + egui::vec2(0.0, 18.0);
                                    }
                                } else {
                                    self.intellisense.active = false;
                                }

                                // Handle autocomplete accept via Enter or Tab if active
                                if self.intellisense.active && response.has_focus() {
                                    ui.input(|i| {
                                        if i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Tab) {
                                            if let Some(sug) = self.intellisense.suggestions.get(self.intellisense.selected_index) {
                                                insert_intellisense = Some((self.intellisense.replace_range.clone(), sug.clone()));
                                            }
                                        }
                                    });
                                }
                            });
                        });

                    // Apply Intellisense replace
                    if let Some((range, sug)) = insert_intellisense {
                        tab.content.replace_range(range, &sug);
                        Self::register_change(tab);
                        self.intellisense.active = false;
                    }
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label(if is_pl { "Brak otwartych plików." } else { "No open files. Click File tab" });
                    });
                }
            });

            // Draw Intellisense Popup Window
            if self.intellisense.active && !self.intellisense.suggestions.is_empty() {
                // ✅ POPRAWKA 2: Opakowano string w egui::Id::new()
                egui::Area::new(egui::Id::new("intellisense_popup"))
                    .fixed_pos(self.intellisense.pos)
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            for (idx, sug) in self.intellisense.suggestions.iter().enumerate() {
                                let is_selected = idx == self.intellisense.selected_index;
                                let response = ui.selectable_label(is_selected, sug);
                                if response.clicked() {
                                    // Normally handle mouse clicks, skipped for simple keyboard-driven
                                }
                            }
                        });
                    });
            }
        }
        self.save_session();
    }
}

fn set_optimized_visuals(ctx: &egui::Context, theme: AppTheme) {
    let mut visuals = match theme {
        AppTheme::Dark => egui::Visuals::dark(),
        AppTheme::Light => egui::Visuals::light(),
    };
    visuals.window_rounding = 8.0.into();
    visuals.widgets.noninteractive.rounding = 4.0.into();
    visuals.widgets.inactive.rounding = 4.0.into();
    visuals.widgets.hovered.rounding = 4.0.into();
    visuals.widgets.active.rounding = 4.0.into();
    visuals.menu_rounding = 4.0.into();
    ctx.set_visuals(visuals);
}

fn build_dir_tree(dir: &Path) -> Vec<CachedDirNode> {
    let mut res = vec![];
    if let Ok(entries) = fs::read_dir(dir) {
        let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| {
            let path = e.path();
            (!path.is_dir(), e.file_name())
        });
        for entry in entries {
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.starts_with('.') || file_name == "target" || file_name == "node_modules" {
                continue;
            }
            let is_dir = path.is_dir();
            let children = if is_dir {
                build_dir_tree(&path)
            } else {
                vec![]
            };
            res.push(CachedDirNode {
                path,
                name: file_name,
                is_dir,
                children,
            });
        }
    }
    res
}

fn render_cached_dir_tree(
    ui: &mut egui::Ui,
    nodes: &[CachedDirNode],
    icons: &HashMap<String, egui::TextureHandle>,
    open_file: &mut Option<PathBuf>,
) {
    for node in nodes {
        if node.is_dir {
            egui::CollapsingHeader::new(format!("📁 {}", node.name))
                .id_source(&node.path)
                .show(ui, |ui| {
                    render_cached_dir_tree(ui, &node.children, icons, open_file);
                });
        } else {
            ui.horizontal(|ui| {
                if let Some(tex) = FlatronixEditor::get_icon_for_file(icons, &node.name) {
                    ui.add(egui::Image::new(egui::load::SizedTexture::new(
                        tex.id(),
                        egui::vec2(14.0, 14.0),
                    )));
                } else {
                    ui.label("📄 ");
                }
                if ui.selectable_label(false, &node.name).clicked() {
                    *open_file = Some(node.path.clone());
                }
            });
        }
    }
}

fn check_code_problems(content: &str, ext: &str) -> Vec<CodeProblem> {
    let mut problems = vec![];
    let info = comment_info(ext);
    let mut stack: Vec<(char, usize)> = vec![];
    let mut in_string_double = false;
    let mut in_string_single = false;
    let mut in_block_comment = false;

    for (line_idx, line) in content.lines().enumerate() {
        let line_num = line_idx + 1;
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let ch = chars[i];
            if in_block_comment {
                if let Some((_, end)) = info.block {
                    let end_chars: Vec<char> = end.chars().collect();
                    if i + end_chars.len() <= chars.len() {
                        let slice: String = chars[i..i + end_chars.len()].iter().collect();
                        if slice == end {
                            in_block_comment = false;
                            i += end_chars.len();
                            continue;
                        }
                    }
                }
                i += 1;
                continue;
            }
            if ch == '"' && !in_string_single && (i == 0 || chars[i - 1] != '\\') {
                in_string_double = !in_string_double;
                i += 1;
                continue;
            }
            if ch == '\''
                && !in_string_double
                && info.track_single
                && (i == 0 || chars[i - 1] != '\\')
            {
                in_string_single = !in_string_single;
                i += 1;
                continue;
            }
            if !in_string_double && !in_string_single {
                let mut found_line_comment = false;
                for marker in info.line {
                    let marker_chars: Vec<char> = marker.chars().collect();
                    if i + marker_chars.len() <= chars.len() {
                        let slice: String = chars[i..i + marker_chars.len()].iter().collect();
                        if slice == *marker {
                            found_line_comment = true;
                            break;
                        }
                    }
                }
                if found_line_comment {
                    break;
                }
                if let Some((start, _)) = info.block {
                    let start_chars: Vec<char> = start.chars().collect();
                    if i + start_chars.len() <= chars.len() {
                        let slice: String = chars[i..i + start_chars.len()].iter().collect();
                        if slice == start {
                            in_block_comment = true;
                            i += start_chars.len();
                            continue;
                        }
                    }
                }
                match ch {
                    '(' | '[' | '{' => {
                        stack.push((ch, line_num));
                    }
                    ')' | ']' | '}' => {
                        let expected_open = match ch {
                            ')' => '(',
                            ']' => '[',
                            '}' => '{',
                            _ => unreachable!(),
                        };
                        if let Some((open_ch, _)) = stack.pop() {
                            if open_ch != expected_open {
                                problems.push(CodeProblem {
                                    line: line_num,
                                    message: format!(
                                        "Mismatched bracket: expected closing for '{}' but found '{}'",
                                        open_ch, ch
                                    ),
                                    severity: ProblemSeverity::Error,
                                });
                            }
                        } else {
                            problems.push(CodeProblem {
                                line: line_num,
                                message: format!("Unexpected closing bracket '{}'", ch),
                                severity: ProblemSeverity::Error,
                            });
                        }
                    }
                    _ => {}
                }
            }
            i += 1;
        }
    }
    for (ch, line_num) in stack {
        problems.push(CodeProblem {
            line: line_num,
            message: format!("Unclosed bracket '{}'", ch),
            severity: ProblemSeverity::Error,
        });
    }
    if in_block_comment {
        problems.push(CodeProblem {
            line: content.lines().count().max(1),
            message: "Unclosed block comment".to_string(),
            severity: ProblemSeverity::Error,
        });
    }
    problems
}

struct CommentInfo {
    line: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
    batch: bool,
    track_single: bool,
}

fn comment_info(ext: &str) -> CommentInfo {
    match ext {
        "py" => CommentInfo {
            line: &["#"],
            block: None,
            batch: false,
            track_single: true,
        },
        "rs" => CommentInfo {
            line: &["//"],
            block: Some(("/*", "*/")),
            batch: false,
            track_single: false,
        },
        "c" | "h" | "cpp" | "cxx" | "cc" | "hpp" | "hxx" | "cs" | "java" | "go" | "js"
        | "mjs" | "cjs" => CommentInfo {
            line: &["//"],
            block: Some(("/*", "*/")),
            batch: false,
            track_single: true,
        },
        "html" | "htm" => CommentInfo {
            line: &[],
            block: Some(("<!--", "-->")),
            batch: false,
            track_single: false,
        },
        "css" => CommentInfo {
            line: &[],
            block: Some(("/*", "*/")),
            batch: false,
            track_single: false,
        },
        "php" => CommentInfo {
            line: &["//", "#"],
            block: Some(("/*", "*/")),
            batch: false,
            track_single: true,
        },
        "bat" | "cmd" => CommentInfo {
            line: &[],
            block: None,
            batch: true,
            track_single: false,
        },
        _ => CommentInfo {
            line: &[],
            block: None,
            batch: false,
            track_single: false,
        },
    }
}

fn language_keywords(ext: &str) -> Vec<&'static str> {
    match ext {
        "rs" => vec!["fn", "let", "mut", "struct", "enum", "impl", "pub", "match", "if", "else", "return", "use", "mod", "crate", "for", "in", "while", "loop"],
        "py" => vec!["def", "class", "import", "from", "return", "if", "elif", "else", "while", "for", "in", "try", "except", "pass", "break", "continue"],
        "js" | "ts" | "mjs" | "cjs" => vec!["function", "const", "let", "var", "return", "if", "else", "import", "export", "class", "async", "await", "for", "while"],
        "cpp" | "c" | "h" | "hpp" => vec!["int", "void", "return", "if", "else", "for", "while", "class", "struct", "public", "private", "#include", "std", "using", "namespace"],
        "cs" => vec!["public", "private", "class", "void", "static", "using", "namespace", "return", "if", "else", "for", "foreach", "in"],
        "java" => vec!["public", "private", "protected", "class", "void", "static", "import", "return", "if", "else", "for", "while"],
        "go" => vec!["func", "package", "import", "var", "const", "return", "if", "else", "for", "range", "type", "struct"],
        "html" | "htm" => vec!["<html>", "<body>", "<div>", "<span>", "<script>", "<style>", "<a>", "<p>", "<h1>", "<h2>"],
        "css" => vec!["color", "background", "margin", "padding", "display", "position", "width", "height", "font-size"],
        "bat" | "cmd" => vec!["echo", "set", "if", "goto", "call", "exit", "pause"],
        _ => vec![],
    }
}

fn append_with_search(
    job: &mut egui::text::LayoutJob,
    text: &str,
    format: egui::TextFormat,
    search_query: &str,
) {
    if search_query.is_empty() {
        job.append(text, 0.0, format);
        return;
    }
    let lower_text = text.to_lowercase();
    let lower_query = search_query.to_lowercase();
    let mut last_idx = 0;
    for (idx, _) in lower_text.match_indices(&lower_query) {
        if idx > last_idx {
            job.append(&text[last_idx..idx], 0.0, format.clone());
        }
        let mut highlight_format = format.clone();
        highlight_format.background = egui::Color32::from_rgb(200, 200, 50);
        highlight_format.color = egui::Color32::BLACK;
        job.append(&text[idx..idx + search_query.len()], 0.0, highlight_format);
        last_idx = idx + search_query.len();
    }
    if last_idx < text.len() {
        job.append(&text[last_idx..], 0.0, format);
    }
}

fn append_gray(job: &mut egui::text::LayoutJob, text: &str, search_query: &str) {
    if text.is_empty() {
        return;
    }
    let format = egui::TextFormat {
        font_id: egui::FontId::new(14.0, egui::FontFamily::Monospace),
        color: egui::Color32::from_rgb(128, 128, 128),
        ..Default::default()
    };
    append_with_search(job, text, format, search_query);
}

fn append_syntect(
    job: &mut egui::text::LayoutJob,
    syntax_set: &SyntaxSet,
    theme: &Theme,
    text: &str,
    ext: &str,
    search_query: &str,
) {
    if text.is_empty() {
        return;
    }
    let syntax = syntax_set
        .find_syntax_by_extension(ext)
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
    let mut h = HighlightLines::new(syntax, theme);
    for line in LinesWithEndings::from(text) {
        match h.highlight_line(line, syntax_set) {
            Ok(ranges) => {
                for (style, chunk) in ranges {
                    let color = egui::Color32::from_rgb(
                        style.foreground.r,
                        style.foreground.g,
                        style.foreground.b,
                    );
                    let format = egui::TextFormat {
                        font_id: egui::FontId::new(14.0, egui::FontFamily::Monospace),
                        color,
                        ..Default::default()
                    };
                    append_with_search(job, chunk, format, search_query);
                }
            }
            Err(_) => {
                let format = egui::TextFormat {
                    font_id: egui::FontId::new(14.0, egui::FontFamily::Monospace),
                    color: egui::Color32::WHITE,
                    ..Default::default()
                };
                append_with_search(job, line, format, search_query);
            }
        }
    }
}

fn find_marker_outside_quotes(line: &str, marker: &str, track_single: bool) -> Option<usize> {
    if marker.is_empty() {
        return None;
    }
    let mut in_double = false;
    let mut in_single = false;
    let mut escape = false;
    for (i, c) in line.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if in_double {
            if c == '"' {
                in_double = false;
            } else if c == '\\' {
                escape = true;
            }
            continue;
        }
        if track_single && in_single {
            if c == '\'' {
                in_single = false;
            } else if c == '\\' {
                escape = true;
            }
            continue;
        }
        if c == '"' {
            in_double = true;
            continue;
        }
        if track_single && c == '\'' {
            in_single = true;
            continue;
        }
        if line[i..].starts_with(marker) {
            return Some(i);
        }
    }
    None
}

fn find_batch_comment(line: &str) -> Option<usize> {
    let no_nl = line.trim_end_matches(|c| c == '\n' || c == '\r');
    let lower = no_nl.to_lowercase();
    let trimmed = lower.trim_start();
    let leading = no_nl.len() - trimmed.len();
    if trimmed.starts_with("rem ")
        || trimmed == "rem"
        || trimmed.starts_with("rem\t")
        || trimmed.starts_with("@rem ")
        || trimmed == "@rem"
        || trimmed.starts_with("::")
    {
        Some(leading)
    } else {
        None
    }
}

fn find_line_comment(line: &str, info: &CommentInfo) -> Option<usize> {
    if info.batch {
        return find_batch_comment(line);
    }
    let mut best: Option<usize> = None;
    for marker in info.line {
        if let Some(pos) = find_marker_outside_quotes(line, marker, info.track_single) {
            best = Some(match best {
                Some(old) => old.min(pos),
                None => pos,
            });
        }
    }
    best
}

fn highlight_code(
    syntax_set: &SyntaxSet,
    theme: &Theme,
    code: &str,
    ext: &str,
    search_query: &str,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let info = comment_info(ext);
    let mut in_block = false;
    for line in LinesWithEndings::from(code) {
        let mut rest = line;
        while !rest.is_empty() {
            if in_block {
                if let Some((_, end)) = info.block {
                    if let Some(pos) = rest.find(end) {
                        let end_pos = pos + end.len();
                        append_gray(&mut job, &rest[..end_pos], search_query);
                        rest = &rest[end_pos..];
                        in_block = false;
                    } else {
                        append_gray(&mut job, rest, search_query);
                        rest = "";
                    }
                } else {
                    append_gray(&mut job, rest, search_query);
                    rest = "";
                    in_block = false;
                }
            } else {
                let line_comment = find_line_comment(rest, &info);
                let block_start = info.block.as_ref().and_then(|&(start, _)| {
                    find_marker_outside_quotes(rest, start, info.track_single)
                });
                match (line_comment, block_start) {
                    (Some(lp), Some(bp)) if bp < lp => {
                        if bp > 0 {
                            append_syntect(&mut job, syntax_set, theme, &rest[..bp], ext, search_query);
                        }
                        let (start, _) = info.block.unwrap();
                        append_gray(&mut job, &rest[bp..bp + start.len()], search_query);
                        rest = &rest[bp + start.len()..];
                        in_block = true;
                    }
                    (Some(lp), _) => {
                        if lp > 0 {
                            append_syntect(&mut job, syntax_set, theme, &rest[..lp], ext, search_query);
                        }
                        append_gray(&mut job, &rest[lp..], search_query);
                        rest = "";
                    }
                    (None, Some(bp)) => {
                        if bp > 0 {
                            append_syntect(&mut job, syntax_set, theme, &rest[..bp], ext, search_query);
                        }
                        let (start, _) = info.block.unwrap();
                        append_gray(&mut job, &rest[bp..bp + start.len()], search_query);
                        rest = &rest[bp + start.len()..];
                        in_block = true;
                    }
                    (None, None) => {
                        append_syntect(&mut job, syntax_set, theme, rest, ext, search_query);
                        rest = "";
                    }
                }
            }
        }
    }
    job
}

fn language_name(ext: &str) -> &'static str {
    match ext {
        "py" => "Python",
        "cpp" | "cxx" | "cc" | "hpp" | "hxx" => "C++",
        "rs" => "Rust",
        "c" | "h" => "C",
        "cs" | "csproj" => "C#",
        "js" | "mjs" | "cjs" | "ts" => "JavaScript",
        "java" => "Java",
        "go" => "Go",
        "html" | "htm" => "HTML",
        "bat" | "cmd" => "Batch",
        "css" => "CSS",
        "php" => "PHP",
        "txt" => "Text",
        _ => "Text",
    }
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.clear();
    fonts.families.clear();
    let exe_code_font = get_asset_path("code.ttf");
    if let Ok(data) = fs::read("C:/Windows/Fonts/segoeui.ttf") {
        fonts.font_data.insert("segoe_ui".to_owned(), egui::FontData::from_owned(data));
    }
    if let Ok(data) = fs::read("C:/Windows/Fonts/seguiemj.ttf") {
        fonts.font_data.insert("segoe_ui_emoji".to_owned(), egui::FontData::from_owned(data));
    }
    if let Ok(data) = fs::read("C:/Windows/Fonts/seguisym.ttf") {
        fonts.font_data.insert("segoe_ui_symbol".to_owned(), egui::FontData::from_owned(data));
    }
    if let Ok(data) = fs::read(&exe_code_font) {
        fonts.font_data.insert("code_font".to_owned(), egui::FontData::from_owned(data));
    }
    let mut proportional: Vec<String> = vec![];
    let mut monospace: Vec<String> = vec![];
    if fonts.font_data.contains_key("segoe_ui") {
        proportional.push("segoe_ui".to_owned());
    }
    if fonts.font_data.contains_key("segoe_ui_symbol") {
        proportional.push("segoe_ui_symbol".to_owned());
    }
    if fonts.font_data.contains_key("segoe_ui_emoji") {
        proportional.push("segoe_ui_emoji".to_owned());
    }
    if fonts.font_data.contains_key("code_font") {
        monospace.push("code_font".to_owned());
    }
    if fonts.font_data.contains_key("segoe_ui") {
        monospace.push("segoe_ui".to_owned());
    }
    if fonts.font_data.contains_key("segoe_ui_symbol") {
        monospace.push("segoe_ui_symbol".to_owned());
    }
    if fonts.font_data.contains_key("segoe_ui_emoji") {
        monospace.push("segoe_ui_emoji".to_owned());
    }
    if proportional.is_empty() {
        proportional.push("segoe_ui".to_owned());
    }
    if monospace.is_empty() {
        monospace.push("segoe_ui".to_owned());
    }
    fonts.families.insert(egui::FontFamily::Proportional, proportional);
    fonts.families.insert(egui::FontFamily::Monospace, monospace);
    ctx.set_fonts(fonts);
}

fn load_window_icon() -> Option<egui::IconData> {
    let icon_path = get_asset_path("icon.ico");
    if let Ok(img) = image::open(&icon_path) {
        let width = img.width();
        let height = img.height();
        let rgba = img.into_rgba8().into_raw();
        Some(egui::IconData {
            rgba,
            width,
            height,
        })
    } else {
        None
    }
}

fn main() -> eframe::Result<()> {
    let icon = load_window_icon();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1150.0, 720.0])
        .with_title("CodeEditer");
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "CodeEditer",
        options,
        Box::new(|cc| Ok(Box::new(FlatronixEditor::new(cc)))),
    )
}