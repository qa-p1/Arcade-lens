//! Settings — Arcade Lens: the standard Template A window on the Arcade
//! design kit. Changes apply at once; fields that need checking (paths,
//! service URLs, currency, languages, action keys, chain names) apply on
//! Enter or when they lose focus and show an inline error instead.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arcade_link::design::egui::components::{ButtonKind, Kit, NavItem, ShortcutRecorder, ShortcutSpec, Toast, Tone};
use arcade_link::design::{size, text};
use eframe::egui::{self, RichText, ViewportBuilder, ViewportId};
use lens_core::capability::Capability;
use lens_core::chain::{Chain, ChainStep};
use lens_core::settings::{Appearance, DateOrder, ImageFormat};
use lens_core::{caps, Produces, Settings};

use super::overlay::{cap_label, effects_text};
use super::runtime::Runtime;
use super::theme;

/// The one-line description used everywhere (README, Tools, About, tray).
pub const TAGLINE: &str = "Select anything on screen and act on it.";
pub const GUIDE_URL: &str = "https://github.com/qa-p1/Arcade-lens/blob/main/docs/user/index.md";
const LINK_GUIDE_URL: &str = "https://github.com/qa-p1/Arcade-lens/blob/main/docs/user/with-other-apps.md";
const LICENCES_URL: &str = "https://github.com/qa-p1/Arcade-lens/blob/main/THIRD_PARTY_NOTICES.md";
pub const DEFAULT_SHORTCUT: &str = "Ctrl+Alt+Shift+L";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    General,
    Shortcuts,
    Actions,
    TextRecognition,
    Chains,
    Services,
    Plugins,
    Privacy,
    ConnectedApps,
    About,
}

impl Section {
    /// The standard order: General, Shortcuts, the app's own sections,
    /// Connected apps, About.
    pub const ALL: [Section; 10] = [
        Section::General,
        Section::Shortcuts,
        Section::Actions,
        Section::TextRecognition,
        Section::Chains,
        Section::Services,
        Section::Plugins,
        Section::Privacy,
        Section::ConnectedApps,
        Section::About,
    ];

    /// The section whose label is `label` (as sent over IPC).
    pub fn from_label(label: &str) -> Option<Section> {
        Section::ALL.into_iter().find(|s| s.label().eq_ignore_ascii_case(label.trim()))
    }

    pub fn label(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Shortcuts => "Shortcuts",
            Section::Actions => "Actions",
            Section::TextRecognition => "Text recognition",
            Section::Chains => "Chains",
            Section::Services => "Services",
            Section::Plugins => "Plugins",
            Section::Privacy => "Privacy",
            Section::ConnectedApps => "Connected apps",
            Section::About => "About",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::General => "settings",
            Section::Shortcuts => "keyboard",
            Section::Actions => "list",
            Section::TextRecognition => "type",
            Section::Chains => "link",
            Section::Services => "globe",
            Section::Plugins => "cube",
            Section::Privacy => "shield",
            Section::ConnectedApps => "apps",
            Section::About => "info",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Section::General => "How Lens looks, starts, saves and reads things.",
            Section::Shortcuts => "The shortcut that starts a capture, and the keys inside it.",
            Section::Actions => "Turn actions off, mark favourites or change their key.",
            Section::TextRecognition => "Reading text in selections. Everything runs on this computer.",
            Section::Chains => "Reusable sequences of actions, checked as you build them.",
            Section::Services => "The websites actions that need the internet use.",
            Section::Plugins => "Extra recognizers and actions, off until you turn them on.",
            Section::Privacy => "What Lens keeps, and where.",
            Section::ConnectedApps => "Which Arcade apps work with Lens, and how.",
            Section::About => "Version, updates, help and licences.",
        }
    }
}

pub enum SettingsRequest {
    Save {
        settings: Box<Settings>,
        chains: Vec<Chain>,
    },
    DownloadTesseract,
    ResetUsage,
    ClearHistory,
    ShowHistory,
    Autostart(bool),
    InstallLauncher,
    OpenFolder(std::path::PathBuf),
    OpenUrl(String),
    /// Ask Arcade Tools to install the app (or open its releases page).
    GetApp(String),
    /// Start an installed app.
    OpenApp(String),
    Quit,
}

/// A destructive action waiting for the user's confirmation.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Confirm {
    DeleteChain(usize),
    ClearHistory,
}

pub struct SettingsView {
    rt: Arc<Runtime>,
    /// The applied settings: every valid change is saved at once.
    settings: Settings,
    /// Chains as saved; `chain_drafts` holds edits until they are valid.
    chains: Vec<Chain>,
    chain_drafts: Vec<Chain>,
    page: usize,
    recorder: ShortcutRecorder,
    chain_sel: usize,
    action_filter: String,
    focus_filter: bool,
    /// Text being edited, by field, until Enter or blur commits it.
    buffers: HashMap<&'static str, String>,
    errors: HashMap<&'static str, String>,
    confirm: Option<Confirm>,
    toast: Option<Toast>,
    save_error: Option<String>,
    /// Set by the shell with the outcome of a request.
    pub status: Option<(String, bool)>,
    pub closed: bool,
    pub autostart: bool,
    pub launcher: bool,
    pub tesseract_busy: bool,
}

const REVERSE_IMAGE_PRESETS: &[(&str, &str)] = &[
    ("Google Lens", "https://lens.google.com/"),
    ("Bing Visual Search", "https://www.bing.com/visualsearch"),
    ("Yandex Images", "https://yandex.com/images/"),
    ("TinEye", "https://tineye.com/"),
];

/// Services: label, field, placeholders the template must contain.
const SERVICES: [(&str, &str, &[&str]); 5] = [
    ("Web search", "web_search", &["{query}"]),
    ("Maps search", "maps_search", &["{query}"]),
    ("Maps for coordinates", "maps_coordinates", &["{lat}", "{lon}"]),
    ("Product search", "product_search", &["{query}"]),
    ("Domain lookup (WHOIS)", "whois", &["{query}"]),
];

/// Checks a service URL template; returns the trimmed template.
pub fn valid_service(template: &str, needs: &[&str]) -> Result<String, String> {
    let t = template.trim();
    if !(t.starts_with("https://") || t.starts_with("http://")) {
        return Err("Use a web address that starts with https://.".into());
    }
    if t.chars().any(char::is_whitespace) {
        return Err("A web address can't contain spaces.".into());
    }
    if let Some(missing) = needs.iter().find(|n| !t.contains(**n)) {
        return Err(format!("Include {missing} where Lens should put the value."));
    }
    Ok(t.to_string())
}

/// ISO 4217: three letters, or empty for none.
pub fn valid_currency(code: &str) -> Result<Option<String>, String> {
    let c = code.trim();
    if c.is_empty() {
        return Ok(None);
    }
    if c.len() == 3 && c.chars().all(|ch| ch.is_ascii_alphabetic()) {
        Ok(Some(c.to_ascii_uppercase()))
    } else {
        Err("Use a three-letter currency code, such as EUR or INR.".into())
    }
}

/// Tesseract language codes, comma separated, at least one.
pub fn valid_languages(list: &str) -> Result<Vec<String>, String> {
    let codes: Vec<String> = list.split(',').map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    if codes.is_empty() {
        return Err("Enter at least one language code, such as eng.".into());
    }
    if let Some(bad) = codes.iter().find(|c| c.len() > 32 || !c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')) {
        return Err(format!("\u{201c}{bad}\u{201d} isn't a language code. Use codes such as eng or deu, separated by commas."));
    }
    Ok(codes)
}

/// A palette key: one letter or digit, or empty for the action's default.
pub fn valid_action_key(key: &str) -> Result<Option<char>, String> {
    let mut chars = key.trim().chars();
    match (chars.next(), chars.next()) {
        (None, _) => Ok(None),
        (Some(c), None) if c.is_ascii_alphanumeric() => Ok(Some(c.to_ascii_lowercase())),
        _ => Err("Use one letter or digit.".into()),
    }
}

/// Chains that may be saved: valid and named, with unique names.
pub fn chain_problem(chain: &Chain, all: &[Chain], registry: &lens_core::Registry) -> Option<String> {
    if chain.name.trim().is_empty() {
        return Some("Give the chain a name.".into());
    }
    if all.iter().filter(|c| c.name.trim() == chain.name.trim()).count() > 1 {
        return Some(format!("Another chain is already called \u{201c}{}\u{201d}.", chain.name.trim()));
    }
    chain.validate(registry).err().map(|e| format!("This chain can't run yet: {e}."))
}

impl SettingsView {
    pub fn new(rt: Arc<Runtime>) -> SettingsView {
        SettingsView {
            settings: (*rt.settings).clone(),
            chains: rt.chains.clone(),
            chain_drafts: rt.chains.clone(),
            rt,
            page: 0,
            recorder: ShortcutRecorder::default(),
            chain_sel: 0,
            action_filter: String::new(),
            focus_filter: false,
            buffers: HashMap::new(),
            errors: HashMap::new(),
            confirm: None,
            toast: None,
            save_error: None,
            status: None,
            closed: false,
            autostart: lens_platform::autostart::is_enabled(),
            launcher: lens_platform::autostart::launcher_installed(),
            tesseract_busy: false,
        }
    }

    pub fn viewport_id() -> ViewportId {
        ViewportId::from_hash_of("lens-settings")
    }

    pub fn builder() -> ViewportBuilder {
        ViewportBuilder::default()
            .with_title("Settings — Arcade Lens")
            .with_inner_size([760.0, 560.0])
            .with_min_inner_size([720.0, 520.0])
            .with_app_id(crate::link::OS_ID)
            .with_icon(crate::icon::window_icon())
    }

    /// Opens on `section` (the overlay's "Install text recognition").
    pub fn show_section(&mut self, section: Section) {
        self.page = Section::ALL.iter().position(|s| *s == section).unwrap_or(0);
    }

    pub fn refresh_runtime(&mut self, rt: Arc<Runtime>) {
        // Keep edits in progress; adopt what was saved elsewhere.
        if self.buffers.is_empty() {
            self.settings = (*rt.settings).clone();
        }
        if self.chain_drafts == self.chains {
            self.chain_drafts = rt.chains.clone();
        }
        self.chains = rt.chains.clone();
        self.rt = rt;
    }

    fn save(&mut self, out: &mut Vec<SettingsRequest>) {
        out.push(SettingsRequest::Save { settings: Box::new(self.settings.clone()), chains: self.chains.clone() });
    }

    pub fn ui(&mut self, ctx: &egui::Context) -> Vec<SettingsRequest> {
        let ctx = ctx.clone();
        theme::sync(&ctx);
        if ctx.input(|i| i.viewport().close_requested()) {
            self.closed = true;
        }
        let mut out = Vec::new();
        if let Some((message, ok)) = self.status.take() {
            if ok {
                self.save_error = None;
                // Controls show their own state; no "Saved" notices.
                if message != "Saved" {
                    self.toast = Some(Toast::new(message, Tone::Ok));
                }
            } else {
                self.save_error = Some(message);
            }
        }
        self.keys(&ctx, &mut out);
        let kit = theme::kit();
        let items: Vec<NavItem> = Section::ALL.iter().map(|s| NavItem { label: s.label(), icon: s.icon(), count: None }).collect();
        let brand = super::glyphs::texture(&ctx, crate::link::ME, 16.0);
        kit.sidebar_with_brand(&ctx, "Arcade Lens", crate::link::VERSION, brand, &items, &mut self.page);
        let section = Section::ALL[self.page];
        let mut new_chain = false;
        kit.page_with_head_actions(
            &ctx,
            section.label(),
            section.description(),
            |ui| {
                if section == Section::Chains {
                    new_chain = kit.button(ui, "New chain", ButtonKind::Primary).clicked();
                }
            },
            |ui| {
                if let Some(error) = self.save_error.clone() {
                    let mut retry = false;
                    kit.notice_action(ui, &error, Tone::Danger, |ui| {
                        retry = kit.button_state(ui, "Retry", ButtonKind::Default, true, false, true).clicked();
                    });
                    ui.add_space(16.0);
                    if retry {
                        self.save_error = None;
                        self.save(&mut out);
                    }
                }
                match section {
                    Section::General => self.general(ui, &kit, &mut out),
                    Section::Shortcuts => self.shortcuts(ui, &kit, &mut out),
                    Section::Actions => self.actions(ui, &kit, &mut out),
                    Section::TextRecognition => self.text_recognition(ui, &kit, &mut out),
                    Section::Chains => self.chains_page(ui, &kit),
                    Section::Services => self.services(ui, &kit, &mut out),
                    Section::Plugins => self.plugins(ui, &kit, &mut out),
                    Section::Privacy => self.privacy(ui, &kit, &mut out),
                    Section::ConnectedApps => self.connected_apps(ui, &kit, &mut out),
                    Section::About => self.about(ui, &kit, &mut out),
                }
            },
        );
        if new_chain {
            let n = self.chain_drafts.len() + 1;
            let mut id = format!("chain-{n}");
            let mut k = n;
            while self.chain_drafts.iter().any(|c| c.id == id) {
                k += 1;
                id = format!("chain-{k}");
            }
            self.chain_drafts.push(Chain { id, name: format!("My chain {k}"), steps: vec![ChainStep::Take { capability: caps::TEXT }] });
            self.chain_sel = self.chain_drafts.len() - 1;
        }
        self.commit_chains(&mut out);
        self.confirm_dialog(&ctx, &kit, &mut out);
        if let Some(t) = &self.toast {
            if !t.show(&ctx, &kit) {
                self.toast = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }
        out
    }

    /// Esc and Ctrl+W close; F1 opens the guide; Ctrl+K filters actions;
    /// Ctrl+Q quits Lens.
    fn keys(&mut self, ctx: &egui::Context, out: &mut Vec<SettingsRequest>) {
        let recording = self.recorder.recording;
        let editing = ctx.memory(|m| m.focused().is_some());
        let (close, help, filter, quit) = ctx.input_mut(|i| {
            let cmd = egui::Modifiers::COMMAND;
            (
                (!recording && self.confirm.is_none() && !editing && i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
                    || i.consume_key(cmd, egui::Key::W),
                i.consume_key(egui::Modifiers::NONE, egui::Key::F1),
                i.consume_key(cmd, egui::Key::K),
                i.consume_key(cmd, egui::Key::Q),
            )
        });
        if close {
            self.closed = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if help {
            out.push(SettingsRequest::OpenUrl(GUIDE_URL.into()));
        }
        if filter {
            self.show_section(Section::Actions);
            self.focus_filter = true;
        }
        if quit {
            out.push(SettingsRequest::Quit);
        }
    }

    /// A text field that applies on Enter or blur through `apply`, which
    /// returns an error message for invalid input. Other settings are saved
    /// as usual while it is invalid.
    fn field(
        &mut self,
        ui: &mut egui::Ui,
        key: &'static str,
        current: String,
        hint: &str,
        out: &mut Vec<SettingsRequest>,
        apply: impl FnOnce(&mut Settings, &str) -> Result<(), String>,
    ) -> egui::Response {
        let mut value = self.buffers.get(key).cloned().unwrap_or(current);
        let error = self.errors.get(key).cloned();
        let r = super::theme::kit().text_input(ui, &mut value, hint, error.as_deref());
        if r.changed() {
            self.buffers.insert(key, value.clone());
        }
        let commit = r.lost_focus() && self.buffers.contains_key(key);
        if commit {
            let mut next = self.settings.clone();
            match apply(&mut next, &value) {
                Ok(()) => {
                    self.errors.remove(key);
                    self.buffers.remove(key);
                    if next != self.settings {
                        self.settings = next;
                        self.save(out);
                    }
                }
                Err(e) => {
                    self.errors.insert(key, e);
                }
            }
        }
        r
    }

    fn general(&mut self, ui: &mut egui::Ui, kit: &Kit, out: &mut Vec<SettingsRequest>) {
        let mut changed = false;
        kit.list(ui, |ui| {
            kit.row(ui, "Appearance", "System, light or dark.", |ui| {
                changed |= kit
                    .segmented(ui, &mut self.settings.appearance, &[(Appearance::System, "System"), (Appearance::Light, "Light"), (Appearance::Dark, "Dark")])
                    .changed();
            });
            let mut login = self.autostart;
            if kit.switch_row(ui, &mut login, "Start at login", "Lens starts in the background, ready for the shortcut.").changed() {
                out.push(SettingsRequest::Autostart(login));
                self.autostart = login;
            }
            changed |= kit
                .switch_row(ui, &mut self.settings.notifications, "Notifications", "When an action fails or finishes while no Lens window is open.")
                .changed();
        });
        if changed {
            theme::apply_style(ui.ctx(), self.settings.appearance.as_str());
            self.save(out);
        }
        kit.section_head(ui, "Saving");
        let mut format_changed = false;
        kit.list(ui, |ui| {
            let dir = self.settings.screenshot_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
            kit.row(ui, "Save folder", "Where images and results are saved. Empty uses Pictures/Arcade Lens.", |ui| {
                ui.set_max_width(300.0);
                self.field(ui, "save_dir", dir, "Pictures/Arcade Lens", out, |s, v| {
                    let v = v.trim();
                    s.screenshot_dir = (!v.is_empty()).then(|| std::path::PathBuf::from(v));
                    Ok(())
                });
            });
            kit.row(ui, "Image format", "", |ui| {
                format_changed |= kit
                    .segmented(ui, &mut self.settings.image_format, &[(ImageFormat::Png, "PNG"), (ImageFormat::Jpeg, "JPEG"), (ImageFormat::Webp, "WebP")])
                    .changed();
            });
        });
        if format_changed {
            self.save(out);
        }
        kit.section_head(ui, "Opening");
        kit.list(ui, |ui| {
            for (key, title, hint) in [
                ("browser", "Browser", "System default, or e.g. firefox"),
                ("terminal", "Terminal", "Found automatically, or e.g. kitty"),
                ("editor", "Editor", "System default, or e.g. code"),
            ] {
                let current = match key {
                    "browser" => self.settings.browser.clone(),
                    "terminal" => self.settings.terminal.clone(),
                    _ => self.settings.editor.clone(),
                }
                .unwrap_or_default();
                kit.row(ui, title, "", |ui| {
                    ui.set_max_width(300.0);
                    self.field(ui, key, current, hint, out, |s, v| {
                        let v = (!v.trim().is_empty()).then(|| v.trim().to_string());
                        match key {
                            "browser" => s.browser = v,
                            "terminal" => s.terminal = v,
                            _ => s.editor = v,
                        }
                        Ok(())
                    });
                });
            }
        });
        kit.section_head(ui, "Reading");
        let mut reading_changed = false;
        kit.list(ui, |ui| {
            kit.row(ui, "Date reading order", "For dates like 05/03. Ambiguous dates still show both readings.", |ui| {
                reading_changed |=
                    kit.segmented(ui, &mut self.settings.date_order, &[(DateOrder::DayFirst, "Day first"), (DateOrder::MonthFirst, "Month first")]).changed();
            });
            let currency = self.settings.home_currency.clone().unwrap_or_default();
            kit.row(ui, "Home currency", "Amounts convert to this currency.", |ui| {
                ui.set_max_width(160.0);
                self.field(ui, "currency", currency, "e.g. EUR", out, |s, v| {
                    s.home_currency = valid_currency(v)?;
                    Ok(())
                });
            });
            kit.row(ui, "Actions shown first", "How many actions the palette shows before More.", |ui| {
                let options: Vec<(usize, String)> = (3..=8).map(|n| (n, n.to_string())).collect();
                let options: Vec<(usize, &str)> = options.iter().map(|(n, l)| (*n, l.as_str())).collect();
                reading_changed |= kit.select(ui, "primary-count", &mut self.settings.primary_action_count, &options).changed();
            });
        });
        if reading_changed {
            self.save(out);
        }
        if cfg!(target_os = "linux") {
            kit.section_head(ui, "Applications menu");
            kit.list(ui, |ui| {
                let (title, description) = if self.launcher {
                    ("In the applications menu", "Arcade Lens has a menu entry and icon.")
                } else {
                    ("Not in the applications menu", "Add an entry so you can start Lens from your desktop's menu.")
                };
                kit.row(ui, title, description, |ui| {
                    let label = if self.launcher { "Repair" } else { "Add to menu" };
                    if kit.button_state(ui, label, ButtonKind::Default, true, false, true).clicked() {
                        out.push(SettingsRequest::InstallLauncher);
                    }
                });
            });
        }
    }

    fn shortcuts(&mut self, ui: &mut egui::Ui, kit: &Kit, out: &mut Vec<SettingsRequest>) {
        let snapshot = self.rt.arcade.snapshot();
        let mut value = self.settings.activation_shortcut.clone().unwrap_or_default();
        let status = if value.is_empty() { "Off. Start a capture from the tray instead." } else { "Works from any application." };
        let mut changed = false;
        kit.list(ui, |ui| {
            let spec = ShortcutSpec { title: "Start a capture", description: status, default: DEFAULT_SHORTCUT, allow_clear: true };
            changed = self.recorder.row(ui, kit, &spec, &mut value, |accelerator| {
                snapshot
                    .installed
                    .shortcut_owner(arcade_link::ids::LENS, accelerator)
                    .or_else(|| lens_platform::shortcut::known_conflict(accelerator).map(|what| format!("{what} (usually)")))
            });
        });
        if changed {
            let next = (!value.is_empty()).then_some(value.clone());
            match next.as_deref().map(lens_platform::shortcut::parse) {
                Some(Err(e)) => self.save_error = Some(format!("Couldn't use that shortcut. {e}")),
                _ => {
                    self.settings.activation_shortcut = next;
                    self.save(out);
                }
            }
        }
        if lens_platform::display_server() == lens_platform::DisplayServer::Wayland {
            ui.add_space(10.0);
            if lens_platform::hyprland::active() {
                kit.notice(ui, "Lens adds this shortcut to Hyprland while it runs. There's nothing to set up.", Tone::Neutral);
            } else {
                kit.notice(
                    ui,
                    "This desktop doesn't let apps register global shortcuts. Bind a shortcut in its keyboard settings to the command below.",
                    Tone::Warn,
                );
                let exe = lens_platform::autostart::launch_path().map(|p| p.display().to_string()).unwrap_or_else(|_| "arcade-lens".into());
                let cmd = format!("{exe} --capture");
                ui.add_space(10.0);
                let mut copy = false;
                kit.list(ui, |ui| {
                    kit.row(ui, &cmd, "The command that starts a capture.", |ui| {
                        copy = kit.button_state(ui, "Copy", ButtonKind::Default, true, false, true).clicked();
                    });
                });
                if copy {
                    ui.ctx().copy_text(cmd);
                    self.toast = Some(Toast::copied());
                }
            }
        }
        let _ = out;
        for (group, keys) in palette_keys() {
            kit.section_head(ui, &group);
            kit.list(ui, |ui| {
                for (title, combos) in keys {
                    kit.row(ui, &title, "", |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (i, combo) in combos.iter().enumerate() {
                            if i > 0 {
                                ui.label(RichText::new("or").font(theme::font(text::CAPTION)).color(theme::c().muted));
                            }
                            for part in combo.split('+') {
                                kit.key_chip(ui, &key_label(part));
                            }
                        }
                    });
                }
            });
        }
    }

    fn actions(&mut self, ui: &mut egui::Ui, kit: &Kit, out: &mut Vec<SettingsRequest>) {
        let r = kit.text_input(ui, &mut self.action_filter, "Filter actions (Ctrl+K)", None);
        if std::mem::take(&mut self.focus_filter) {
            r.request_focus();
        }
        if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.action_filter.clear();
        }
        let f = self.action_filter.trim().to_lowercase();
        let mut by_cap: std::collections::BTreeMap<String, Vec<lens_core::ActionDescriptor>> = Default::default();
        for a in self.rt.arcade.actions().actions() {
            let d = a.descriptor();
            if !d.in_palette || (!f.is_empty() && !d.label.to_lowercase().contains(&f) && !d.id.contains(&f)) {
                continue;
            }
            let cap = d.accepts.first().map(cap_label).unwrap_or_default();
            by_cap.entry(cap).or_default().push(d.clone());
        }
        let shown: usize = by_cap.values().map(Vec::len).sum();
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, format!("Filter actions, {shown} shown")));
        if by_cap.is_empty() {
            ui.add_space(16.0);
            kit.empty_state(
                ui,
                "search",
                &format!("Nothing matches \u{201c}{}\u{201d}", self.action_filter.trim()),
                "Try another word, or clear the filter.",
                |ui| {
                    if kit.button(ui, "Clear filter", ButtonKind::Default).clicked() {
                        self.action_filter.clear();
                    }
                },
            );
            return;
        }
        let mut changed = false;
        for (cap, list) in by_cap {
            kit.section_head(ui, &cap);
            kit.list(ui, |ui| {
                for d in list {
                    let safety = match d.safety() {
                        lens_core::SafetyClass::External => "Online: sends the selection to a website.",
                        lens_core::SafetyClass::Dangerous => "Asks first: runs, deletes or overwrites.",
                        _ => "",
                    };
                    kit.row(ui, &d.label, safety, |ui| {
                        let mut on = !self.settings.disabled_actions.contains(&d.id);
                        if kit.switch(ui, &mut on, &format!("Use {}", d.label)).changed() {
                            self.settings.disabled_actions.retain(|x| x != &d.id);
                            if !on {
                                self.settings.disabled_actions.push(d.id.clone());
                            }
                            changed = true;
                        }
                        let mut fav = self.settings.preferred_actions.contains(&d.id);
                        if kit.checkbox(ui, &mut fav, "Favourite").changed() {
                            self.settings.preferred_actions.retain(|x| x != &d.id);
                            if fav {
                                self.settings.preferred_actions.push(d.id.clone());
                            }
                            changed = true;
                        }
                        let key_id = format!("key:{}", d.id);
                        let current = self.settings.action_keys.get(&d.id).copied().or(d.key).map(|c| c.to_ascii_uppercase().to_string()).unwrap_or_default();
                        let field = ui.make_persistent_id(&key_id);
                        let mut value = ui.data(|m| m.get_temp::<String>(field)).unwrap_or(current);
                        let error_id = field.with("error");
                        let error = ui.data(|m| m.get_temp::<String>(error_id));
                        ui.allocate_ui(egui::vec2(44.0, size::CONTROL_SM), |ui| {
                            let r =
                                kit.text_input(ui, &mut value, "Key", None).on_hover_text(error.clone().unwrap_or_else(|| "The action's palette key".into()));
                            if r.changed() {
                                ui.data_mut(|m| m.insert_temp(field, value.clone()));
                            }
                            if r.lost_focus() {
                                match valid_action_key(&value) {
                                    Ok(key) => {
                                        ui.data_mut(|m| {
                                            m.remove::<String>(field);
                                            m.remove::<String>(error_id);
                                        });
                                        match key {
                                            Some(c) if Some(c) != d.key => {
                                                self.settings.action_keys.insert(d.id.clone(), c);
                                            }
                                            _ => {
                                                self.settings.action_keys.remove(&d.id);
                                            }
                                        }
                                        changed = true;
                                    }
                                    Err(e) => ui.data_mut(|m| m.insert_temp(error_id, e)),
                                }
                            }
                        });
                        if let Some((label, tone)) = theme::safety_badge(d.safety()) {
                            kit.badge(ui, label, tone);
                        }
                    });
                    let error_id = ui.make_persistent_id(format!("key:{}", d.id)).with("error");
                    if let Some(e) = ui.data(|m| m.get_temp::<String>(error_id)) {
                        ui.label(RichText::new(e).font(theme::font(text::SMALL)).color(theme::c().danger));
                    }
                }
            });
        }
        if changed {
            self.save(out);
        }
    }

    fn text_recognition(&mut self, ui: &mut egui::Ui, kit: &Kit, out: &mut Vec<SettingsRequest>) {
        let missing = self.rt.ocr_name == super::runtime::OCR_MISSING;
        kit.list(ui, |ui| {
            let (title, description) = if missing {
                ("Not installed", "Lens reads text with Tesseract. Without it, only image actions are offered.".to_string())
            } else {
                ("Installed", self.rt.ocr_name.clone())
            };
            kit.row(ui, "Text recognition engine", &format!("{title}. {description}"), |ui| {
                if missing {
                    match arcade_link::engines::tesseract_source() {
                        Some(source) => {
                            let label = format!("Download ({})", arcade_link::design::format::format_size(u64::from(source.size_mb) * 1_000_000));
                            if kit.button_state(ui, &label, ButtonKind::Primary, !self.tesseract_busy, self.tesseract_busy, true).clicked() {
                                out.push(SettingsRequest::DownloadTesseract);
                            }
                        }
                        None => {
                            kit.badge(ui, "Install from your package manager", Tone::Warn);
                        }
                    }
                } else {
                    kit.badge(ui, "Ready", Tone::Ok);
                }
            });
            let langs = self.settings.ocr_languages.join(", ");
            kit.row(ui, "Languages", "Tesseract language codes, separated by commas.", |ui| {
                ui.set_max_width(220.0);
                self.field(ui, "ocr_languages", langs, "eng", out, |s, v| {
                    s.ocr_languages = valid_languages(v)?;
                    Ok(())
                });
            });
        });
        kit.section_head(ui, "What Lens looks for");
        let mut changed = false;
        kit.list(ui, |ui| {
            for r in self.rt.registry.recognizers() {
                let d = r.descriptor();
                let mut on = !self.settings.disabled_recognizers.contains(&d.id);
                let produces: Vec<String> = d.produces.iter().map(cap_label).collect();
                if kit.switch_row(ui, &mut on, &produces.join(", "), &d.id).changed() {
                    self.settings.disabled_recognizers.retain(|x| x != &d.id);
                    if !on {
                        self.settings.disabled_recognizers.push(d.id.clone());
                    }
                    changed = true;
                }
            }
        });
        if changed {
            self.save(out);
        }
    }

    fn capabilities(&self) -> Vec<Capability> {
        let mut v: Vec<Capability> = vec![
            caps::REGION,
            caps::IMAGE,
            caps::TEXT,
            caps::TABLE,
            caps::CODE,
            caps::URL,
            caps::COLOR,
            caps::PALETTE,
            caps::ERROR,
            caps::COMMAND,
            caps::PATH,
            caps::QR_CODE,
        ];
        for a in self.rt.registry.actions() {
            for c in &a.descriptor().accepts {
                if *c != caps::ANY && !v.contains(c) {
                    v.push(c.clone());
                }
            }
        }
        v
    }

    /// Saves the chain drafts when every one of them is valid. While one is
    /// invalid the saved chains stay as they were; nothing else is held back.
    fn commit_chains(&mut self, out: &mut Vec<SettingsRequest>) {
        if self.chain_drafts == self.chains {
            return;
        }
        let registry = self.rt.registry.clone();
        if self.chain_drafts.iter().all(|c| chain_problem(c, &self.chain_drafts, &registry).is_none()) {
            self.chains = self.chain_drafts.clone();
            self.save(out);
        }
    }

    fn chains_page(&mut self, ui: &mut egui::Ui, kit: &Kit) {
        let registry = self.rt.registry.clone();
        if self.chain_drafts.is_empty() {
            kit.empty_state(ui, "link", "No chains yet", "A chain runs several actions in a row, such as reading text and translating it.", |_| ());
            return;
        }
        self.chain_sel = self.chain_sel.min(self.chain_drafts.len() - 1);
        kit.list(ui, |ui| {
            for i in 0..self.chain_drafts.len() {
                let chain = &self.chain_drafts[i];
                let problem = chain_problem(chain, &self.chain_drafts, &registry);
                let steps = chain.steps.len();
                let description = format!("{} step{}", steps, if steps == 1 { "" } else { "s" });
                let name = if chain.name.trim().is_empty() { "Untitled chain".to_string() } else { chain.name.clone() };
                let selected = i == self.chain_sel;
                kit.row(ui, &name, &description, |ui| {
                    if problem.is_some() {
                        kit.badge(ui, "Needs attention", Tone::Danger);
                    }
                    if !selected && kit.button_state(ui, "Edit", ButtonKind::Default, true, false, true).clicked() {
                        self.chain_sel = i;
                    }
                    if selected {
                        kit.badge(ui, "Editing", Tone::Accent);
                    }
                });
            }
        });
        let caps_list = self.capabilities();
        let sel = self.chain_sel;
        let all_names: Vec<String> = self.chain_drafts.iter().map(|c| c.name.clone()).collect();
        kit.section_head(ui, "Edit chain");
        let graph = registry.graph();
        let mut current: Option<Capability> = None;
        let mut remove: Option<usize> = None;
        let mut swap: Option<(usize, usize)> = None;
        let mut delete = false;
        {
            let chain = &mut self.chain_drafts[sel];
            kit.list(ui, |ui| {
                kit.row(ui, "Name", "", |ui| {
                    ui.set_max_width(260.0);
                    let duplicate = all_names.iter().filter(|n| n.trim() == chain.name.trim()).count() > 1;
                    let error = if chain.name.trim().is_empty() {
                        Some("Give the chain a name.")
                    } else if duplicate {
                        Some("Another chain has this name.")
                    } else {
                        None
                    };
                    kit.text_input(ui, &mut chain.name, "Chain name", error);
                });
                let len = chain.steps.len();
                for (i, step) in chain.steps.iter_mut().enumerate() {
                    let title = format!("Step {}", i + 1);
                    kit.row(ui, &title, "", |ui| {
                        if i > 0 {
                            if kit.icon_button(ui, "x", "Remove step").clicked() {
                                remove = Some(i);
                            }
                            if i + 1 < len && kit.icon_button(ui, "chevronDown", "Move step down").clicked() {
                                swap = Some((i, i + 1));
                            }
                            if i > 1 && kit.icon_button(ui, "chevronLeft", "Move step up").clicked() {
                                swap = Some((i, i - 1));
                            }
                        }
                        match step {
                            ChainStep::Take { capability } => {
                                let options: Vec<(Capability, String)> =
                                    caps_list.iter().map(|c| (c.clone(), format!("Take {}", cap_label(c).to_lowercase()))).collect();
                                let options: Vec<(Capability, &str)> = options.iter().map(|(c, l)| (c.clone(), l.as_str())).collect();
                                kit.select(ui, ("take", sel, i), capability, &options);
                                current = Some(capability.clone());
                            }
                            ChainStep::Run { action, params } => {
                                let compatible: Vec<(String, String)> = registry
                                    .actions()
                                    .iter()
                                    .map(|a| a.descriptor())
                                    .filter(|d| current.as_ref().is_some_and(|c| d.accepts.iter().any(|acc| graph.is_a(c, acc))) || d.id == *action)
                                    .map(|d| (d.id.clone(), d.label.clone()))
                                    .collect();
                                let options: Vec<(String, &str)> = compatible.iter().map(|(id, l)| (id.clone(), l.as_str())).collect();
                                let before = action.clone();
                                kit.select(ui, ("run", sel, i), action, &options);
                                if *action != before {
                                    params.clear();
                                }
                                if let Some(a) = registry.action(action) {
                                    let d = a.descriptor();
                                    for p in &d.params {
                                        let mut v = params.get(&p.name).map(|v| v.to_string().trim_matches('"').to_string()).unwrap_or_default();
                                        ui.allocate_ui(egui::vec2(120.0, size::CONTROL_SM), |ui| {
                                            if kit.text_input(ui, &mut v, &p.name, None).on_hover_text(&p.description).changed() {
                                                if v.is_empty() {
                                                    params.remove(&p.name);
                                                } else {
                                                    params.insert(p.name.clone(), serde_json::from_str(&v).unwrap_or(serde_json::Value::String(v)));
                                                }
                                            }
                                        });
                                    }
                                    current = match &d.produces {
                                        Produces::Nothing => None,
                                        Produces::Same => current.clone(),
                                        Produces::Capability(c) => Some(c.clone()),
                                    };
                                }
                            }
                        }
                    });
                }
            });
            if let Some(i) = remove {
                chain.steps.remove(i);
            }
            if let Some((a, b)) = swap {
                chain.steps.swap(a, b);
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if current.is_some() && kit.button(ui, "Add step", ButtonKind::Default).clicked() {
                    let first = registry
                        .actions()
                        .iter()
                        .map(|a| a.descriptor())
                        .find(|d| current.as_ref().is_some_and(|c| d.accepts.iter().any(|acc| graph.is_a(c, acc))));
                    if let Some(d) = first {
                        chain.steps.push(ChainStep::Run { action: d.id.clone(), params: Default::default() });
                    }
                }
                delete = kit.button(ui, "Delete chain", ButtonKind::Danger).clicked();
            });
        }
        if delete {
            self.confirm = Some(Confirm::DeleteChain(sel));
        }
        ui.add_space(10.0);
        let chain = &self.chain_drafts[sel];
        match chain_problem(chain, &self.chain_drafts, &registry) {
            None => {
                if let Ok(plan) = chain.validate(&registry) {
                    let effects = effects_text(plan.effects);
                    let how = if plan.needs_confirmation { "Asks before running" } else { "Runs on this computer" };
                    let mut message = format!("Ready. Starts from {} · {how}.", cap_label(&plan.input).to_lowercase());
                    if !effects.is_empty() {
                        message.push_str(&format!(" May {effects}."));
                    }
                    kit.notice(ui, &message, Tone::Ok);
                }
            }
            Some(problem) => {
                kit.notice(ui, &format!("{problem} Your other chains and settings are saved as usual."), Tone::Danger);
            }
        }
    }

    fn services(&mut self, ui: &mut egui::Ui, kit: &Kit, out: &mut Vec<SettingsRequest>) {
        kit.notice(ui, "Nothing is contacted unless you choose an action marked online. {query}, {lat} and {lon} are filled in.", Tone::Neutral);
        ui.add_space(16.0);
        kit.list(ui, |ui| {
            for (title, key, needs) in SERVICES {
                let p = &self.settings.providers;
                let current = match key {
                    "web_search" => p.web_search.clone(),
                    "maps_search" => p.maps_search.clone(),
                    "maps_coordinates" => p.maps_coordinates.clone(),
                    "product_search" => p.product_search.clone(),
                    _ => p.whois.clone(),
                };
                kit.row(ui, title, "", |ui| {
                    ui.set_max_width(360.0);
                    self.field(ui, key, current, "https://", out, |s, v| {
                        let v = valid_service(v, needs)?;
                        let p = &mut s.providers;
                        match key {
                            "web_search" => p.web_search = v,
                            "maps_search" => p.maps_search = v,
                            "maps_coordinates" => p.maps_coordinates = v,
                            "product_search" => p.product_search = v,
                            _ => p.whois = v,
                        }
                        Ok(())
                    });
                });
            }
        });
        kit.section_head(ui, "Reverse image search");
        let mut changed = false;
        kit.list(ui, |ui| {
            kit.row(ui, "Service", "Lens copies the selection and opens the service, where you paste it. Lens never uploads pixels itself.", |ui| {
                let cur = self.settings.providers.reverse_image_search.clone();
                let mut options: Vec<(Option<String>, String)> = vec![(None, "Off".into())];
                options.extend(REVERSE_IMAGE_PRESETS.iter().map(|(n, u)| (Some(u.to_string()), n.to_string())));
                // A custom service from an older settings file stays selectable.
                if let Some(url) = cur.as_ref().filter(|u| !REVERSE_IMAGE_PRESETS.iter().any(|(_, p)| p == u)) {
                    options.push((Some(url.clone()), format!("Custom: {url}")));
                }
                let options: Vec<(Option<String>, &str)> = options.iter().map(|(v, l)| (v.clone(), l.as_str())).collect();
                changed |= kit.select(ui, "reverse-image", &mut self.settings.providers.reverse_image_search, &options).changed();
            });
        });
        if changed {
            self.save(out);
        }
    }

    fn plugins(&mut self, ui: &mut egui::Ui, kit: &Kit, out: &mut Vec<SettingsRequest>) {
        let found = lens_plugins::discover(&self.rt.paths.plugins());
        if found.is_empty() {
            kit.empty_state(ui, "cube", "No plugins installed", "Put a plugin's folder in the plugins folder, then turn it on here.", |ui| {
                if kit.button(ui, "Open plugins folder", ButtonKind::Default).clicked() {
                    out.push(SettingsRequest::OpenFolder(self.rt.paths.plugins()));
                }
            });
            return;
        }
        let mut changed = false;
        kit.list(ui, |ui| {
            for (dir, m) in &found {
                match m {
                    Ok(m) => {
                        let permissions = if m.plugin.permissions.is_empty() { "no permissions".to_string() } else { m.plugin.permissions.join(", ") };
                        let description = format!(
                            "{}{}Version {} · {} recognizer(s), {} action(s) · Can use {permissions}",
                            m.plugin.description,
                            if m.plugin.description.is_empty() { "" } else { " · " },
                            m.plugin.version,
                            m.recognizers.len(),
                            m.actions.len()
                        );
                        let mut on = self.settings.enabled_plugins.contains(&m.plugin.id);
                        if kit.switch_row(ui, &mut on, &m.plugin.name, &description).changed() {
                            self.settings.enabled_plugins.retain(|x| x != &m.plugin.id);
                            if on {
                                self.settings.enabled_plugins.push(m.plugin.id.clone());
                            }
                            changed = true;
                        }
                        if let Some(e) = self.rt.plugins.iter().find(|s| s.id == m.plugin.id).and_then(|s| s.error.clone()) {
                            kit.notice(ui, &format!("Couldn't load {}. {e}", m.plugin.name), Tone::Danger);
                        }
                    }
                    Err(e) => {
                        let name = dir.file_name().map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into_owned());
                        kit.row(ui, &name, &format!("Couldn't read this plugin. {e}"), |ui| {
                            kit.badge(ui, "Not loaded", Tone::Danger);
                        });
                    }
                }
            }
        });
        if changed {
            self.save(out);
        }
        ui.add_space(12.0);
        if kit.button(ui, "Open plugins folder", ButtonKind::Default).clicked() {
            out.push(SettingsRequest::OpenFolder(self.rt.paths.plugins()));
        }
    }

    fn privacy(&mut self, ui: &mut egui::Ui, kit: &Kit, out: &mut Vec<SettingsRequest>) {
        let mut changed = false;
        kit.list(ui, |ui| {
            changed |= kit
                .switch_row(
                    ui,
                    &mut self.settings.privacy.guard_secrets,
                    "Protect secrets",
                    "Hide online actions when a selection contains passwords, keys or tokens.",
                )
                .changed();
            changed |= kit
                .switch_row(ui, &mut self.settings.privacy.learn_action_usage, "Learn which actions I use", "Local counters only, never what you selected.")
                .changed();
            kit.row(ui, "Learned preferences", "Start ranking actions from scratch.", |ui| {
                if kit.button_state(ui, "Forget", ButtonKind::Default, true, false, true).clicked() {
                    out.push(SettingsRequest::ResetUsage);
                    self.toast = Some(Toast::new("Learned preferences forgotten", Tone::Ok));
                }
            });
        });
        kit.section_head(ui, "History");
        let count = super::history::count(&self.rt.paths.history());
        kit.list(ui, |ui| {
            changed |= kit
                .switch_row(
                    ui,
                    &mut self.settings.privacy.history_enabled,
                    "Keep a history",
                    "Thumbnails and what was recognized, with secrets masked. Off by default.",
                )
                .changed();
            let items = format!("{} item{}", arcade_link::design::format::format_count(count as i64), if count == 1 { "" } else { "s" });
            kit.row(ui, &items, &format!("Stored on this computer in {}", self.rt.paths.history().display()), |ui| {
                if kit.button_state(ui, "Clear history", ButtonKind::Danger, count > 0, false, true).clicked() {
                    self.confirm = Some(Confirm::ClearHistory);
                }
                if kit.button_state(ui, "Open folder", ButtonKind::Default, true, false, true).clicked() {
                    out.push(SettingsRequest::OpenFolder(self.rt.paths.history()));
                }
                if kit.button_state(ui, "Show history", ButtonKind::Default, true, false, true).clicked() {
                    out.push(SettingsRequest::ShowHistory);
                }
            });
        });
        if changed {
            self.save(out);
        }
    }

    fn connected_apps(&mut self, ui: &mut egui::Ui, kit: &Kit, out: &mut Vec<SettingsRequest>) {
        use arcade_link::client::AppState;
        let mut changed = false;
        if !self.settings.link.enabled {
            kit.notice_action(ui, "Connections are off in this app", Tone::Warn, |ui| {
                if kit.button_state(ui, "Turn on", ButtonKind::Default, true, false, true).clicked() {
                    self.settings.link.enabled = true;
                    changed = true;
                }
            });
            ui.add_space(16.0);
        }
        kit.list(ui, |ui| {
            changed |= kit
                .switch_row(
                    ui,
                    &mut self.settings.link.enabled,
                    "Connect with other Arcade apps",
                    "Their actions appear in the palette, and they can use Lens to capture.",
                )
                .changed();
        });
        kit.section_head(ui, "Apps");
        let snapshot = self.rt.arcade.snapshot();
        kit.list(ui, |ui| {
            for peer in &snapshot.peers {
                let name = arcade_link::manifest::app_name(&peer.id).to_string();
                let glyph = super::glyphs::texture(ui.ctx(), &peer.id, 18.0);
                let (status, tone, installed, running) = match &peer.state {
                    AppState::Running { .. } if peer.link_enabled => ("Connected", Tone::Ok, true, true),
                    AppState::Running { .. } => ("Not connected", Tone::Neutral, true, true),
                    AppState::Installed { .. } => ("Not running", Tone::Neutral, true, false),
                    AppState::NotInstalled => ("Not installed", Tone::Neutral, false, false),
                };
                kit.row_with_glyph(ui, glyph, "apps", &name, peer_description(&peer.id), |ui| {
                    if installed {
                        let mut on = !self.settings.link.disabled_peers.contains(&peer.id);
                        ui.add_enabled_ui(self.settings.link.enabled, |ui| {
                            if kit.switch(ui, &mut on, &format!("Use {name} with Arcade Lens")).changed() {
                                self.settings.link.disabled_peers.retain(|id| id != &peer.id);
                                if !on {
                                    self.settings.link.disabled_peers.push(peer.id.clone());
                                }
                                changed = true;
                            }
                        });
                        if !running && kit.button_state(ui, "Open", ButtonKind::Default, true, false, true).clicked() {
                            out.push(SettingsRequest::OpenApp(peer.id.clone()));
                        }
                    } else if kit.button_state(ui, "Install", ButtonKind::Default, true, false, true).clicked() {
                        out.push(SettingsRequest::GetApp(peer.id.clone()));
                    }
                    kit.badge(ui, status, tone);
                });
            }
        });
        if changed {
            self.save(out);
        }
        ui.add_space(16.0);
        kit.disclosure(ui, "activity", "Recent activity", |ui| {
            let activity = crate::link::recent_activity();
            if activity.is_empty() {
                ui.label(RichText::new("Nothing yet. Actions Lens sends to other apps appear here.").font(theme::font(text::SMALL)).color(theme::c().muted));
            }
            for (when, line) in activity.iter().rev().take(20) {
                ui.label(
                    RichText::new(format!("{}  {line}", arcade_link::design::format::format_relative(*when, now_ms(), local_offset_minutes())))
                        .font(theme::font(text::SMALL)),
                );
            }
        });
        kit.disclosure(ui, "link-diagnostics", "Diagnostics", |ui| {
            ui.label(RichText::new(format!("Registry: {}", self.rt.arcade.locations().registry.display())).font(theme::mono()));
            ui.label(RichText::new(format!("Last error: {}", snapshot.last_error.as_deref().unwrap_or("none"))).font(theme::font(text::SMALL)));
        });
        ui.add_space(8.0);
        if kit.button(ui, "How it works", ButtonKind::Ghost).clicked() {
            out.push(SettingsRequest::OpenUrl(LINK_GUIDE_URL.into()));
        }
    }

    fn about(&mut self, ui: &mut egui::Ui, kit: &Kit, out: &mut Vec<SettingsRequest>) {
        let channel = crate::app::installer::channel();
        let brand = super::glyphs::texture(ui.ctx(), crate::link::ME, 18.0);
        let snapshot = self.rt.arcade.snapshot();
        let tools = snapshot.installed.get(arcade_link::ids::TOOLS).is_some();
        let mut copy: Option<String> = None;
        kit.list(ui, |ui| {
            kit.row_with_glyph(ui, brand, "search", &format!("Version {}", crate::link::VERSION), TAGLINE, |ui| {
                kit.badge(ui, channel_label(&channel), Tone::Neutral);
            });
            if tools {
                kit.row(ui, "Updates", "Updates are managed by Arcade Tools.", |ui| {
                    if kit.button_state(ui, "Open Tools", ButtonKind::Default, true, false, true).clicked() {
                        out.push(SettingsRequest::OpenApp(arcade_link::ids::TOOLS.into()));
                    }
                });
            } else {
                kit.row(ui, "Updates", "Install Arcade Tools to get updates automatically.", |ui| {
                    if kit.button_state(ui, "Check for updates", ButtonKind::Default, true, false, true).clicked() {
                        out.push(SettingsRequest::OpenUrl(arcade_link::manifest::releases_url(arcade_link::ids::LENS).into()));
                    }
                });
            }
            kit.row(ui, "User guide", "How to select, act, chain and annotate.", |ui| {
                if kit.button_state(ui, "Open user guide", ButtonKind::Default, true, false, true).clicked() {
                    out.push(SettingsRequest::OpenUrl(GUIDE_URL.into()));
                }
            });
            let logs = self.rt.paths.data.join("logs");
            kit.row(ui, "Logs", &logs.display().to_string(), |ui| {
                if kit.button_state(ui, "Open logs folder", ButtonKind::Default, true, false, true).clicked() {
                    out.push(SettingsRequest::OpenFolder(logs.clone()));
                }
            });
            kit.row(ui, "Diagnostics", "App, version, desktop and Link details for a bug report. No selections.", |ui| {
                if kit.button_state(ui, "Copy diagnostics", ButtonKind::Default, true, false, true).clicked() {
                    copy = Some(diagnostics(&self.rt, &channel));
                }
            });
            kit.row(ui, "Licences", "MIT OR Apache-2.0. Third-party licences ship with the app.", |ui| {
                if kit.button_state(ui, "Open", ButtonKind::Default, true, false, true).clicked() {
                    out.push(SettingsRequest::OpenUrl(LICENCES_URL.into()));
                }
            });
            kit.row(ui, "Quit Arcade Lens", "Stops Lens and its shortcut.", |ui| {
                if kit.button_state(ui, "Quit", ButtonKind::Default, true, false, true).clicked() {
                    out.push(SettingsRequest::Quit);
                }
            });
        });
        if let Some(text) = copy {
            ui.ctx().copy_text(text);
            self.toast = Some(Toast::copied());
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context, kit: &Kit, out: &mut Vec<SettingsRequest>) {
        let Some(confirm) = self.confirm else { return };
        let (title, body, verb) = match confirm {
            Confirm::DeleteChain(i) => {
                let name = self.chain_drafts.get(i).map_or_else(String::new, |c| c.name.clone());
                (format!("Delete chain \u{201c}{name}\u{201d}?"), "Its steps are removed. Your actions and other chains stay.".to_string(), "Delete chain")
            }
            Confirm::ClearHistory => (
                "Clear history?".to_string(),
                "Every saved thumbnail and recognized item is removed from this computer. Your settings stay.".to_string(),
                "Clear history",
            ),
        };
        let mut decision: Option<bool> = None;
        let modal = egui::Modal::new(egui::Id::new("lens-settings-confirm")).show(ctx, |ui| {
            ui.set_width(380.0);
            kit.dialog_head(ui, &title, &body);
            kit.dialog_actions(ui, |ui| {
                if kit.button(ui, "Keep", ButtonKind::Default).clicked() {
                    decision = Some(false);
                }
                if kit.button(ui, verb, ButtonKind::Destructive).clicked() {
                    decision = Some(true);
                }
            });
        });
        if modal.should_close() && decision.is_none() {
            decision = Some(false);
        }
        match decision {
            Some(true) => {
                self.confirm = None;
                match confirm {
                    Confirm::DeleteChain(i) => {
                        if i < self.chain_drafts.len() {
                            self.chain_drafts.remove(i);
                            self.chain_sel = 0;
                        }
                    }
                    Confirm::ClearHistory => out.push(SettingsRequest::ClearHistory),
                }
            }
            Some(false) => self.confirm = None,
            None => {}
        }
    }
}

fn channel_label(channel: &str) -> &'static str {
    match channel {
        "nightly" | "beta" => "Beta",
        "development" => "Development build",
        _ => "Stable",
    }
}

/// "Copy diagnostics": Link's shared header, then Lens's lines. No
/// selections, settings payloads or keys.
pub fn diagnostics(rt: &Runtime, channel: &str) -> String {
    let mut header = arcade_link::diagnostics::Header::new(crate::link::ME, crate::link::VERSION, channel);
    header.peers_from(rt.arcade.locations());
    header.engine("tesseract", if rt.ocr_name == super::runtime::OCR_MISSING { "not installed" } else { rt.ocr_name.as_str() });
    let mut out = header.render();
    out.push_str(&format!("Display: {:?}\n", lens_platform::display_server()));
    out.push_str(&format!("Recognizers: {}, actions: {}\n", rt.registry.recognizers().len(), rt.registry.actions().len()));
    out.push_str(&format!("Plugins enabled: {}\n", rt.settings.enabled_plugins.len()));
    out
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn local_offset_minutes() -> i32 {
    chrono::Local::now().offset().local_minus_utc() / 60
}

/// A shortcut-sheet group: its title and (action, bindings) rows.
pub type KeyGroup = (String, Vec<(String, Vec<String>)>);

/// The in-capture keys, from the shortcut sheet Tools shows too
/// (`docs/user/shortcuts.json`), grouped as there.
pub fn palette_keys() -> Vec<KeyGroup> {
    let doc: serde_json::Value = serde_json::from_str(include_str!("../../../../docs/user/shortcuts.json")).unwrap_or(serde_json::Value::Null);
    let platform = if cfg!(target_os = "macos") { "macos" } else { "default" };
    let mut out = Vec::new();
    for group in doc["groups"].as_array().into_iter().flatten() {
        if group["context"] == "global" || group["context"] == "settings-shortcut" {
            continue;
        }
        let mut rows: Vec<(String, Vec<String>)> = Vec::new();
        for s in group["shortcuts"].as_array().into_iter().flatten() {
            let keys = &s["keys"];
            let binding = keys.get(platform).unwrap_or(&keys["default"]);
            let combos: Vec<String> = match binding {
                serde_json::Value::String(k) => vec![k.clone()],
                serde_json::Value::Array(list) => list.iter().filter_map(|k| k.as_str().map(str::to_string)).collect(),
                _ => Vec::new(),
            };
            let Some(title) = s["title"].as_str() else { continue };
            if combos.is_empty() {
                continue;
            }
            match rows.iter_mut().find(|(t, _)| t == title) {
                Some((_, existing)) => existing.extend(combos),
                None => rows.push((title.to_string(), combos)),
            }
        }
        out.push((group["title"].as_str().unwrap_or("").to_string(), rows));
    }
    out
}

/// Key names as the key chips show them.
fn key_label(key: &str) -> String {
    match key {
        "Escape" => "Esc".into(),
        "Enter" => "↵".into(),
        "Left" => "←".into(),
        "Right" => "→".into(),
        "Up" => "↑".into(),
        "Down" => "↓".into(),
        "Super" if cfg!(target_os = "macos") => "⌘".into(),
        "PageDown" => "Page Down".into(),
        "PageUp" => "Page Up".into(),
        other => other.to_string(),
    }
}

/// What Lens and a peer do together (Connected apps).
pub fn peer_description(app: &str) -> &'static str {
    arcade_link::manifest::peer_description(crate::link::ME, app)
}

#[cfg(test)]
mod tests {

    #[test]
    fn section_labels_round_trip_over_ipc() {
        for section in Section::ALL {
            assert_eq!(Section::from_label(section.label()), Some(section));
        }
        assert_eq!(Section::from_label(" text recognition "), Some(Section::TextRecognition));
        assert_eq!(Section::from_label("Nope"), None);
    }

    use super::*;

    #[test]
    fn sections_follow_the_standard_order() {
        let labels: Vec<_> = Section::ALL.iter().map(|s| s.label()).collect();
        assert_eq!(labels.first(), Some(&"General"));
        assert_eq!(labels[1], "Shortcuts");
        assert_eq!(&labels[labels.len() - 2..], ["Connected apps", "About"]);
        assert!(labels.contains(&"Text recognition") && !labels.contains(&"Recognizers"));
    }

    #[test]
    fn service_templates_are_checked() {
        assert!(valid_service("https://duckduckgo.com/?q={query}", &["{query}"]).is_ok());
        assert!(valid_service("duckduckgo.com/?q={query}", &["{query}"]).is_err());
        assert!(valid_service("https://example.com/", &["{query}"]).unwrap_err().contains("{query}"));
        assert!(valid_service("https://osm.org/?a={lat}", &["{lat}", "{lon}"]).unwrap_err().contains("{lon}"));
        assert!(valid_service("https://a b/{query}", &["{query}"]).is_err());
    }

    #[test]
    fn small_fields_are_checked() {
        assert_eq!(valid_currency(" eur "), Ok(Some("EUR".into())));
        assert_eq!(valid_currency(""), Ok(None));
        assert!(valid_currency("euro").is_err());
        assert_eq!(valid_languages("eng, deu"), Ok(vec!["eng".to_string(), "deu".to_string()]));
        assert!(valid_languages(" , ").is_err());
        assert!(valid_languages("eng; deu").is_err());
        assert_eq!(valid_action_key("Q"), Ok(Some('q')));
        assert_eq!(valid_action_key(""), Ok(None));
        assert!(valid_action_key("qq").is_err());
        assert!(valid_action_key("?").is_err());
    }

    #[test]
    fn palette_keys_come_from_the_shortcut_sheet() {
        let groups = palette_keys();
        assert!(groups.iter().all(|(g, _)| g != "Global"), "the global shortcut has its own row");
        let selection = groups.iter().find(|(g, _)| g == "Screen selection").expect("selection keys");
        let all = selection.1.iter().find(|(t, _)| t == "Show all actions").expect("all actions");
        assert_eq!(all.1, ["Space", "Tab"], "equivalent keys share one row");
    }

    #[test]
    fn diagnostics_have_the_shared_header() {
        let rt = Runtime::load(crate::config::Paths { config: std::env::temp_dir().join("lens-diag-c"), data: std::env::temp_dir().join("lens-diag-d") });
        if let Ok(rt) = rt {
            let d = diagnostics(&rt, "stable");
            assert!(d.contains(crate::link::VERSION));
            assert!(d.contains("Recognizers:"));
        }
    }
}
