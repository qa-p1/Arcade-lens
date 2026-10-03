//! The configuration window. Edits a draft; nothing applies until Save.

use std::sync::Arc;

use eframe::egui::{self, Color32, RichText, ViewportBuilder, ViewportId};
use lens_core::capability::Capability;
use lens_core::chain::{Chain, ChainStep};
use lens_core::settings::{DateOrder, ImageFormat};
use lens_core::{caps, Produces, Settings};

use super::overlay::{cap_label, effects_text};
use super::runtime::Runtime;
use super::theme::{ACCENT, ACCENT_SOFT, DANGER, MUTED, SUCCESS, TEXT};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    General,
    Shortcut,
    Actions,
    Recognizers,
    Chains,
    Providers,
    Plugins,
    Privacy,
    About,
}

impl Section {
    const ALL: [Section; 9] = [
        Section::General,
        Section::Shortcut,
        Section::Actions,
        Section::Recognizers,
        Section::Chains,
        Section::Providers,
        Section::Plugins,
        Section::Privacy,
        Section::About,
    ];
    fn label(&self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Shortcut => "Shortcut",
            Section::Actions => "Actions",
            Section::Recognizers => "Recognizers",
            Section::Chains => "Chains",
            Section::Providers => "Services",
            Section::Plugins => "Plugins",
            Section::Privacy => "Privacy",
            Section::About => "About",
        }
    }
}

pub enum SettingsRequest {
    Save { settings: Box<Settings>, chains: Vec<Chain> },
    DownloadModels,
    ResetUsage,
    ClearHistory,
    Autostart(bool),
    InstallLauncher,
    OpenFolder(std::path::PathBuf),
}

pub struct SettingsView {
    rt: Arc<Runtime>,
    draft: Settings,
    chains: Vec<Chain>,
    section: Section,
    recording_shortcut: bool,
    chain_sel: usize,
    action_filter: String,
    pub status: Option<(String, bool)>,
    pub closed: bool,
    pub autostart: bool,
    pub models_busy: bool,
}

const REVERSE_IMAGE_PRESETS: &[(&str, &str)] = &[
    ("Google Lens", "https://lens.google.com/"),
    ("Bing Visual Search", "https://www.bing.com/visualsearch"),
    ("Yandex Images", "https://yandex.com/images/"),
    ("TinEye", "https://tineye.com/"),
];

fn key_to_string(key: egui::Key, m: egui::Modifiers) -> Option<String> {
    use egui::Key::*;
    if matches!(key, Escape) {
        return None;
    }
    let mut parts = Vec::new();
    if m.ctrl {
        parts.push("Ctrl");
    }
    if m.alt {
        parts.push("Alt");
    }
    if m.shift {
        parts.push("Shift");
    }
    if m.mac_cmd {
        parts.push("Super");
    }
    let name = key.name();
    if parts.is_empty() && !matches!(key, F1 | F2 | F3 | F4 | F5 | F6 | F7 | F8 | F9 | F10 | F11 | F12) {
        return None; // A bare letter would hijack normal typing.
    }
    Some(format!("{}+{name}", parts.join("+")).trim_start_matches('+').to_string())
}

impl SettingsView {
    pub fn new(rt: Arc<Runtime>) -> SettingsView {
        SettingsView {
            draft: (*rt.settings).clone(),
            chains: rt.chains.clone(),
            rt,
            section: Section::General,
            recording_shortcut: false,
            chain_sel: 0,
            action_filter: String::new(),
            status: None,
            closed: false,
            autostart: lens_platform::autostart::is_enabled(),
            models_busy: false,
        }
    }

    pub fn viewport_id() -> ViewportId {
        ViewportId::from_hash_of("lens-settings")
    }

    pub fn builder() -> ViewportBuilder {
        ViewportBuilder::default()
            .with_title("Arcade Lens Settings")
            .with_inner_size([920.0, 640.0])
            .with_min_inner_size([720.0, 480.0])
            .with_app_id("arcade-lens-settings")
    }

    pub fn refresh_runtime(&mut self, rt: Arc<Runtime>) {
        self.rt = rt;
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Vec<SettingsRequest> {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) {
            self.closed = true;
        }
        let mut out = Vec::new();
        egui::Panel::left("settings-nav").exact_size(170.0).frame(egui::Frame::new().fill(Color32::from_rgb(18, 18, 20)).inner_margin(12)).show(ui, |ui| {
            ui.label(RichText::new("Arcade Lens").color(TEXT).size(16.0).strong());
            ui.label(RichText::new("Select anything. Do anything useful with it.").color(MUTED).size(11.0));
            ui.add_space(12.0);
            for s in Section::ALL {
                let selected = self.section == s;
                let b = egui::Button::new(RichText::new(s.label()).size(13.5).color(if selected { Color32::WHITE } else { TEXT }))
                    .fill(if selected { ACCENT.gamma_multiply(0.55) } else { Color32::TRANSPARENT })
                    .min_size(egui::vec2(146.0, 28.0));
                if ui.add(b).clicked() {
                    self.section = s;
                }
            }
        });
        egui::Panel::bottom("settings-actions").frame(egui::Frame::new().fill(Color32::from_rgb(24, 24, 27)).inner_margin(10)).show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some((msg, ok)) = &self.status {
                    ui.label(RichText::new(msg).color(if *ok { SUCCESS } else { DANGER }));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let dirty = self.draft != *self.rt.settings || self.chains != self.rt.chains;
                    let save = egui::Button::new(RichText::new("Save").color(if dirty { Color32::WHITE } else { MUTED })).fill(if dirty {
                        ACCENT.gamma_multiply(0.85)
                    } else {
                        super::theme::SURFACE_HI
                    });
                    if ui.add_enabled(dirty, save).clicked() {
                        out.push(SettingsRequest::Save { settings: Box::new(self.draft.clone()), chains: self.chains.clone() });
                    }
                    if ui.add_enabled(dirty, egui::Button::new("Revert")).clicked() {
                        self.draft = (*self.rt.settings).clone();
                        self.chains = self.rt.chains.clone();
                        self.status = None;
                    }
                });
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(24, 24, 27)).inner_margin(16)).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.section {
                Section::General => self.general(ui, &mut out),
                Section::Shortcut => self.shortcut(ui, &ctx),
                Section::Actions => self.actions(ui),
                Section::Recognizers => self.recognizers(ui),
                Section::Chains => self.chains_ui(ui),
                Section::Providers => self.providers(ui),
                Section::Plugins => self.plugins(ui, &mut out),
                Section::Privacy => self.privacy(ui, &mut out),
                Section::About => self.about(ui),
            });
        });
        out
    }

    fn title(ui: &mut egui::Ui, t: &str, sub: &str) {
        ui.label(RichText::new(t).size(20.0).color(TEXT).strong());
        ui.label(RichText::new(sub).color(MUTED));
        ui.add_space(12.0);
    }

    fn opt_text(ui: &mut egui::Ui, label: &str, value: &mut Option<String>, hint: &str) {
        ui.horizontal(|ui| {
            ui.add_sized([150.0, 20.0], egui::Label::new(label));
            let mut s = value.clone().unwrap_or_default();
            if ui.add(egui::TextEdit::singleline(&mut s).hint_text(hint).desired_width(380.0)).changed() {
                *value = (!s.trim().is_empty()).then(|| s.trim().to_string());
            }
        });
    }

    fn general(&mut self, ui: &mut egui::Ui, out: &mut Vec<SettingsRequest>) {
        Self::title(ui, "General", "How Lens saves, opens and reads things.");
        let mut dir = self.draft.screenshot_dir.as_ref().map(|p| p.display().to_string());
        Self::opt_text(ui, "Save folder", &mut dir, "Pictures/Arcade Lens");
        self.draft.screenshot_dir = dir.map(Into::into);
        ui.horizontal(|ui| {
            ui.add_sized([150.0, 20.0], egui::Label::new("Image format"));
            for (f, l) in [(ImageFormat::Png, "PNG"), (ImageFormat::Jpeg, "JPEG"), (ImageFormat::Webp, "WebP")] {
                ui.radio_value(&mut self.draft.image_format, f, l);
            }
        });
        Self::opt_text(ui, "Browser", &mut self.draft.browser, "system default (e.g. firefox)");
        Self::opt_text(ui, "Terminal", &mut self.draft.terminal, "auto-detect (e.g. kitty, wezterm)");
        Self::opt_text(ui, "Editor", &mut self.draft.editor, "system default (e.g. code)");
        ui.horizontal(|ui| {
            ui.add_sized([150.0, 20.0], egui::Label::new("Numeric dates"));
            ui.radio_value(&mut self.draft.date_order, DateOrder::DayFirst, "Day first (05/03 = 5 March)");
            ui.radio_value(&mut self.draft.date_order, DateOrder::MonthFirst, "Month first (05/03 = May 3)");
        });
        ui.label(RichText::new("Ambiguous dates are always confirmed; this only sets which reading is listed first.").color(MUTED).size(11.5));
        Self::opt_text(ui, "Home currency", &mut self.draft.home_currency, "ISO code, e.g. INR, EUR");
        ui.horizontal(|ui| {
            ui.add_sized([150.0, 20.0], egui::Label::new("Primary actions"));
            ui.add(egui::Slider::new(&mut self.draft.primary_action_count, 3..=8));
        });
        ui.horizontal(|ui| {
            ui.add_sized([150.0, 20.0], egui::Label::new("OCR languages"));
            let mut s = self.draft.ocr_languages.join(", ");
            if ui.add(egui::TextEdit::singleline(&mut s).desired_width(200.0)).changed() {
                self.draft.ocr_languages = s.split(',').map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
            }
        });
        ui.add_space(12.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.add_sized([150.0, 20.0], egui::Label::new("Text recognition"));
            ui.label(RichText::new(&self.rt.ocr_name).color(ACCENT_SOFT));
            if self.rt.ocr_name.contains("not installed")
                && ui.add_enabled(!self.models_busy, egui::Button::new(if self.models_busy { "Downloading…" } else { "Download OCR models (12 MB)" })).clicked()
            {
                out.push(SettingsRequest::DownloadModels);
            }
        });
        ui.horizontal(|ui| {
            ui.add_sized([150.0, 20.0], egui::Label::new("Start at login"));
            let mut a = self.autostart;
            if ui.checkbox(&mut a, "Run Arcade Lens in the background when you log in").changed() {
                out.push(SettingsRequest::Autostart(a));
            }
        });
        if cfg!(target_os = "linux") {
            ui.horizontal(|ui| {
                ui.add_sized([150.0, 20.0], egui::Label::new("App launcher"));
                if ui.button("Add Arcade Lens to the applications menu").clicked() {
                    out.push(SettingsRequest::InstallLauncher);
                }
            });
        }
    }

    fn shortcut(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        Self::title(ui, "Activation shortcut", "Works from any application. Lens never takes a common system shortcut without asking.");
        let enabled = self.draft.activation_shortcut.is_some();
        let mut on = enabled;
        if ui.checkbox(&mut on, "Enable the global shortcut").changed() {
            self.draft.activation_shortcut = on.then(|| Settings::default().activation_shortcut.unwrap());
        }
        if let Some(sc) = self.draft.activation_shortcut.clone() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&sc).size(18.0).monospace().color(ACCENT_SOFT));
                let label = if self.recording_shortcut { "Press keys… (Esc to cancel)" } else { "Change…" };
                if ui.button(label).clicked() {
                    self.recording_shortcut = !self.recording_shortcut;
                }
            });
            if self.recording_shortcut {
                let events = ctx.input(|i| i.events.clone());
                for e in events {
                    if let egui::Event::Key { key, pressed: true, modifiers, .. } = e {
                        if key == egui::Key::Escape {
                            self.recording_shortcut = false;
                        } else if let Some(s) = key_to_string(key, modifiers) {
                            if lens_platform::shortcut::parse(&s).is_ok() {
                                self.draft.activation_shortcut = Some(s);
                                self.recording_shortcut = false;
                            }
                        }
                    }
                }
            }
            match lens_platform::shortcut::known_conflict(&sc) {
                Some(what) => ui.label(RichText::new(format!("⚠  Usually used by {what}. Pick another unless you're sure.")).color(DANGER)),
                None => ui.label(RichText::new("No known conflicts. Saving checks whether another app holds it.").color(MUTED)),
            };
        }
        ui.add_space(12.0);
        if lens_platform::display_server() == lens_platform::DisplayServer::Wayland {
            ui.label(
                RichText::new("Wayland does not let applications register global shortcuts. Bind a shortcut in your desktop's keyboard settings to:")
                    .color(MUTED),
            );
            let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "arcade-lens".into());
            ui.label(RichText::new(format!("{exe} capture")).monospace().color(TEXT));
        }
        ui.add_space(12.0);
        ui.label(RichText::new("In the palette").strong());
        egui::Grid::new("keys").striped(true).show(ui, |ui| {
            for (k, d) in [
                ("Enter", "default action"),
                ("Letter", "action with that key"),
                ("Space / Tab", "all actions"),
                ("Arrows", "nudge selection (Shift: resize)"),
                ("Ctrl+C", "copy screenshot"),
                ("M", "measure mode"),
                ("Esc", "back / close"),
            ] {
                ui.label(RichText::new(k).monospace());
                ui.label(d);
                ui.end_row();
            }
        });
    }

    fn actions(&mut self, ui: &mut egui::Ui) {
        Self::title(ui, "Actions", "Turn actions off, mark favorites (always ranked high), or change their key.");
        ui.add(egui::TextEdit::singleline(&mut self.action_filter).hint_text("Filter…").desired_width(260.0));
        ui.add_space(6.0);
        let f = self.action_filter.to_lowercase();
        let mut by_cap: std::collections::BTreeMap<String, Vec<lens_core::ActionDescriptor>> = Default::default();
        for a in self.rt.registry.actions() {
            let d = a.descriptor();
            if !d.in_palette || (!f.is_empty() && !d.label.to_lowercase().contains(&f) && !d.id.contains(&f)) {
                continue;
            }
            let cap = d.accepts.first().map(cap_label).unwrap_or_default();
            by_cap.entry(cap).or_default().push(d.clone());
        }
        for (cap, list) in by_cap {
            egui::CollapsingHeader::new(RichText::new(format!("{cap}  ({})", list.len())).strong()).default_open(!f.is_empty()).show(ui, |ui| {
                egui::Grid::new(format!("acts-{cap}")).num_columns(4).spacing([14.0, 4.0]).show(ui, |ui| {
                    for d in list {
                        let mut on = !self.draft.disabled_actions.contains(&d.id);
                        if ui.checkbox(&mut on, &d.label).changed() {
                            if on {
                                self.draft.disabled_actions.retain(|x| x != &d.id);
                            } else {
                                self.draft.disabled_actions.push(d.id.clone());
                            }
                        }
                        let mut fav = self.draft.preferred_actions.contains(&d.id);
                        if ui.checkbox(&mut fav, "favorite").changed() {
                            if fav {
                                self.draft.preferred_actions.push(d.id.clone());
                            } else {
                                self.draft.preferred_actions.retain(|x| x != &d.id);
                            }
                        }
                        let mut key = self.draft.action_keys.get(&d.id).map(|c| c.to_string()).or(d.key.map(|c| c.to_string())).unwrap_or_default();
                        if ui.add(egui::TextEdit::singleline(&mut key).desired_width(28.0).char_limit(1)).changed() {
                            match key.chars().next() {
                                Some(c) if Some(c) != d.key => {
                                    self.draft.action_keys.insert(d.id.clone(), c.to_ascii_lowercase());
                                }
                                _ => {
                                    self.draft.action_keys.remove(&d.id);
                                }
                            }
                        }
                        let tag = match d.safety() {
                            lens_core::SafetyClass::External => RichText::new("online").color(super::theme::EXTERNAL),
                            lens_core::SafetyClass::Dangerous => RichText::new("asks first").color(DANGER),
                            _ => RichText::new("local").color(MUTED),
                        };
                        ui.label(tag.size(11.0));
                        ui.end_row();
                    }
                });
            });
        }
    }

    fn recognizers(&mut self, ui: &mut egui::Ui) {
        Self::title(ui, "Recognizers", "What Lens looks for. Everything runs locally.");
        for r in self.rt.registry.recognizers() {
            let d = r.descriptor();
            let mut on = !self.draft.disabled_recognizers.contains(&d.id);
            let produces: Vec<String> = d.produces.iter().map(cap_label).collect();
            if ui.checkbox(&mut on, RichText::new(produces.join(", ")).strong()).on_hover_text(&d.id).changed() {
                if on {
                    self.draft.disabled_recognizers.retain(|x| x != &d.id);
                } else {
                    self.draft.disabled_recognizers.push(d.id.clone());
                }
            }
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

    fn chains_ui(&mut self, ui: &mut egui::Ui) {
        Self::title(ui, "Chains", "Reusable sequences of actions. Steps are type-checked as you build them.");
        let registry = self.rt.registry.clone();
        ui.horizontal(|ui| {
            for (i, c) in self.chains.iter().enumerate() {
                if ui.selectable_label(self.chain_sel == i, &c.name).clicked() {
                    self.chain_sel = i;
                }
            }
            if ui.button("+ New chain").clicked() {
                let n = self.chains.len() + 1;
                self.chains.push(Chain { id: format!("chain-{n}"), name: format!("My chain {n}"), steps: vec![ChainStep::Take { capability: caps::TEXT }] });
                self.chain_sel = self.chains.len() - 1;
            }
        });
        ui.separator();
        let caps_list = self.capabilities();
        let Some(chain) = self.chains.get_mut(self.chain_sel) else {
            ui.label(RichText::new("No chains yet.").color(MUTED));
            return;
        };
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut chain.name);
        });
        ui.add_space(8.0);
        // Walk the steps, tracking the current type to offer only compatible actions.
        let graph = registry.graph();
        let mut current: Option<Capability> = None;
        let mut remove: Option<usize> = None;
        let mut swap: Option<(usize, usize)> = None;
        let len = chain.steps.len();
        for (i, step) in chain.steps.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}.", i + 1)).color(MUTED));
                match step {
                    ChainStep::Take { capability } => {
                        ui.label("Take");
                        egui::ComboBox::from_id_salt(("take", i)).selected_text(cap_label(capability)).show_ui(ui, |ui| {
                            for c in &caps_list {
                                ui.selectable_value(capability, c.clone(), cap_label(c));
                            }
                        });
                        current = Some(capability.clone());
                    }
                    ChainStep::Run { action, params } => {
                        let label = registry.action(action).map_or(action.clone(), |a| a.descriptor().label.clone());
                        egui::ComboBox::from_id_salt(("run", i)).width(260.0).selected_text(label).show_ui(ui, |ui| {
                            for a in registry.actions() {
                                let d = a.descriptor();
                                let ok = current.as_ref().is_some_and(|c| d.accepts.iter().any(|acc| graph.is_a(c, acc)));
                                if ok
                                    && ui
                                        .selectable_label(*action == d.id, format!("{}  ({})", d.label, d.accepts.first().map(cap_label).unwrap_or_default()))
                                        .clicked()
                                {
                                    *action = d.id.clone();
                                    params.clear();
                                }
                            }
                        });
                        if let Some(a) = registry.action(action) {
                            let d = a.descriptor();
                            for p in &d.params {
                                let mut v = params.get(&p.name).map(|v| v.to_string().trim_matches('"').to_string()).unwrap_or_default();
                                ui.label(RichText::new(&p.name).color(MUTED));
                                if ui
                                    .add(egui::TextEdit::singleline(&mut v).hint_text(p.default.to_string()).desired_width(110.0))
                                    .on_hover_text(&p.description)
                                    .changed()
                                {
                                    if v.is_empty() {
                                        params.remove(&p.name);
                                    } else {
                                        params.insert(p.name.clone(), serde_json::from_str(&v).unwrap_or(serde_json::Value::String(v)));
                                    }
                                }
                            }
                            current = match &d.produces {
                                Produces::Nothing => None,
                                Produces::Same => current.clone(),
                                Produces::Capability(c) => Some(c.clone()),
                            };
                        }
                    }
                }
                if i > 0 {
                    if ui.small_button("↑").clicked() && i > 1 {
                        swap = Some((i, i - 1));
                    }
                    if ui.small_button("↓").clicked() && i + 1 < len {
                        swap = Some((i, i + 1));
                    }
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                }
            });
        }
        if let Some(i) = remove {
            chain.steps.remove(i);
        }
        if let Some((a, b)) = swap {
            chain.steps.swap(a, b);
        }
        let mut delete = false;
        ui.horizontal(|ui| {
            if current.is_some() && ui.button("+ Add step").clicked() {
                let first = registry
                    .actions()
                    .iter()
                    .map(|a| a.descriptor())
                    .find(|d| current.as_ref().is_some_and(|c| d.accepts.iter().any(|acc| graph.is_a(c, acc))));
                if let Some(d) = first {
                    chain.steps.push(ChainStep::Run { action: d.id.clone(), params: Default::default() });
                }
            }
            if ui.button(RichText::new("Delete chain").color(DANGER)).clicked() {
                delete = true;
            }
        });
        if delete {
            self.chains.remove(self.chain_sel);
            self.chain_sel = 0;
            return;
        }
        if let Some(chain) = self.chains.get(self.chain_sel) {
            ui.add_space(8.0);
            match chain.validate(&registry) {
                Ok(plan) => {
                    let effects = effects_text(plan.effects);
                    ui.label(
                        RichText::new(format!(
                            "✓ Valid · starts from {} · {}",
                            cap_label(&plan.input),
                            if plan.needs_confirmation { "asks before running" } else { "runs locally" }
                        ))
                        .color(SUCCESS),
                    );
                    if !effects.is_empty() {
                        ui.label(RichText::new(format!("May {effects}.")).color(MUTED));
                    }
                }
                Err(e) => {
                    ui.label(RichText::new(format!("Not valid: {e}")).color(DANGER));
                }
            }
        }
    }

    fn providers(&mut self, ui: &mut egui::Ui) {
        Self::title(
            ui,
            "Services",
            "Actions that need the internet use these. {query}, {lat} and {lon} are filled in. Nothing is contacted unless you choose such an action.",
        );
        let p = &mut self.draft.providers;
        for (label, v) in [
            ("Web search", &mut p.web_search),
            ("Maps (search)", &mut p.maps_search),
            ("Maps (coordinates)", &mut p.maps_coordinates),
            ("Product search", &mut p.product_search),
            ("WHOIS", &mut p.whois),
        ] {
            ui.horizontal(|ui| {
                ui.add_sized([150.0, 20.0], egui::Label::new(label));
                ui.add(egui::TextEdit::singleline(v).desired_width(460.0));
            });
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_sized([150.0, 20.0], egui::Label::new("Reverse image search"));
            let cur = p.reverse_image_search.clone();
            let name = REVERSE_IMAGE_PRESETS
                .iter()
                .find(|(_, u)| Some(*u) == cur.as_deref())
                .map_or_else(|| cur.clone().unwrap_or_else(|| "Off".into()), |(n, _)| n.to_string());
            egui::ComboBox::from_id_salt("ris").selected_text(name).show_ui(ui, |ui| {
                if ui.selectable_label(cur.is_none(), "Off").clicked() {
                    p.reverse_image_search = None;
                }
                for (n, u) in REVERSE_IMAGE_PRESETS {
                    if ui.selectable_label(cur.as_deref() == Some(u), *n).clicked() {
                        p.reverse_image_search = Some(u.to_string());
                    }
                }
            });
        });
        ui.label(
            RichText::new(
                "Reverse image search copies the selection to your clipboard and opens the service, where you paste it. Lens never uploads pixels by itself.",
            )
            .color(MUTED)
            .size(11.5),
        );
    }

    fn plugins(&mut self, ui: &mut egui::Ui, out: &mut Vec<SettingsRequest>) {
        Self::title(ui, "Plugins", "Add recognizers and actions. Plugins are off until you enable them, and can only do what their manifest declares.");
        let found = lens_plugins::discover(&self.rt.paths.plugins());
        if found.is_empty() {
            ui.label(RichText::new("No plugins installed.").color(MUTED));
        }
        for (dir, m) in found {
            match m {
                Ok(m) => {
                    egui::Frame::new().stroke(egui::Stroke::new(1.0, super::theme::BORDER)).corner_radius(8).inner_margin(10).show(ui, |ui| {
                        let mut on = self.draft.enabled_plugins.contains(&m.plugin.id);
                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut on, RichText::new(&m.plugin.name).strong()).changed() {
                                if on {
                                    self.draft.enabled_plugins.push(m.plugin.id.clone());
                                } else {
                                    self.draft.enabled_plugins.retain(|x| x != &m.plugin.id);
                                }
                            }
                            ui.label(RichText::new(format!("{} · v{}", m.plugin.id, m.plugin.version)).color(MUTED));
                        });
                        if !m.plugin.description.is_empty() {
                            ui.label(&m.plugin.description);
                        }
                        ui.label(
                            RichText::new(format!(
                                "Permissions: {}",
                                if m.plugin.permissions.is_empty() { "none".into() } else { m.plugin.permissions.join(", ") }
                            ))
                            .color(ACCENT_SOFT)
                            .size(11.5),
                        );
                        ui.label(RichText::new(format!("{} recognizer(s), {} action(s)", m.recognizers.len(), m.actions.len())).color(MUTED).size(11.5));
                        if let Some(s) = self.rt.plugins.iter().find(|s| s.id == m.plugin.id) {
                            if let Some(e) = &s.error {
                                ui.label(RichText::new(e).color(DANGER));
                            }
                        }
                    });
                }
                Err(e) => {
                    ui.label(RichText::new(format!("{}: {e}", dir.display())).color(DANGER));
                }
            }
            ui.add_space(6.0);
        }
        ui.add_space(8.0);
        if ui.button("Open plugins folder").clicked() {
            out.push(SettingsRequest::OpenFolder(self.rt.paths.plugins()));
        }
        ui.label(RichText::new("Arcade apps (Clipboard, Quick Look, Wheel) integrate through this same plugin interface.").color(MUTED).size(11.5));
    }

    fn privacy(&mut self, ui: &mut egui::Ui, out: &mut Vec<SettingsRequest>) {
        Self::title(ui, "Privacy", "Selections are analyzed on this computer and forgotten when Lens closes.");
        ui.checkbox(&mut self.draft.privacy.guard_secrets, "Hide online actions for selections that contain passwords, keys or tokens");
        ui.checkbox(&mut self.draft.privacy.learn_action_usage, "Learn which actions I use (local counters only, no content)");
        ui.horizontal(|ui| {
            if ui.button("Forget learned preferences").clicked() {
                out.push(SettingsRequest::ResetUsage);
            }
        });
        ui.add_space(10.0);
        ui.checkbox(&mut self.draft.privacy.history_enabled, "Keep a history of selections (thumbnails and what was recognized)");
        ui.label(RichText::new("History is off by default and stays on this computer.").color(MUTED).size(11.5));
        let count = super::history::count(&self.rt.paths.history());
        ui.horizontal(|ui| {
            ui.label(format!("{count} item(s) in history"));
            if count > 0 && ui.button("Clear history").clicked() {
                out.push(SettingsRequest::ClearHistory);
            }
            if ui.button("Open folder").clicked() {
                out.push(SettingsRequest::OpenFolder(self.rt.paths.history()));
            }
        });
    }

    fn about(&mut self, ui: &mut egui::Ui) {
        Self::title(ui, "Arcade Lens", &format!("Version {}", env!("CARGO_PKG_VERSION")));
        egui::Grid::new("about").show(ui, |ui| {
            for (k, v) in [
                ("Display", format!("{:?}", lens_platform::display_server())),
                ("Text recognition", self.rt.ocr_name.clone()),
                ("Recognizers", self.rt.registry.recognizers().len().to_string()),
                ("Actions", self.rt.registry.actions().len().to_string()),
                ("Settings", self.rt.paths.settings().display().to_string()),
            ] {
                ui.label(RichText::new(k).color(MUTED));
                ui.label(v);
                ui.end_row();
            }
        });
    }
}
