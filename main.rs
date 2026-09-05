#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};


// Templates

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


// Extensions

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


// Problems

#[derive(Clone, PartialEq)]
#[allow(dead_code)]
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


// File Tab

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

    // ⚡ Layout cache
    #[serde(skip)]
    cached_layout: Option<(String, egui::text::LayoutJob)>,
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
        }
    }
}

// Session

#[derive(Serialize, Deserialize, Default)]
struct Session {
    tabs: Vec<FileTab>,
    active_tab_index: usize,
}


// Main App

struct FlatronixEditor {
    tabs: Vec<FileTab>,
    active_tab: usize,
    syntax_set: SyntaxSet,
    theme: Theme,
    icons: HashMap<String, egui::TextureHandle>,

    editing_tab: Option<usize>,
    editing_name: String,
    rename_focus_requested: bool,

    // Discord RPC
    discord_client: Option<DiscordIpcClient>,
    discord_start_time: i64,
    last_discord_update: Instant,

    // Extensions
    extensions: Vec<Extension>,
    show_extensions_window: bool,
    extension_page: Option<String>,

    // Problems
    problems: Vec<CodeProblem>,
    problems_cache_content: String,
    problems_cache_ext: String,
    show_problems_panel: bool,
}

impl FlatronixEditor {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);

        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        cc.egui_ctx.style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(6.0, 6.0);
            style.spacing.button_padding = egui::vec2(8.0, 4.0);
        });

        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        let theme = theme_set.themes["base16-ocean.dark"].clone();

        let mut app = Self {
            tabs: vec![],
            active_tab: 0,
            syntax_set,
            theme,
            icons: HashMap::new(),

            editing_tab: None,
            editing_name: String::new(),
            rename_focus_requested: false,

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
                    description: "Show your friends on Discord what are you coding! 🎮".to_string(),
                    long_description: "This extension connects CodeEditer with Discord Rich Presence.\n\nYour friends will see:\n• What file you're editing\n• What language you're using\n• How many lines/characters you wrote\n• Whether your file is saved or not\n\nRequires Discord desktop app to be running.".to_string(),
                    author: "Flatronix".to_string(),
                    version: "1.0.0".to_string(),
                    installed: false,
                    verified: true,
                },
                Extension {
                    id: "live_server".to_string(),
                    name: "Live Server".to_string(),
                    description: "Live preview for HTML/CSS/JS files (coming soon) 🚀".to_string(),
                    long_description: "This extension will provide a live preview server for HTML, CSS and JavaScript files.\n\nFeatures (planned):\n• Auto-reload on save\n• Local server on port 5500\n• Browser sync\n\n⚠️ This extension is still in development.".to_string(),
                    author: "Flatronix".to_string(),
                    version: "0.0.1-alpha".to_string(),
                    installed: false,
                    verified: true,
                },
            ],

            show_extensions_window: false,
            extension_page: None,

            problems: vec![],
            problems_cache_content: String::new(),
            problems_cache_ext: String::new(),
            show_problems_panel: false,
        };

        app.load_icons(&cc.egui_ctx);
        app.load_session();
        app.load_extensions_state();

        // Connect Discord if extension was previously installed
        if app.is_extension_installed("discord_rpc") {
            app.init_discord();
        }

        // Open files from CLI args (Windows "Open with")
        let args: Vec<PathBuf> = env::args().skip(1).map(PathBuf::from).collect();
        for arg in args {
            if arg.is_file() {
                app.open_path(&arg);
            }
        }

        if app.tabs.is_empty() {
            app.tabs.push(FileTab::new(
                None,
                "new_file.txt".to_string(),
                "Hi! To start try creating a new file using the file menu! 👋\n".to_string(),
            ));
        }

        app
    }


    // Extensions helpers

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


    // Discord RPC

    fn init_discord(&mut self) {
        let app_id = "1545533503519596594";
        let mut client = DiscordIpcClient::new(app_id);

        match client.connect() {
            Ok(_) => {
                println!("Discord RPC: Connected!");
                self.discord_client = Some(client);
            }
            Err(e) => {
                println!("Discord RPC: Cannot connect: {}", e);
            }
        }
    }

    fn disconnect_discord(&mut self) {
        if let Some(mut client) = self.discord_client.take() {
            let _ = client.close();
            println!("🔌 Discord RPC: Disconnected");
        }
    }

    fn update_discord_presence(&mut self) {
        // Only if extension is installed
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
                    "js" | "mjs" | "cjs" => "javascript",
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

        if let Err(e) = client.set_activity(activity) {
            println!("Discord RPC: Update error: {}", e);

            if client.connect().is_ok() {
                println!("Discord RPC: Reconnected");
            }
        }
    }

    // Problems

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


    // Icons

    fn load_icons(&mut self, ctx: &egui::Context) {
        let icon_map: [(&str, &str); 13] = [
            ("py", "icons/py.ico"),
            ("cplus", "icons/cplus.ico"),
            ("html", "html.ico"),
            ("rs", "rs.ico"),
            ("bat", "bat.ico"),
            ("js", "js.ico"),
            ("go", "go.ico"),
            ("c", "c.ico"),
            ("csharp", "csharp.ico"),
            ("css", "css.ico"),
            ("txt", "txt.ico"),
            ("php", "php.ico"),
            ("verified", "icons/verified.ico"),
        ];

        for (key, path) in icon_map.iter() {
            let candidates = [path.to_string(), format!("icons/{}", path)];

            for candidate in candidates {
                if let Ok(img) = image::open(&candidate) {
                    let size = [img.width() as usize, img.height() as usize];
                    let rgba = img.into_rgba8().into_raw();

                    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                    let texture =
                        ctx.load_texture(*key, color_image, egui::TextureOptions::LINEAR);

                    self.icons.insert((*key).to_string(), texture);
                    break;
                }
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
            "js" | "mjs" | "cjs" => "js",
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
}

     // GUI

impl eframe::App for FlatronixEditor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 🎮 Discord RPC
        self.update_discord_presence();

        // Problems
        self.update_problems();

        // Drag & drop
        let mut dropped_paths: Vec<PathBuf> = Vec::new();
        ctx.input(|i| {
            for file in &i.raw.dropped_files {
                if let Some(path) = &file.path {
                    dropped_paths.push(path.clone());
                }
            }
        });

        for path in dropped_paths {
            self.open_path(&path);
        }

        let mut create_template_cmd: Option<NewTemplate> = None;
        let mut save_current = false;
        let mut close_current = false;
        let mut open_file_dialog = false;

        let mut new_shortcut = false;
        let mut undo_shortcut = false;
        let mut redo_shortcut = false;
        let mut copy_all_shortcut = false;
        let mut paste_all_text: Option<String> = None;

        let editing_name_mode = self.editing_tab.is_some();
        let ui_has_focus = ctx.memory(|mem| mem.focused().is_some());

        // Keybinds
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

                if i.key_pressed(egui::Key::Z) {
                    if i.modifiers.shift {
                        redo_shortcut = true;
                        i.consume_key(
                            egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
                            egui::Key::Z,
                        );
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

            if !ui_has_focus && !editing_name_mode {
                for ev in &i.raw.events {
                    if let egui::Event::Paste(text) = ev {
                        paste_all_text = Some(text.clone());
                    }
                }
            }
        });

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


        // Menu bar

        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New File").clicked() {
                        create_template_cmd = Some(NewTemplate::Plain);
                        ui.close_menu();
                    }

                    // Open File
                    if ui.button("Open File").clicked() {
                        open_file_dialog = true;
                        ui.close_menu();
                    }

                    ui.menu_button("Templates", |ui| {
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

                    if ui.button("Save").clicked() {
                        save_current = true;
                        ui.close_menu();
                    }

                    if ui.button("Close Active Tab").clicked() {
                        close_current = true;
                        ui.close_menu();
                    }
                });

                // Extensions button
                if ui.button("Extensions").clicked() {
                    self.show_extensions_window = !self.show_extensions_window;
                }

                ui.separator();
                ui.label("");
            });
        });

        // Open File dialog
        if open_file_dialog {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Open File")
                .pick_file()
            {
                self.open_path(&path);
            }
        }

        
        // Extensions Window

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
                    .with_title("Extensions")
                    .with_inner_size([480.0, 560.0])
                    .with_min_inner_size([380.0, 420.0]),
                |ctx, _class| {
                    if ctx.input(|i| i.viewport().close_requested()) {
                        close_requested = true;
                    }

                    ctx.set_visuals(egui::Visuals::dark());
                    ctx.style_mut(|style| {
                        style.spacing.item_spacing = egui::vec2(6.0, 6.0);
                        style.spacing.button_padding = egui::vec2(8.0, 4.0);
                    });

                    egui::CentralPanel::default().show(ctx, |ui| {
                        match &current_page {
                            // ── Extension detail page ──
                            Some(page_id) => {
                                let ext_data = extensions_clone
                                    .iter()
                                    .find(|e| &e.id == page_id)
                                    .cloned();

                                if let Some(ext) = ext_data {
                                    if ui.button("<- Back to list").clicked() {
                                        go_back = true;
                                    }

                                    ui.separator();
                                    ui.heading(&ext.name);
                                    ui.add_space(4.0);

                                    ui.horizontal(|ui| {
                                        ui.label("Version:");
                                        ui.label(egui::RichText::new(&ext.version).strong());
                                    });

   
                                    ui.horizontal(|ui| {
                                        ui.label("Author:");
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
                                                egui::RichText::new("Installed")
                                                    .color(egui::Color32::from_rgb(100, 220, 100)),
                                            );
                                        } else {
                                            ui.label(
                                                egui::RichText::new("Not installed")
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
                                        if ui.button("Uninstall").clicked() {
                                            extension_action = Some((ext.id.clone(), false));
                                        }
                                    } else if ui.button("Install").clicked() {
                                        extension_action = Some((ext.id.clone(), true));
                                    }
                                } else if ui.button("<- Back to list").clicked() {
                                    go_back = true;
                                }
                            }

                            // ── Extensions list ──
                            None => {
                                ui.label("Extensions");
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
                                                    if ui.button("Details ->").clicked() {
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

            // Install / Uninstall extensions
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


        // Tab bar

        let mut selected_tab: Option<usize> = None;
        let mut tab_to_close: Option<usize> = None;
        let mut start_rename: Option<usize> = None;
        let mut commit_rename: Option<(usize, String)> = None;
        let mut cancel_rename = false;

        let current_active = self.active_tab;
        let editing_tab = self.editing_tab;

        egui::TopBottomPanel::top("tabs_panel").show(ctx, |ui| {
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


        // Problems panel (above status bar)

        if self.show_problems_panel {
            egui::TopBottomPanel::bottom("problems_panel")
                .min_height(120.0)
                .max_height(250.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Problems").strong());

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
                                    egui::RichText::new("No problems detected")
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
                                            "{} Line {}: {}",
                                            icon, problem.line, problem.message
                                        ))
                                        .color(color),
                                    );
                                }
                            }
                        });
                });
        }


        // Status bar

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
                    ui.label(format!("Lines: {}", lines));
                    ui.separator();
                    ui.label(format!("Characters: {}", chars));
                    ui.separator();

                    if tab.is_modified {
                        ui.label("Modified");
                    } else {
                        ui.label("Saved");
                    }
                } else {
                    ui.label("No opened files");
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let problems_count = self.problems.len();

                    let problems_label = if problems_count == 0 {
                        "No problems".to_string()
                    } else {
                        format!("{} problem(s)", problems_count)
                    };

                    if ui.button(problems_label).clicked() {
                        self.show_problems_panel = !self.show_problems_panel;
                    }

                    ui.separator();
                    ui.label("Ctrl+S | Ctrl+N | Ctrl+Z | Ctrl+Y");
                });
            });
        });


        // Process tab actions

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


        // Main editor area
        {
            let active_tab = self.active_tab;
            let syntax_set = &self.syntax_set;
            let theme = &self.theme;

            egui::CentralPanel::default().show(ctx, |ui| {
                if let Some(tab) = self.tabs.get_mut(active_tab) {
                    let ext = Path::new(&tab.name)
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("txt")
                        .to_lowercase();

                    // ⚡ Cache: regenerate layout only when content changes
                    let needs_regen = match &tab.cached_layout {
                        Some((cached_content, _)) => *cached_content != tab.content,
                        None => true,
                    };

                    if needs_regen {
                        let mut layout = highlight_code(syntax_set, theme, &tab.content, &ext);
                        layout.wrap.max_width = f32::INFINITY; // 🚫 No word wrap!
                        tab.cached_layout = Some((tab.content.clone(), layout));
                    }

                    let layout_clone = tab.cached_layout.as_ref().unwrap().1.clone();

                    // Line numbers
                    let line_count = tab.content.lines().count().max(1);
                    let mut line_numbers = (1..=line_count)
                        .map(|n| format!("{:>4}", n))
                        .collect::<Vec<_>>()
                        .join("\n");

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
                                        .text_color(egui::Color32::from_rgb(120, 120, 120)),
                                );

                                ui.separator();

                                let mut layouter =
                                    move |ui: &egui::Ui, _string: &str, _wrap_width: f32| {
                                        let job = layout_clone.clone();
                                        ui.fonts(|f| f.layout_job(job))
                                    };

                                let response = ui.add(
                                    egui::TextEdit::multiline(&mut tab.content)
                                        .font(egui::FontId::new(
                                            14.0,
                                            egui::FontFamily::Monospace,
                                        ))
                                        .desired_width(f32::INFINITY)
                                        .layouter(&mut layouter),
                                );

                                if response.changed() {
                                    Self::register_change(tab);
                                }
                            });
                        });
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label("No open files. Click File tab");
                    });
                }
            });
        }

        // Auto save session
        self.save_session();
    }
}


// Check code problems

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
                // Check line comment
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

                // Check block comment start
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

                // Check brackets
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


// Comment syntax

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


// Syntax highlighting

fn append_gray(job: &mut egui::text::LayoutJob, text: &str) {
    if text.is_empty() {
        return;
    }

    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::new(14.0, egui::FontFamily::Monospace),
            color: egui::Color32::from_rgb(128, 128, 128),
            ..Default::default()
        },
    );
}

fn append_syntect(
    job: &mut egui::text::LayoutJob,
    syntax_set: &SyntaxSet,
    theme: &Theme,
    text: &str,
    ext: &str,
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

                    job.append(
                        chunk,
                        0.0,
                        egui::TextFormat {
                            font_id: egui::FontId::new(14.0, egui::FontFamily::Monospace),
                            color,
                            ..Default::default()
                        },
                    );
                }
            }
            Err(_) => {
                job.append(
                    line,
                    0.0,
                    egui::TextFormat {
                        font_id: egui::FontId::new(14.0, egui::FontFamily::Monospace),
                        color: egui::Color32::WHITE,
                        ..Default::default()
                    },
                );
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
                        append_gray(&mut job, &rest[..end_pos]);
                        rest = &rest[end_pos..];
                        in_block = false;
                    } else {
                        append_gray(&mut job, rest);
                        rest = "";
                    }
                } else {
                    append_gray(&mut job, rest);
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
                            append_syntect(&mut job, syntax_set, theme, &rest[..bp], ext);
                        }

                        let (start, _) = info.block.unwrap();
                        append_gray(&mut job, &rest[bp..bp + start.len()]);
                        rest = &rest[bp + start.len()..];
                        in_block = true;
                    }
                    (Some(lp), _) => {
                        if lp > 0 {
                            append_syntect(&mut job, syntax_set, theme, &rest[..lp], ext);
                        }

                        append_gray(&mut job, &rest[lp..]);
                        rest = "";
                    }
                    (None, Some(bp)) => {
                        if bp > 0 {
                            append_syntect(&mut job, syntax_set, theme, &rest[..bp], ext);
                        }

                        let (start, _) = info.block.unwrap();
                        append_gray(&mut job, &rest[bp..bp + start.len()]);
                        rest = &rest[bp + start.len()..];
                        in_block = true;
                    }
                    (None, None) => {
                        append_syntect(&mut job, syntax_set, theme, rest, ext);
                        rest = "";
                    }
                }
            }
        }
    }

    job
}


// Language name

fn language_name(ext: &str) -> &'static str {
    match ext {
        "py" => "Python",
        "cpp" | "cxx" | "cc" | "hpp" | "hxx" => "C++",
        "rs" => "Rust",
        "c" | "h" => "C",
        "cs" | "csproj" => "C#",
        "js" | "mjs" | "cjs" => "JavaScript",
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


// Fonts

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();


    fonts.font_data.clear();
    fonts.families.clear();

    // Load Segoe UI
    if let Ok(data) = fs::read("C:/Windows/Fonts/segoeui.ttf") {
        fonts
            .font_data
            .insert("segoe_ui".to_owned(), egui::FontData::from_owned(data));
    }


    if let Ok(data) = fs::read("C:/Windows/Fonts/seguiemj.ttf") {
        fonts
            .font_data
            .insert("segoe_ui_emoji".to_owned(), egui::FontData::from_owned(data));
    }


    if let Ok(data) = fs::read("C:/Windows/Fonts/seguisym.ttf") {
        fonts
            .font_data
            .insert("segoe_ui_symbol".to_owned(), egui::FontData::from_owned(data));
    }

    if let Ok(data) = fs::read("code.ttf") {
        fonts
            .font_data
            .insert("code_font".to_owned(), egui::FontData::from_owned(data));
    }

    // Build families
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

    fonts
        .families
        .insert(egui::FontFamily::Proportional, proportional);

    fonts.families.insert(egui::FontFamily::Monospace, monospace);

    ctx.set_fonts(fonts);
}


// Window icon

fn load_window_icon() -> Option<egui::IconData> {
    if let Ok(img) = image::open("icon.ico") {
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


// Main

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