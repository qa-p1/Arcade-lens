//! Palette surfaces attached to the selection: the compact action row, the
//! expanded list, and confirmation / choice / result panels.

use std::time::Duration;

use eframe::egui::{self, Align2, Color32, Key, Modifiers, Pos2, RichText, Sense, Stroke, StrokeKind, Vec2};
use lens_core::action::Params;
use lens_core::palette::{PaletteEntry, Target};
use lens_core::{FindingId, SafetyClass, Value};

use super::overlay::{cap_label, describe, Env, Overlay, Panel};
use super::theme;

const GAP: f32 = 10.0;

/// Places a panel of `size` next to `sel`: below if it fits, otherwise
/// above, otherwise inside the selection's bottom edge. Never off-screen.
pub fn place(screen: egui::Rect, sel: egui::Rect, size: Vec2) -> Pos2 {
    let x = sel.min.x.clamp(screen.min.x + 8.0, (screen.max.x - size.x - 8.0).max(screen.min.x + 8.0));
    let y = if sel.max.y + GAP + size.y <= screen.max.y - 8.0 {
        sel.max.y + GAP
    } else if sel.min.y - GAP - size.y >= screen.min.y + 8.0 {
        sel.min.y - GAP - size.y
    } else {
        (sel.max.y - size.y - GAP).max(screen.min.y + 8.0)
    };
    Pos2::new(x, y)
}

/// Places a tall side panel to the right (or left) of the selection.
fn place_side(screen: egui::Rect, sel: egui::Rect, size: Vec2) -> Pos2 {
    let x = if sel.max.x + GAP + size.x <= screen.max.x - 8.0 {
        sel.max.x + GAP
    } else if sel.min.x - GAP - size.x >= screen.min.x + 8.0 {
        sel.min.x - GAP - size.x
    } else {
        (screen.max.x - size.x - 8.0).max(screen.min.x + 8.0)
    };
    let y = sel.min.y.clamp(screen.min.y + 8.0, (screen.max.y - size.y - 8.0).max(screen.min.y + 8.0));
    Pos2::new(x, y)
}

pub fn show(ov: &mut Overlay, ctx: &egui::Context, env: &Env, screen: egui::Rect, sel: egui::Rect) {
    match ov.panel {
        Panel::Primary => primary(ov, ctx, env, screen, sel),
        Panel::Expanded { .. } => expanded(ov, ctx, env, screen, sel),
        Panel::Confirm { .. } => confirm(ov, ctx, env, screen, sel),
        Panel::Choice { .. } => choice(ov, ctx, env, screen, sel),
        Panel::Result { .. } => result(ov, ctx, env, screen, sel),
    }
}

/// Text sizes, inks and widths the palette surfaces share.
mod ui_kit {
    pub use arcade_link::design::egui::components::{ButtonKind, Tone};
    pub use arcade_link::design::{radius, size, text};
}
use ui_kit::*;

/// The palette header: the recognized kind as a badge, then a short summary
/// and up to two details.
fn header(ui: &mut egui::Ui, ov: &Overlay) {
    let c = theme::c();
    let kit = theme::kit();
    ui.horizontal(|ui| {
        ui.set_min_height(size::KEY_CHIP + 4.0);
        ui.add_space(2.0);
        if let Some(f) = ov.headline() {
            if let Value::Color(col) = &f.value {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                // The recognized colour itself, as a swatch.
                ui.painter().rect(r, radius::XS, Color32::from_rgb(col.r, col.g, col.b), Stroke::new(1.0_f32, c.line_strong), StrokeKind::Inside);
            }
            let (title, summary) = describe(f);
            kit.badge(ui, &title, Tone::Accent);
            let details: Vec<String> = f
                .details
                .iter()
                .filter(|(k, _)| !matches!(k.as_str(), "Engine" | "Lines" | "Size" | "Status" | "Possible formats" | "Search" | "Value"))
                .take(2)
                .map(|(k, v)| {
                    if k == "=" {
                        format!("= {v}")
                    } else if matches!(k.as_str(), "HEX" | "RGB" | "HSL" | "Kind" | "Type") {
                        v.clone()
                    } else {
                        format!("{k} {v}")
                    }
                })
                .collect();
            let budget = 64usize;
            let mut summary_text = summary;
            if !matches!(f.value, Value::Color(_)) {
                if summary_text.chars().count() > 40 {
                    summary_text = summary_text.chars().take(39).collect::<String>() + "…";
                }
                ui.label(RichText::new(&summary_text).font(theme::font(text::BODY)).color(c.text));
            }
            let mut d = details.join("  ·  ");
            let room = budget.saturating_sub(summary_text.chars().count());
            if d.chars().count() > room {
                d = d.chars().take(room.saturating_sub(1)).collect::<String>() + "…";
            }
            if !d.is_empty() && room > 8 {
                ui.label(RichText::new(d).font(theme::font(text::SMALL)).color(c.muted));
            }
        }
        if let Some((label, t)) = ov.job_running() {
            if t > Duration::from_millis(120) {
                ui.add(egui::Spinner::new().size(12.0).color(c.accent));
                ui.label(RichText::new(format!("{label}…")).font(theme::font(text::SMALL)).color(c.muted));
            }
        } else if ov.analyzing() {
            ui.add(egui::Spinner::new().size(12.0).color(c.muted));
        }
    });
}

/// The footer hint bar every palette surface ends with.
fn footer(ui: &mut egui::Ui, keys: &[(&str, &str)]) {
    let c = theme::c();
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), size::HINT_BAR), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0_f32, c.line));
    let p = ui.painter();
    let caption = theme::font(text::CAPTION);
    let mut x = rect.left() + 4.0;
    let y = rect.center().y + 2.0;
    for (key, label) in keys {
        x += theme::key_chip(p, Pos2::new(x, y), key) + 6.0;
        let g = p.layout_no_wrap((*label).to_string(), caption.clone(), c.muted);
        let w = g.size().x;
        p.galley(Pos2::new(x, y - g.size().y / 2.0), g, c.muted);
        x += w + 14.0;
    }
}

fn primary(ov: &mut Overlay, ctx: &egui::Context, env: &Env, screen: egui::Rect, sel: egui::Rect) {
    let pos = place(screen, sel, ov.palette_size());
    let mut clicked: Option<PaletteEntry> = None;
    let mut more = false;
    let mut install_ocr = false;
    let resp = egui::Area::new(egui::Id::new("lens-palette")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        theme::panel_frame().show(ui, |ui| {
            header(ui, ov);
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, e) in ov.palette.primary.iter().enumerate() {
                    let key = e.key.map(|k| k.to_ascii_uppercase().to_string());
                    let peer = e.icon.starts_with("arcade.").then_some(e.icon.as_str());
                    let response = theme::action_chip(
                        ui,
                        &e.label,
                        key.as_deref(),
                        e.safety,
                        theme::ChipState { is_default: i == ov.palette.default_index, focused: false },
                        peer,
                    );
                    let response = if let Some(preview) = &e.preview { response.on_hover_text(preview) } else { response };
                    if response.clicked() {
                        clicked = Some(e.clone());
                    }
                }
                if ov.palette.all.len() > ov.palette.primary.len() {
                    let r = theme::chip(ui, "More", Some("Tab"), SafetyClass::Pure, theme::ChipState { is_default: false, focused: false });
                    if r.clicked() {
                        more = true;
                    }
                }
            });
            if ov.palette.primary.is_empty() {
                // Analyzing: the kit skeleton row stands in for the actions.
                theme::kit().skeleton_row(ui);
            }
            // Without an OCR engine no text is ever found; say why, and offer
            // the download instead of offering only image actions.
            let no_text = !ov.report.findings.iter().any(|f| f.capability == lens_core::caps::TEXT);
            if no_text && !ov.analyzing() && env.rt.ocr_name == super::runtime::OCR_MISSING {
                ui.add_space(4.0);
                let kit = theme::kit();
                kit.notice_action(ui, "Text isn't recognized: text recognition isn't installed.", Tone::Warn, |ui| {
                    if kit.button_state(ui, "Install text recognition", ButtonKind::Default, true, false, true).clicked() {
                        install_ocr = true;
                    }
                });
            }
            footer(ui, &[("↵", "Run"), ("Tab", "More actions"), ("Esc", "Close")]);
        });
    });
    *ov.palette_size_mut() = resp.response.rect.size();
    if more {
        ov.panel = Panel::Expanded { filter: String::new(), cursor: 0, peers: false };
    }
    if install_ocr {
        ov.open_settings = Some(super::settings_view::Section::TextRecognition);
        ov.close_requested = true;
    }
    if let Some(e) = clicked {
        ov.run_entry(env, e, Params::new(), false);
    }
}

fn matches_filter(e: &PaletteEntry, f: &str) -> bool {
    if f.is_empty() {
        return true;
    }
    let f = f.to_lowercase();
    e.label.to_lowercase().contains(&f) || cap_label(&e.capability).to_lowercase().contains(&f)
}

/// Group label in the expanded list: sentence case `caption`, muted.
fn group_label(ui: &mut egui::Ui, title: &str, summary: Option<&str>) {
    let c = theme::c();
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(title).font(theme::font(text::CAPTION)).color(c.muted));
        if let Some(summary) = summary {
            ui.add(egui::Label::new(RichText::new(summary).font(theme::font(text::CAPTION)).color(c.muted)).truncate());
        }
    });
    ui.add_space(2.0);
}

fn expanded(ov: &mut Overlay, ctx: &egui::Context, env: &Env, screen: egui::Rect, sel: egui::Rect) {
    let Panel::Expanded { filter, cursor, peers } = &mut ov.panel else { return };
    let peers_only = *peers;
    // Navigation keys are consumed before the text field sees them.
    let (down, up, enter, pgdn, pgup) = ctx.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowDown) || i.consume_key(Modifiers::NONE, Key::Tab),
            i.consume_key(Modifiers::NONE, Key::ArrowUp) || i.consume_key(Modifiers::SHIFT, Key::Tab),
            i.consume_key(Modifiers::NONE, Key::Enter),
            i.consume_key(Modifiers::NONE, Key::PageDown),
            i.consume_key(Modifiers::NONE, Key::PageUp),
        )
    });
    // Group by finding (in order of each finding's best action), best actions first within a group.
    let mut visible: Vec<PaletteEntry> =
        ov.palette.all.iter().filter(|e| matches_filter(e, filter) && (!peers_only || e.icon.starts_with("arcade."))).cloned().collect();
    let mut order: Vec<FindingId> = Vec::new();
    for e in &visible {
        if !order.contains(&e.finding) {
            order.push(e.finding);
        }
    }
    let chain_rank = |e: &PaletteEntry| matches!(e.target, Target::Chain(_));
    visible.sort_by_key(|e| (chain_rank(e), order.iter().position(|f| *f == e.finding).unwrap_or(usize::MAX)));
    let n = visible.len();
    if n > 0 {
        if down {
            *cursor = (*cursor + 1) % n;
        }
        if up {
            *cursor = (*cursor + n - 1) % n;
        }
        if pgdn {
            *cursor = (*cursor + 8).min(n - 1);
        }
        if pgup {
            *cursor = cursor.saturating_sub(8);
        }
        *cursor = (*cursor).min(n - 1);
    }
    let cur = *cursor;
    let size = Vec2::new(440.0, (screen.height() * 0.7).min(580.0));
    let pos = place_side(screen, sel, size);
    let mut run: Option<PaletteEntry> = None;
    let mut new_filter = filter.clone();
    let query = filter.clone();
    // Bring the selected row into view only when the selection or the list
    // changes (or the panel was just opened), so the mouse wheel can scroll.
    let follow_id = egui::Id::new("lens-expanded-follow");
    let pass = ctx.cumulative_pass_nr();
    let shown = (cur, visible.len());
    let follow = ctx.data(|d| d.get_temp::<((usize, usize), u64)>(follow_id)).is_none_or(|(last, at)| last != shown || at + 1 < pass);
    ctx.data_mut(|d| d.insert_temp(follow_id, (shown, pass)));
    let c = theme::c();
    egui::Area::new(egui::Id::new("lens-expanded")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        theme::panel_frame().show(ui, |ui| {
            ui.set_width(size.x - 16.0);
            // The filter field: 44 px with the kit search icon.
            let (field, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), size::OVERLAY_SEARCH), Sense::hover());
            theme::paint_icon(ui.painter(), "search", Pos2::new(field.left() + 18.0, field.center().y), 16.0, c.muted);
            let te = egui::TextEdit::singleline(&mut new_filter)
                .hint_text("Filter actions")
                .font(theme::font(text::BODY))
                .frame(false)
                .desired_width(field.width() - 44.0)
                .vertical_align(egui::Align::Center);
            let r = ui.put(egui::Rect::from_min_max(Pos2::new(field.left() + 36.0, field.top()), Pos2::new(field.right() - 8.0, field.bottom())), te);
            if !r.has_focus() {
                r.request_focus();
            }
            ui.painter().hline(field.x_range(), field.bottom(), Stroke::new(1.0_f32, c.line));
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(size.y - size::OVERLAY_SEARCH - size::HINT_BAR - 24.0).auto_shrink([false, true]).show(ui, |ui| {
                let mut last_finding: Option<FindingId> = None;
                let mut chains_labelled = false;
                for (i, e) in visible.iter().enumerate() {
                    if let Target::Chain(_) = e.target {
                        if !chains_labelled {
                            chains_labelled = true;
                            group_label(ui, "Chains", None);
                        }
                    } else if last_finding != Some(e.finding) {
                        last_finding = Some(e.finding);
                        if let Some(f) = ov.report.findings.iter().find(|f| f.id == e.finding) {
                            let (title, summary) = describe(f);
                            let short: String = summary.chars().take(48).collect();
                            group_label(ui, &title, Some(&short));
                        }
                    }
                    let selected = i == cur;
                    let disabled = e.disabled_reason.is_some();
                    let two_lines = e.preview.is_some() || disabled;
                    let row = ui.allocate_response(
                        Vec2::new(ui.available_width(), if two_lines { size::ROW_OVERLAY_2 } else { size::ROW_OVERLAY }),
                        if disabled { Sense::hover() } else { Sense::click() },
                    );
                    let p = ui.painter();
                    if selected {
                        p.rect_filled(row.rect, radius::SM, c.accent_soft);
                        p.rect_filled(egui::Rect::from_min_size(row.rect.min + Vec2::new(0.0, 8.0), Vec2::new(2.0, row.rect.height() - 16.0)), 1.0, c.accent);
                    } else if row.hovered() {
                        p.rect_filled(row.rect, radius::SM, c.hover);
                    }
                    let y = if two_lines { row.rect.min.y + 17.0 } else { row.rect.center().y };
                    let mut x = row.rect.min.x + 12.0;
                    // Leading glyph: the peer app's glyph, else the action's kind.
                    let peer = e.icon.starts_with("arcade.");
                    if peer {
                        super::glyphs::paint(p, Pos2::new(x + 8.0, y), &e.icon, c.muted);
                    } else {
                        theme::paint_icon(p, action_icon(e), Pos2::new(x + 8.0, y), 16.0, c.muted);
                    }
                    x += 26.0;
                    let label_colour = if disabled { c.muted } else { c.text };
                    // Trailing: safety badge, then the key chip on the selected row.
                    let badge_w = if peer && e.safety == SafetyClass::External { 0.0 } else { theme::safety_badge_width(p, e.safety) };
                    let enter_w = if selected && !disabled { theme::key_chip_width(p, "↵") + 8.0 } else { 0.0 };
                    let width = row.rect.max.x - x - badge_w - enter_w - 16.0;
                    let mut layout = egui::text::LayoutJob::simple(e.label.clone(), theme::font_weight(text::BODY, 500), label_colour, width);
                    layout.wrap.max_rows = 1;
                    let g = p.layout_job(layout);
                    p.galley(Pos2::new(x, y - g.size().y / 2.0), g, label_colour);
                    let mut right = row.rect.max.x - 8.0;
                    if enter_w > 0.0 {
                        right -= enter_w - 8.0;
                        theme::key_chip(p, Pos2::new(right, row.rect.center().y), "↵");
                        right -= 8.0;
                    }
                    if badge_w > 0.0 {
                        theme::paint_safety(p, Pos2::new(right - badge_w, row.rect.center().y), e.safety);
                    }
                    let second = row.rect.min.y + 35.0;
                    if let Some(reason) = &e.disabled_reason {
                        let mut layout = egui::text::LayoutJob::simple(reason.clone(), theme::font(text::SMALL), c.muted, width);
                        layout.wrap.max_rows = 1;
                        let g = p.layout_job(layout);
                        p.galley(Pos2::new(x, second - g.size().y / 2.0), g, c.muted);
                    } else if let Some(prev) = &e.preview {
                        let short: String = prev.chars().take(60).collect();
                        let line = if peer { short } else { format!("Sends: {short}") };
                        let mut layout = egui::text::LayoutJob::simple(line, theme::font(text::SMALL), c.muted, width);
                        layout.wrap.max_rows = 1;
                        let g = p.layout_job(layout);
                        p.galley(Pos2::new(x, second - g.size().y / 2.0), g, c.muted);
                    }
                    row.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, !disabled, selected, &e.label));
                    if selected && follow {
                        row.scroll_to_me(None);
                    }
                    if row.clicked() && !disabled {
                        run = Some(e.clone());
                    }
                }
                if visible.is_empty() && peers_only && query.trim().is_empty() {
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("No connected app takes this").font(theme::font(text::HEADING)).color(c.text));
                        ui.label(RichText::new("Install or start an Arcade app that works with text.").font(theme::font(text::SMALL)).color(c.muted));
                    });
                    ui.add_space(12.0);
                } else if visible.is_empty() {
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| {
                        let title = if query.trim().is_empty() {
                            "No matching actions".to_string()
                        } else {
                            format!("No actions match \u{201c}{}\u{201d}", query.trim())
                        };
                        ui.label(RichText::new(title).font(theme::font(text::HEADING)).color(c.text));
                        ui.label(RichText::new("Try a shorter word, or clear the filter.").font(theme::font(text::SMALL)).color(c.muted));
                    });
                    ui.add_space(12.0);
                }
            });
            footer(ui, &[("↵", "Run"), ("↑↓", "Choose"), ("Esc", "Back")]);
        });
    });
    if let Panel::Expanded { filter, cursor, .. } = &mut ov.panel {
        if *filter != new_filter {
            *filter = new_filter;
            *cursor = 0;
        }
    }
    if enter {
        run = visible.get(cur).filter(|e| e.disabled_reason.is_none()).cloned();
    }
    if let Some(e) = run {
        ov.panel = Panel::Primary;
        ov.run_entry(env, e, Params::new(), false);
    }
}

/// A kit icon for an action row that has no peer glyph.
fn action_icon(e: &PaletteEntry) -> &'static str {
    if let Target::Chain(_) = e.target {
        return "link";
    }
    let label = e.label.to_lowercase();
    let pick = |words: &[&str]| words.iter().any(|w| label.contains(w));
    if pick(&["copy"]) {
        "copy"
    } else if pick(&["save", "export", "download"]) {
        "download"
    } else if pick(&["search", "look up", "find"]) {
        "search"
    } else if pick(&["open", "browser", "map"]) {
        "external"
    } else if pick(&["pin"]) {
        "pin"
    } else if pick(&["annotate", "edit", "draw"]) {
        "edit"
    } else if pick(&["run", "terminal", "execute"]) {
        "code"
    } else if pick(&["translate", "text", "ocr"]) {
        "type"
    } else if pick(&["qr", "barcode"]) {
        "qr"
    } else if pick(&["color", "colour", "palette"]) {
        "sparkle"
    } else if pick(&["measure", "ruler"]) {
        "fit"
    } else if pick(&["record", "video"]) {
        "video"
    } else {
        match e.safety {
            SafetyClass::External => "globe",
            SafetyClass::Dangerous => "alert",
            _ => "arrowRight",
        }
    }
}

fn confirm(ov: &mut Overlay, ctx: &egui::Context, env: &Env, screen: egui::Rect, sel: egui::Rect) {
    let Panel::Confirm { entry, req, .. } = &ov.panel else { return };
    let dangerous = entry.safety == SafetyClass::Dangerous;
    let (title, subject, reasons, verb) = (req.title.clone(), req.subject.clone(), req.reasons.clone(), entry.label.clone());
    let size = Vec2::new(460.0, 240.0);
    let pos = place(screen, sel, size);
    let mut go = false;
    let mut cancel = false;
    let c = theme::c();
    let kit = theme::kit();
    egui::Area::new(egui::Id::new("lens-confirm")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        theme::panel_frame().inner_margin(egui::Margin::same(16)).show(ui, |ui| {
            ui.set_max_width(size.x);
            ui.label(RichText::new(&title).font(theme::font(text::TITLE_SM)).color(c.text));
            ui.add_space(8.0);
            egui::Frame::new().fill(c.sunken).stroke(Stroke::new(1.0_f32, c.line)).corner_radius(radius::SM).inner_margin(8).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(110.0).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(&subject).font(theme::mono()).color(c.text)).wrap().selectable(true));
                });
            });
            ui.add_space(8.0);
            // The consequences, as a list.
            for r in &reasons {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("•").font(theme::font(text::BODY)).color(if dangerous { c.danger } else { c.muted }));
                    ui.label(RichText::new(r).font(theme::font(text::BODY)).color(c.text));
                });
            }
            kit.dialog_actions(ui, |ui| {
                // Right-to-left: the action (last) is added first.
                let action = if dangerous { ButtonKind::Destructive } else { ButtonKind::Primary };
                if kit.button(ui, &verb, action).on_hover_text(if dangerous { "Ctrl+Enter" } else { "Enter" }).clicked() {
                    go = true;
                }
                if kit.button(ui, "Cancel", ButtonKind::Default).on_hover_text("Esc").clicked() {
                    cancel = true;
                }
            });
            footer(ui, &[(if dangerous { "Ctrl+↵" } else { "↵" }, &verb), ("Esc", "Cancel")]);
        });
    });
    if cancel {
        ov.panel = Panel::Primary;
    } else if go {
        ov.confirm(env);
    }
}

fn choice(ov: &mut Overlay, ctx: &egui::Context, env: &Env, screen: egui::Rect, sel: egui::Rect) {
    let Panel::Choice { entry, choices, cursor, .. } = &ov.panel else { return };
    let (label, items, cur) = (entry.label.clone(), choices.iter().map(|c| c.label.clone()).collect::<Vec<_>>(), *cursor);
    let size = Vec2::new(380.0, 40.0 + size::ROW_OVERLAY * items.len().min(9) as f32);
    let pos = place(screen, sel, size);
    let mut picked = None;
    let c = theme::c();
    egui::Area::new(egui::Id::new("lens-choice")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        theme::panel_frame().show(ui, |ui| {
            ui.set_width(size.x - 16.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.label(RichText::new(format!("{label}: which one?")).font(theme::font(text::HEADING)).color(c.text));
            });
            ui.add_space(4.0);
            // List rows with radio semantics.
            for (i, it) in items.iter().enumerate() {
                let row = ui.allocate_response(Vec2::new(ui.available_width(), size::ROW_OVERLAY), Sense::click());
                let p = ui.painter();
                let selected = i == cur;
                if selected {
                    p.rect_filled(row.rect, radius::SM, c.accent_soft);
                    p.rect_filled(egui::Rect::from_min_size(row.rect.min + Vec2::new(0.0, 8.0), Vec2::new(2.0, row.rect.height() - 16.0)), 1.0, c.accent);
                } else if row.hovered() {
                    p.rect_filled(row.rect, radius::SM, c.hover);
                }
                let dot = Pos2::new(row.rect.min.x + 18.0, row.rect.center().y);
                p.circle_stroke(dot, 7.0, Stroke::new(1.5_f32, if selected { c.accent } else { c.line_strong }));
                if selected {
                    p.circle_filled(dot, 3.5, c.accent);
                }
                p.text(Pos2::new(row.rect.min.x + 36.0, row.rect.center().y), Align2::LEFT_CENTER, it, theme::font_weight(text::BODY, 500), c.text);
                if i < 9 {
                    let key = (i + 1).to_string();
                    let w = theme::key_chip_width(p, &key);
                    theme::key_chip(p, Pos2::new(row.rect.max.x - 8.0 - w, row.rect.center().y), &key);
                }
                row.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, it));
                if row.clicked() {
                    picked = Some(i);
                }
            }
            footer(ui, &[("↵", "Choose"), ("1–9", "Pick"), ("Esc", "Back")]);
        });
    });
    if let Some(i) = picked {
        ov.pick_choice(env, i);
    }
}

fn result(ov: &mut Overlay, ctx: &egui::Context, env: &Env, screen: egui::Rect, sel: egui::Rect) {
    let Panel::Result { title, body } = &ov.panel else { return };
    let (title, body) = (title.clone(), body.clone());
    let size = Vec2::new(480.0, 280.0);
    let pos = place(screen, sel, size);
    let mut action: Option<ResultAction> = None;
    let c = theme::c();
    let kit = theme::kit();
    egui::Area::new(egui::Id::new("lens-result")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        theme::panel_frame().inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            ui.set_max_width(size.x);
            ui.label(RichText::new(&title).font(theme::font(text::HEADING)).color(c.text));
            ui.add_space(6.0);
            // A copyable mono block; long output scrolls inside the panel.
            egui::Frame::new().fill(c.sunken).stroke(Stroke::new(1.0_f32, c.line)).corner_radius(radius::SM).inner_margin(8).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(size.y - 110.0).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(&body).font(theme::mono()).color(c.text)).wrap().selectable(true));
                });
            });
            ui.add_space(8.0);
            // Fixed order: Copy, Save, Pin, Send to….
            ui.horizontal(|ui| {
                if kit.button(ui, "Copy", ButtonKind::Primary).on_hover_text("Enter").clicked() {
                    action = Some(ResultAction::Copy);
                }
                if kit.button(ui, "Save", ButtonKind::Default).on_hover_text("Save as a text file in your save folder").clicked() {
                    action = Some(ResultAction::Save);
                }
                if kit.button(ui, "Pin", ButtonKind::Default).on_hover_text("Pin the selection on screen").clicked() {
                    action = Some(ResultAction::Pin);
                }
                if kit.button(ui, "Send to…", ButtonKind::Default).on_hover_text("Connected apps that take text").clicked() {
                    action = Some(ResultAction::Send);
                }
            });
            footer(ui, &[("↵", "Copy"), ("Esc", "Back")]);
        });
    });
    match action {
        Some(ResultAction::Copy) => match lens_core::host::Host::set_clipboard_text(&*env.host, &body) {
            Ok(()) => ov.toast_copied(),
            Err(e) => ov.toast_error(format!("Couldn't copy the result. {e}")),
        },
        Some(ResultAction::Save) => ov.save_text_result(env, &title, &body),
        Some(ResultAction::Pin) => ov.pin_selection(env),
        Some(ResultAction::Send) => ov.send_text_result(&body),
        None => {}
    }
}

enum ResultAction {
    Copy,
    Save,
    Pin,
    Send,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_prefers_below_then_above_then_inside() {
        let screen = egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 800.0));
        let size = Vec2::new(300.0, 60.0);
        let mid = egui::Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::new(200.0, 100.0));
        assert_eq!(place(screen, mid, size), Pos2::new(100.0, 210.0));
        let low = egui::Rect::from_min_size(Pos2::new(100.0, 700.0), Vec2::new(200.0, 80.0));
        assert_eq!(place(screen, low, size), Pos2::new(100.0, 630.0));
        let full = egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 800.0));
        assert_eq!(place(screen, full, size).y, 730.0);
        // Right edge clamping.
        let right = egui::Rect::from_min_size(Pos2::new(900.0, 100.0), Vec2::new(90.0, 50.0));
        assert_eq!(place(screen, right, size).x, 692.0);
    }
}
