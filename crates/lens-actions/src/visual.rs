//! Region, image, icon, color, palette, UI-inspection and window actions.

use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::Arc;

use image::imageops::{self, FilterType};
use image::{Rgba, RgbaImage};
use lens_core::action::{ActionGroup as G, Effects, Item};
use lens_core::builder::action;
use lens_core::color::Rgb;
use lens_core::host::{HostFeatures as F, OpenPathMode, SaveRequest, WindowCommand};
use lens_core::registry::PluginRegistrar;
use lens_core::value::{ImageValue, PaletteColor};
use lens_core::{caps, ActionOutcome, LensError, Result, Value};

use crate::util::*;

fn image_output(img: RgbaImage) -> ActionOutcome {
    ActionOutcome::output(Item::new(caps::IMAGE, Value::Image(ImageValue { image: Arc::new(img), origin: None })))
}

pub fn register(r: &mut PluginRegistrar) {
    region(r);
    image_ops(r);
    icon(r);
    color(r);
    palette(r);
    inspect(r);
    window(r);
}

fn region(r: &mut PluginRegistrar) {
    let image_caps = [caps::REGION, caps::IMAGE, caps::ICON];
    r.action(
        action("core.region.copy", "Copy")
            .icon("copy")
            .group(G::Copy)
            .accepts(caps::REGION)
            .passthrough()
            .priority(80)
            .key('c')
            .effects(COPY)
            .requires(F::CLIPBOARD_IMAGE)
            .run(|i, cx| copy_image(cx, image_of(i)?)),
    );
    r.action(
        action("core.region.save", "Save")
            .icon("download")
            .group(G::Save)
            .accepts(caps::REGION)
            .produces(caps::FILE)
            .priority(75)
            .key('s')
            .effects(SAVE)
            .requires(F::SAVE_FILE)
            .run(|i, cx| save_image(cx, image_of(i)?, "Lens", None)),
    );
    r.action(
        action("core.region.pin", "Pin")
            .icon("pin")
            .group(G::Edit)
            .accepts(caps::REGION)
            .priority(72)
            .key('p')
            .effects(Effects::PERSISTS)
            .requires(F::PIN)
            .run(|i, cx| {
                let Value::Image(v) = &i.value else { return Err(LensError::InvalidInput("not an image".into())) };
                cx.host.pin(v.image.clone(), v.origin)?;
                Ok(ActionOutcome::done("Pinned"))
            }),
    );
    r.action(
        action("core.region.annotate", "Annotate")
            .icon("pencil")
            .group(G::Edit)
            .accepts(caps::REGION)
            .priority(60)
            .key('a')
            .effects(LAUNCH)
            .requires(F::ANNOTATE)
            .run(|i, cx| {
                let Value::Image(v) = &i.value else { return Err(LensError::InvalidInput("not an image".into())) };
                cx.host.annotate(v.image.clone())?;
                Ok(ActionOutcome::default())
            }),
    );
    r.action(
        action("core.region.share", "Share")
            .icon("share")
            .group(G::Share)
            .accepts(caps::REGION)
            .priority(45)
            .effects(Effects::UPLOADS_CONTENT | Effects::NETWORK)
            .requires(F::SHARE)
            .run(|i, cx| {
                cx.host.share_image(image_of(i)?)?;
                Ok(ActionOutcome::default())
            }),
    );
    r.action(
        action("core.region.send", "Send to Device")
            .icon("device")
            .group(G::Share)
            .accepts_all(image_caps.clone())
            .priority(42)
            .effects(Effects::SENDS_TO_DEVICE)
            .requires(F::SEND_TO_DEVICE)
            .run(|i, cx| {
                cx.host.send_to_device(None, Some(image_of(i)?))?;
                Ok(ActionOutcome::done("Sent"))
            }),
    );
    r.action(
        action("core.region.edit", "Open in Image Editor")
            .icon("image-edit")
            .group(G::Edit)
            .accepts_all(image_caps.clone())
            .priority(40)
            .effects(SAVE | LAUNCH)
            .requires(F::SAVE_FILE | F::OPEN_PATH)
            .run(|i, cx| {
                let out = save_image(cx, image_of(i)?, "Lens", Some(lens_core::settings::ImageFormat::Png))?;
                if let Some(Value::File(f)) = out.output.as_ref().map(|o| &o.value) {
                    cx.host.open_path(&f.path, OpenPathMode::ImageEditor)?;
                }
                Ok(out)
            }),
    );
    // Chain building blocks that accept any value.
    r.action(action("core.copy", "Copy").icon("copy").group(G::Copy).accepts(caps::ANY).passthrough().effects(COPY).chain_only().run(|i, cx| {
        match i.value.as_image() {
            Some(img) => copy_image(cx, img),
            None => copy(cx, &text_of(i)?, "text"),
        }
    }));
    r.action(
        action("core.save", "Save")
            .icon("download")
            .group(G::Save)
            .accepts(caps::ANY)
            .produces(caps::FILE)
            .effects(SAVE)
            .chain_only()
            .param("directory", "Target folder (default: screenshots folder)", serde_json::Value::Null)
            .param("format", "png, jpg or webp for images; file extension for text", serde_json::Value::Null)
            .run(|i, cx| match i.value.as_image() {
                Some(img) => save_image(cx, img, "Lens", parse_format(cx.param_str("format"))),
                None => {
                    let ext = cx.param_str("format").unwrap_or("txt");
                    save_text(cx, &timestamp_name("Lens", ext), &text_of(i)?, "text/plain")
                }
            }),
    );
}

fn image_ops(r: &mut PluginRegistrar) {
    let imgs = [caps::REGION, caps::IMAGE, caps::ICON];
    r.action(
        action("core.image.open", "Open Image")
            .icon("image")
            .group(G::Open)
            .accepts(caps::IMAGE)
            .priority(55)
            .effects(SAVE | LAUNCH)
            .requires(F::SAVE_FILE | F::OPEN_PATH)
            .run(|i, cx| {
                let out = save_image(cx, image_of(i)?, "Lens", Some(lens_core::settings::ImageFormat::Png))?;
                if let Some(Value::File(f)) = out.output.as_ref().map(|o| &o.value) {
                    cx.host.open_path(&f.path, OpenPathMode::Default)?;
                }
                Ok(out)
            }),
    );
    r.action(
        action("core.image.quick-look", "Quick Look")
            .icon("eye")
            .group(G::Inspect)
            .accepts(caps::IMAGE)
            .priority(35)
            .effects(SAVE | LAUNCH)
            .requires(F::SAVE_FILE | F::QUICK_LOOK)
            .run(|i, cx| {
                let out = save_image(cx, image_of(i)?, "Lens", Some(lens_core::settings::ImageFormat::Png))?;
                if let Some(Value::File(f)) = out.output.as_ref().map(|o| &o.value) {
                    cx.host.open_path(&f.path, OpenPathMode::QuickLook)?;
                }
                Ok(ActionOutcome::default())
            }),
    );
    r.action(
        action("core.image.resize", "Resize 50%")
            .icon("resize")
            .group(G::Transform)
            .accepts_all(imgs.clone())
            .produces(caps::IMAGE)
            .priority(30)
            .param("percent", "Scale in percent", serde_json::json!(50))
            .run(|i, cx| {
                let img = image_of(i)?;
                let pct = cx.param_f64("percent").unwrap_or(50.0).clamp(1.0, 800.0) / 100.0;
                let (w, h) = (((img.width() as f64 * pct).round() as u32).max(1), ((img.height() as f64 * pct).round() as u32).max(1));
                Ok(image_output(imageops::resize(img, w, h, FilterType::Lanczos3)))
            }),
    );
    r.action(
        action("core.image.rotate", "Rotate 90°")
            .icon("rotate")
            .group(G::Transform)
            .accepts(caps::IMAGE)
            .produces(caps::IMAGE)
            .priority(25)
            .run(|i, _| Ok(image_output(imageops::rotate90(image_of(i)?)))),
    );
    r.action(
        action("core.image.flip", "Flip Horizontal")
            .icon("flip")
            .group(G::Transform)
            .accepts(caps::IMAGE)
            .produces(caps::IMAGE)
            .priority(24)
            .run(|i, _| Ok(image_output(imageops::flip_horizontal(image_of(i)?)))),
    );
    r.action(action("core.image.dominant-color", "Copy Dominant Color").icon("droplet").group(G::Copy).accepts(caps::IMAGE).priority(35).effects(COPY).run(
        |i, cx| {
            let p = lens_recognizers::image::color::extract_palette(image_of(i)?);
            let c = p.first().ok_or_else(|| LensError::Failed("no dominant color".into()))?;
            copy(cx, &c.color.hex(), "dominant color")
        },
    ));
    // Chain-only typed conversions.
    r.action(action("core.image.grayscale", "Grayscale").icon("contrast").group(G::Transform).accepts(caps::IMAGE).produces(caps::IMAGE).chain_only().run(
        |i, _| {
            let g = image::DynamicImage::ImageRgba8(image_of(i)?.clone()).grayscale().to_rgba8();
            Ok(image_output(g))
        },
    ));
}

/// Removes a flat background by flood-filling from the border, so icons on
/// a solid backdrop become transparent. Deterministic; no ML.
pub fn remove_background(img: &RgbaImage, tolerance: f64) -> RgbaImage {
    let (w, h) = img.dimensions();
    let mut out = img.clone();
    let corners = [img.get_pixel(0, 0), img.get_pixel(w - 1, 0), img.get_pixel(0, h - 1), img.get_pixel(w - 1, h - 1)];
    let bg = Rgb::new(corners[0][0], corners[0][1], corners[0][2]).to_lab();
    let mut seen = vec![false; (w * h) as usize];
    let mut queue: VecDeque<(u32, u32)> = VecDeque::new();
    for x in 0..w {
        queue.push_back((x, 0));
        queue.push_back((x, h - 1));
    }
    for y in 0..h {
        queue.push_back((0, y));
        queue.push_back((w - 1, y));
    }
    while let Some((x, y)) = queue.pop_front() {
        let idx = (y * w + x) as usize;
        if seen[idx] {
            continue;
        }
        seen[idx] = true;
        let p = img.get_pixel(x, y);
        if p[3] != 0 && Rgb::new(p[0], p[1], p[2]).to_lab().delta_e(&bg) > tolerance {
            continue;
        }
        out.put_pixel(x, y, Rgba([p[0], p[1], p[2], 0]));
        if x > 0 {
            queue.push_back((x - 1, y));
        }
        if x + 1 < w {
            queue.push_back((x + 1, y));
        }
        if y > 0 {
            queue.push_back((x, y - 1));
        }
        if y + 1 < h {
            queue.push_back((x, y + 1));
        }
    }
    out
}

fn square(img: &RgbaImage, size: u32) -> RgbaImage {
    let side = img.width().max(img.height());
    let mut canvas = RgbaImage::from_pixel(side, side, Rgba([0, 0, 0, 0]));
    imageops::overlay(&mut canvas, img, ((side - img.width()) / 2) as i64, ((side - img.height()) / 2) as i64);
    imageops::resize(&canvas, size, size, FilterType::Lanczos3)
}

pub fn ico_bytes(img: &RgbaImage) -> Result<Vec<u8>> {
    use image::codecs::ico::{IcoEncoder, IcoFrame};
    let frames: Vec<IcoFrame> = [16u32, 24, 32, 48, 64, 128, 256]
        .iter()
        .map(|s| {
            let sq = square(img, *s);
            IcoFrame::as_png(sq.as_raw(), *s, *s, image::ExtendedColorType::Rgba8).map_err(|e| LensError::Failed(e.to_string()))
        })
        .collect::<Result<_>>()?;
    let mut buf = Cursor::new(Vec::new());
    IcoEncoder::new(&mut buf).encode_images(&frames).map_err(|e| LensError::Failed(e.to_string()))?;
    Ok(buf.into_inner())
}

fn icon(r: &mut PluginRegistrar) {
    r.action(
        action("core.icon.save-png", "Save PNG")
            .icon("download")
            .group(G::Save)
            .accepts(caps::ICON)
            .produces(caps::FILE)
            .priority(50)
            .effects(SAVE)
            .requires(F::SAVE_FILE)
            .run(|i, cx| save_image(cx, image_of(i)?, "Icon", Some(lens_core::settings::ImageFormat::Png))),
    );
    r.action(
        action("core.icon.remove-background", "Remove Background")
            .icon("eraser")
            .group(G::Transform)
            .accepts(caps::ICON)
            .produces(caps::IMAGE)
            .priority(45)
            .run(|i, _| Ok(image_output(remove_background(image_of(i)?, 10.0)))),
    );
    r.action(
        action("core.icon.ico", "Create ICO")
            .icon("file-image")
            .group(G::Save)
            .accepts(caps::ICON)
            .produces(caps::FILE)
            .priority(40)
            .effects(SAVE)
            .requires(F::SAVE_FILE)
            .run(|i, cx| {
                let bytes = ico_bytes(image_of(i)?)?;
                let path =
                    cx.host.save_file(SaveRequest { suggested_name: timestamp_name("Icon", "ico"), bytes, directory: None, mime: "image/x-icon".into() })?;
                Ok(saved(path, "image/x-icon"))
            }),
    );
    r.action(
        action("core.icon.app-sizes", "Create App Icon Sizes")
            .icon("grid")
            .group(G::Save)
            .accepts(caps::ICON)
            .priority(35)
            .effects(SAVE)
            .requires(F::SAVE_FILE)
            .run(|i, cx| {
                let img = image_of(i)?;
                let mut last = None;
                for s in [16u32, 32, 64, 128, 256, 512, 1024] {
                    let bytes = encode(&square(img, s), lens_core::settings::ImageFormat::Png)?;
                    last = Some(cx.host.save_file(SaveRequest {
                        suggested_name: format!("icon-{s}x{s}.png"),
                        bytes,
                        directory: None,
                        mime: "image/png".into(),
                    })?);
                }
                let dir = last.and_then(|p| p.parent().map(|d| d.display().to_string())).unwrap_or_default();
                Ok(ActionOutcome::done(format!("Saved 7 sizes to {dir}")))
            }),
    );
    r.action(action("core.icon.color", "Extract Color").icon("droplet").group(G::Copy).accepts(caps::ICON).priority(38).effects(COPY).run(|i, cx| {
        let img = image_of(i)?;
        let bg = img.get_pixel(0, 0);
        let bg = Rgb::new(bg[0], bg[1], bg[2]).to_lab();
        let palette = lens_recognizers::image::color::extract_palette(img);
        let c = palette.iter().find(|p| p.color.to_lab().delta_e(&bg) > 12.0).or(palette.first()).ok_or_else(|| LensError::Failed("no color".into()))?;
        copy(cx, &c.color.hex(), "icon color")
    }));
}

fn color_of(i: &Item) -> Result<Rgb> {
    match &i.value {
        Value::Color(c) => Ok(*c),
        _ => Err(LensError::InvalidInput("not a color".into())),
    }
}

fn color(r: &mut PluginRegistrar) {
    r.action(
        action("core.color.copy-hex", "Copy HEX")
            .icon("hash")
            .group(G::Copy)
            .accepts(caps::COLOR)
            .priority(95)
            .key('c')
            .effects(COPY)
            .run(|i, cx| copy(cx, &color_of(i)?.hex(), "HEX")),
    );
    r.action(
        action("core.color.copy-rgb", "Copy RGB")
            .icon("copy")
            .group(G::Copy)
            .accepts(caps::COLOR)
            .priority(85)
            .key('r')
            .effects(COPY)
            .run(|i, cx| copy(cx, &color_of(i)?.rgb_string(), "RGB")),
    );
    r.action(
        action("core.color.copy-hsl", "Copy HSL")
            .icon("copy")
            .group(G::Transform)
            .accepts(caps::COLOR)
            .priority(80)
            .key('h')
            .effects(COPY)
            .run(|i, cx| copy(cx, &color_of(i)?.hsl_string(), "HSL")),
    );
    r.action(
        action("core.color.copy-css", "Copy CSS")
            .icon("code")
            .group(G::Transform)
            .accepts(caps::COLOR)
            .priority(78)
            .effects(COPY)
            .run(|i, cx| copy(cx, &format!("color: {};", color_of(i)?.hex()), "CSS")),
    );
    r.action(
        action("core.color.save", "Add to Palette")
            .icon("palette")
            .group(G::Save)
            .accepts(caps::COLOR)
            .priority(60)
            .effects(Effects::PERSISTS | SAVE)
            .requires(F::PERSIST)
            .run(|i, cx| {
                cx.host.persist("colors", serde_json::json!({ "hex": color_of(i)?.hex() }))?;
                Ok(ActionOutcome::done("Added to palette"))
            }),
    );
}

fn palette_of(i: &Item) -> Result<&[PaletteColor]> {
    match &i.value {
        Value::Palette(p) => Ok(p),
        _ => Err(LensError::InvalidInput("not a palette".into())),
    }
}

pub fn css_variables(p: &[PaletteColor]) -> String {
    let vars: Vec<String> = p.iter().enumerate().map(|(n, c)| format!("  --color-{}: {};", n + 1, c.color.hex())).collect();
    format!(":root {{\n{}\n}}\n", vars.join("\n"))
}

fn palette(r: &mut PluginRegistrar) {
    r.action(action("core.palette.copy", "Copy Palette").icon("palette").group(G::Copy).accepts(caps::PALETTE).priority(65).effects(COPY).run(|i, cx| {
        let s = palette_of(i)?.iter().map(|c| c.color.hex()).collect::<Vec<_>>().join(", ");
        copy(cx, &s, "palette")
    }));
    r.action(
        action("core.palette.css", "Export CSS Variables")
            .icon("code")
            .group(G::Transform)
            .accepts(caps::PALETTE)
            .priority(55)
            .effects(COPY)
            .run(|i, cx| copy(cx, &css_variables(palette_of(i)?), "CSS variables")),
    );
    r.action(
        action("core.palette.hex-list", "Copy HEX List")
            .icon("list")
            .group(G::Copy)
            .accepts(caps::PALETTE)
            .priority(50)
            .effects(COPY)
            .run(|i, cx| copy(cx, &palette_of(i)?.iter().map(|c| c.color.hex()).collect::<Vec<_>>().join("\n"), "HEX list")),
    );
    r.action(
        action("core.palette.save", "Save Palette")
            .icon("save")
            .group(G::Save)
            .accepts(caps::PALETTE)
            .priority(40)
            .effects(Effects::PERSISTS | SAVE)
            .requires(F::PERSIST)
            .run(|i, cx| {
                let colors: Vec<String> = palette_of(i)?.iter().map(|c| c.color.hex()).collect();
                cx.host.persist("palettes", serde_json::json!({ "colors": colors }))?;
                Ok(ActionOutcome::done("Palette saved"))
            }),
    );
}

fn inspect(r: &mut PluginRegistrar) {
    let geo = |i: &Item| match &i.value {
        Value::Geometry(g) => Ok(g.clone()),
        _ => Err(LensError::InvalidInput("not a UI element".into())),
    };
    r.action(action("core.ui.copy-size", "Copy Dimensions").icon("ruler").group(G::Copy).accepts(caps::UI_ELEMENT).priority(55).key('d').effects(COPY).run(
        move |i, cx| {
            let g = geo(i)?;
            copy(cx, &format!("{} × {}", g.rect.width, g.rect.height), "dimensions")
        },
    ));
    r.action(action("core.ui.copy-position", "Copy Position").icon("crosshair").group(G::Copy).accepts(caps::UI_ELEMENT).priority(40).effects(COPY).run(
        move |i, cx| {
            let g = geo(i)?;
            copy(cx, &format!("{}, {}", g.rect.x, g.rect.y), "position")
        },
    ));
    r.action(
        action("core.ui.copy-background", "Copy Background Color")
            .icon("droplet")
            .group(G::Copy)
            .accepts(caps::UI_ELEMENT)
            .priority(45)
            .effects(COPY)
            .applies(|f, _| matches!(&f.value, Value::Geometry(g) if g.background.is_some()))
            .run(move |i, cx| copy(cx, &geo(i)?.background.map(|c| c.hex()).unwrap_or_default(), "background color")),
    );
    r.action(
        action("core.ui.copy-foreground", "Copy Text Color")
            .icon("type")
            .group(G::Copy)
            .accepts(caps::UI_ELEMENT)
            .priority(44)
            .effects(COPY)
            .applies(|f, _| matches!(&f.value, Value::Geometry(g) if g.foreground.is_some()))
            .run(move |i, cx| copy(cx, &geo(i)?.foreground.map(|c| c.hex()).unwrap_or_default(), "text color")),
    );
    r.action(action("core.ui.measure", "Measure").icon("ruler").group(G::Inspect).accepts(caps::UI_ELEMENT).priority(42).key('m').requires(F::MEASURE).run(
        move |i, cx| {
            cx.host.measure(geo(i)?.rect)?;
            Ok(ActionOutcome::default())
        },
    ));
}

fn window(r: &mut PluginRegistrar) {
    let win = |i: &Item| match &i.value {
        Value::Window(w) => Ok(w.clone()),
        _ => Err(LensError::InvalidInput("not a window".into())),
    };
    let cmd = move |id: &str, label: &str, icon: &str, priority: i32, command: WindowCommand, feature: F, extra: Effects| {
        let command2 = command.clone();
        action(id, label).icon(icon).group(G::System).accepts(caps::WINDOW).priority(priority).effects(Effects::WINDOW_CONTROL | extra).requires(feature).run(
            move |i, cx| {
                cx.host.window_command(&win(i)?, command2.clone())?;
                Ok(ActionOutcome::default())
            },
        )
    };
    r.action(cmd("core.window.topmost", "Always on Top", "layers", 50, WindowCommand::ToggleAlwaysOnTop, F::WINDOW_TOPMOST, Effects::empty()));
    r.action(cmd("core.window.next-monitor", "Move to Next Monitor", "monitor", 40, WindowCommand::MoveToNextMonitor, F::WINDOW_MOVE, Effects::empty()));
    r.action(cmd("core.window.record", "Record Window", "video", 45, WindowCommand::Record, F::RECORD_WINDOW, SAVE));
    // Closing can discard unsaved work in the target app, so it is confirmed.
    r.action(
        action("core.window.close", "Close Window")
            .icon("x")
            .group(G::System)
            .accepts(caps::WINDOW)
            .priority(10)
            .effects(Effects::WINDOW_CONTROL)
            .requires(F::WINDOW_CLOSE)
            .confirm(|i, _| {
                let title = match &i.value {
                    Value::Window(w) => w.title.clone(),
                    _ => String::new(),
                };
                Some(lens_core::action::ConfirmRequest {
                    title: "Close window".into(),
                    subject: title,
                    reasons: vec!["Unsaved work in that window may be lost.".into()],
                })
            })
            .run(move |i, cx| {
                cx.host.window_command(&win(i)?, WindowCommand::Close)?;
                Ok(ActionOutcome::default())
            }),
    );
    r.action(
        action("core.window.screenshot", "Copy Window Image")
            .icon("app-window")
            .group(G::Copy)
            .accepts(caps::WINDOW)
            .priority(55)
            .effects(COPY)
            .requires(F::CLIPBOARD_IMAGE)
            .run(|_, cx| {
                let sel = cx.selection.ok_or_else(|| LensError::InvalidInput("no selection".into()))?;
                copy_image(cx, &sel.image)
            }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_removal_keeps_the_subject() {
        let mut img = RgbaImage::from_pixel(20, 20, Rgba([255, 255, 255, 255]));
        for x in 5..15 {
            for y in 5..15 {
                img.put_pixel(x, y, Rgba([200, 30, 30, 255]));
            }
        }
        let out = remove_background(&img, 10.0);
        assert_eq!(out.get_pixel(0, 0)[3], 0);
        assert_eq!(out.get_pixel(10, 10)[3], 255);
    }

    #[test]
    fn ico_contains_all_sizes() {
        let img = RgbaImage::from_pixel(40, 30, Rgba([10, 20, 30, 255]));
        let bytes = ico_bytes(&img).unwrap();
        assert_eq!(&bytes[0..4], &[0, 0, 1, 0]);
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), 7);
    }

    #[test]
    fn css_vars() {
        let p = vec![PaletteColor { color: Rgb::new(0x18, 0x18, 0x1B), coverage: 0.5 }, PaletteColor { color: Rgb::new(0x7C, 0x3A, 0xED), coverage: 0.2 }];
        assert_eq!(css_variables(&p), ":root {\n  --color-1: #18181B;\n  --color-2: #7C3AED;\n}\n");
    }
}
