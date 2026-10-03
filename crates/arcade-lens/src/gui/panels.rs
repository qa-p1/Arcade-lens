//! Palette surfaces attached to the selection: the compact action row, the
//! expanded list, and confirmation / choice / result panels.

use std::time::Duration;

use eframe::egui::{self, Align2, Color32, CornerRadius, Key, Modifiers, Pos2, RichText, Sense, Stroke, StrokeKind, Vec2};
use lens_core::action::Params;
use lens_core::palette::{PaletteEntry, Target};
use lens_core::{FindingId, SafetyClass, Value};

use super::overlay::{cap_label, describe, Env, Overlay, Panel};
use super::theme::{self, ACCENT, ACCENT_SOFT, BORDER, DANGER, EXTERNAL, FAINT, MUTED, TEXT};

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

fn header(ui: &mut egui::Ui, ov: &Overlay) {
    let painter = ui.painter().clone();
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        if let Some(f) = ov.headline() {
            if let Value::Color(c) = &f.value {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                painter.rect(r, CornerRadius::same(3), Color32::from_rgb(c.r, c.g, c.b), Stroke::new(1.0, Color32::from_white_alpha(70)), StrokeKind::Inside);
            }
            let (title, summary) = describe(f);
            ui.label(RichText::new(title).color(ACCENT_SOFT).size(11.5).strong());
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
            let mut text = summary;
            if !matches!(f.value, Value::Color(_)) {
                if text.chars().count() > 40 {
                    text = text.chars().take(39).collect::<String>() + "…";
                }
                ui.label(RichText::new(&text).color(TEXT).size(12.0));
            }
            let mut d = details.join("  ·  ");
            let room = budget.saturating_sub(text.chars().count());
            if d.chars().count() > room {
                d = d.chars().take(room.saturating_sub(1)).collect::<String>() + "…";
            }
            if !d.is_empty() && room > 8 {
                ui.label(RichText::new(d).color(MUTED).size(11.5));
            }
        }
        if let Some((label, t)) = ov.job_running() {
            if t > Duration::from_millis(120) {
                ui.add(egui::Spinner::new().size(11.0).color(ACCENT_SOFT));
                ui.label(RichText::new(format!("{label}…")).color(MUTED).size(11.5));
            }
        } else if ov.analyzing() {
            ui.add(egui::Spinner::new().size(11.0).color(FAINT));
        }
    });
}

fn primary(ov: &mut Overlay, ctx: &egui::Context, env: &Env, screen: egui::Rect, sel: egui::Rect) {
    let pos = place(screen, sel, ov.palette_size());
    let mut clicked: Option<PaletteEntry> = None;
    let mut more = false;
    let resp = egui::Area::new(egui::Id::new("lens-palette")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        theme::panel_frame().show(ui, |ui| {
            header(ui, ov);
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, e) in ov.palette.primary.iter().enumerate() {
                    let key = e.key.map(|k| k.to_ascii_uppercase().to_string());
                    if theme::chip(ui, &e.label, key.as_deref(), e.safety, theme::ChipState { is_default: i == ov.palette.default_index, focused: false })
                        .clicked()
                    {
                        clicked = Some(e.clone());
                    }
                }
                if ov.palette.all.len() > ov.palette.primary.len() {
                    let r = theme::chip(ui, "More", Some("Space"), SafetyClass::Pure, theme::ChipState { is_default: false, focused: false });
                    if r.clicked() {
                        more = true;
                    }
                }
                if ov.palette.primary.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new("Analyzing…").color(MUTED).size(12.5));
                }
            });
        });
    });
    *ov.palette_size_mut() = resp.response.rect.size();
    if more {
        ov.panel = Panel::Expanded { filter: String::new(), cursor: 0 };
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

fn expanded(ov: &mut Overlay, ctx: &egui::Context, env: &Env, screen: egui::Rect, sel: egui::Rect) {
    let Panel::Expanded { filter, cursor } = &mut ov.panel else { return };
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
    let mut visible: Vec<PaletteEntry> = ov.palette.all.iter().filter(|e| matches_filter(e, filter)).cloned().collect();
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
    let size = Vec2::new(420.0, (screen.height() * 0.7).min(560.0));
    let pos = place_side(screen, sel, size);
    let mut run: Option<PaletteEntry> = None;
    let mut new_filter = filter.clone();
    egui::Area::new(egui::Id::new("lens-expanded")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        theme::panel_frame().inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.set_width(size.x - 16.0);
            let te = egui::TextEdit::singleline(&mut new_filter).hint_text("Filter actions…").desired_width(f32::INFINITY);
            let r = ui.add(te);
            if !r.has_focus() {
                r.request_focus();
            }
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(size.y - 60.0).auto_shrink([false, true]).show(ui, |ui| {
                let mut last_finding: Option<FindingId> = None;
                for (i, e) in visible.iter().enumerate() {
                    if last_finding != Some(e.finding) {
                        last_finding = Some(e.finding);
                        if let Target::Chain(_) = e.target {
                            ui.add_space(6.0);
                            ui.label(RichText::new("CHAINS").color(ACCENT_SOFT).size(10.5).strong());
                        } else if let Some(f) = ov.report.findings.iter().find(|f| f.id == e.finding) {
                            ui.add_space(6.0);
                            let (title, summary) = describe(f);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(title.to_uppercase()).color(ACCENT_SOFT).size(10.5).strong());
                                let short: String = summary.chars().take(48).collect();
                                ui.label(RichText::new(short).color(MUTED).size(11.0));
                            });
                        }
                    }
                    let selected = i == cur;
                    let row = ui.allocate_response(Vec2::new(ui.available_width(), if e.preview.is_some() { 38.0 } else { 26.0 }), Sense::click());
                    let p = ui.painter();
                    if selected || row.hovered() {
                        p.rect_filled(row.rect, CornerRadius::same(6), if selected { ACCENT.gamma_multiply(0.28) } else { theme::SURFACE_HI });
                    }
                    let y = row.rect.min.y + 13.0;
                    let mut x = row.rect.min.x + 8.0;
                    if let Some(k) = e.key {
                        theme::key_badge(p, Pos2::new(x, y), &k.to_ascii_uppercase().to_string(), MUTED);
                    }
                    x += 26.0;
                    let g = p.layout_no_wrap(e.label.clone(), theme::font(13.5), TEXT);
                    let gw = g.size().x;
                    p.galley(Pos2::new(x, y - g.size().y / 2.0), g, TEXT);
                    theme::safety_marker(p, Pos2::new(x + gw + 9.0, y), e.safety);
                    if e.needs_confirmation {
                        p.text(Pos2::new(row.rect.max.x - 8.0, y), Align2::RIGHT_CENTER, "asks first", theme::font(10.5), FAINT);
                    }
                    if let Some(prev) = &e.preview {
                        let short: String = prev.chars().take(60).collect();
                        let color = if e.safety == SafetyClass::External { EXTERNAL } else { FAINT };
                        p.text(Pos2::new(x, y + 15.0), Align2::LEFT_CENTER, format!("sends: {short}"), theme::font(10.5), color);
                    }
                    if selected {
                        row.scroll_to_me(None);
                    }
                    if row.clicked() {
                        run = Some(e.clone());
                    }
                }
                if visible.is_empty() {
                    ui.label(RichText::new("No matching actions").color(MUTED));
                }
            });
        });
    });
    if let Panel::Expanded { filter, cursor } = &mut ov.panel {
        if *filter != new_filter {
            *filter = new_filter;
            *cursor = 0;
        }
    }
    if enter {
        run = visible.get(cur).cloned();
    }
    if let Some(e) = run {
        ov.panel = Panel::Primary;
        ov.run_entry(env, e, Params::new(), false);
    }
}

fn confirm(ov: &mut Overlay, ctx: &egui::Context, env: &Env, screen: egui::Rect, sel: egui::Rect) {
    let Panel::Confirm { entry, req, .. } = &ov.panel else { return };
    let dangerous = entry.safety == SafetyClass::Dangerous;
    let (title, subject, reasons) = (req.title.clone(), req.subject.clone(), req.reasons.clone());
    let size = Vec2::new(460.0, 220.0);
    let pos = place(screen, sel, size);
    let mut go = false;
    let mut cancel = false;
    egui::Area::new(egui::Id::new("lens-confirm")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        theme::panel_frame().inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            ui.set_max_width(size.x);
            ui.label(RichText::new(&title).color(if dangerous { DANGER } else { TEXT }).size(15.0).strong());
            ui.add_space(6.0);
            egui::Frame::new().fill(Color32::from_rgb(9, 9, 11)).stroke(Stroke::new(1.0, BORDER)).corner_radius(6).inner_margin(8).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(110.0).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(&subject).monospace().color(TEXT).size(12.5)).wrap());
                });
            });
            ui.add_space(6.0);
            for r in &reasons {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("•").color(if dangerous { DANGER } else { MUTED }));
                    ui.label(RichText::new(r).color(MUTED).size(12.0));
                });
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Cancel  (Esc)").size(12.5)).clicked() {
                    cancel = true;
                }
                let label = if dangerous { "Run anyway  (Ctrl+Enter)" } else { "Continue  (Enter)" };
                let b = egui::Button::new(RichText::new(label).color(Color32::WHITE).size(12.5)).fill(if dangerous {
                    DANGER.gamma_multiply(0.75)
                } else {
                    ACCENT.gamma_multiply(0.85)
                });
                if ui.add(b).clicked() {
                    go = true;
                }
            });
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
    let size = Vec2::new(380.0, 40.0 + 30.0 * items.len().min(9) as f32);
    let pos = place(screen, sel, size);
    let mut picked = None;
    egui::Area::new(egui::Id::new("lens-choice")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        theme::panel_frame().inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.set_width(size.x - 16.0);
            ui.label(RichText::new(format!("{label} — which one?")).color(MUTED).size(12.0));
            ui.add_space(2.0);
            for (i, it) in items.iter().enumerate() {
                let row = ui.allocate_response(Vec2::new(ui.available_width(), 28.0), Sense::click());
                let p = ui.painter();
                if i == cur || row.hovered() {
                    p.rect_filled(row.rect, CornerRadius::same(6), if i == cur { ACCENT.gamma_multiply(0.28) } else { theme::SURFACE_HI });
                }
                if i < 9 {
                    theme::key_badge(p, Pos2::new(row.rect.min.x + 8.0, row.rect.center().y), &(i + 1).to_string(), MUTED);
                }
                p.text(Pos2::new(row.rect.min.x + 34.0, row.rect.center().y), Align2::LEFT_CENTER, it, theme::font(13.0), TEXT);
                if row.clicked() {
                    picked = Some(i);
                }
            }
        });
    });
    if let Some(i) = picked {
        ov.pick_choice(env, i);
    }
}

fn result(ov: &mut Overlay, ctx: &egui::Context, env: &Env, screen: egui::Rect, sel: egui::Rect) {
    let Panel::Result { title, body } = &ov.panel else { return };
    let (title, body) = (title.clone(), body.clone());
    let size = Vec2::new(480.0, 260.0);
    let pos = place(screen, sel, size);
    let mut copy = false;
    let mut close = false;
    egui::Area::new(egui::Id::new("lens-result")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        theme::panel_frame().inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            ui.set_max_width(size.x);
            ui.label(RichText::new(&title).color(ACCENT_SOFT).size(13.0).strong());
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(size.y - 70.0).show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(&body).monospace().color(TEXT).size(12.5)).wrap());
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Copy  (Enter)").clicked() {
                    copy = true;
                }
                if ui.button("Close  (Esc)").clicked() {
                    close = true;
                }
            });
        });
    });
    if copy {
        let _ = lens_core::host::Host::set_clipboard_text(&*env.host, &body);
        ov.close_requested = true;
    }
    if close {
        ov.panel = Panel::Primary;
    }
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
