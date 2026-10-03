//! Helpers shared by action providers.

use std::io::Cursor;

use image::RgbaImage;
use lens_core::action::{ActionContext, ActionOutcome, Effects, Item};
use lens_core::host::SaveRequest;
use lens_core::settings::{fill_template, ImageFormat};
use lens_core::{LensError, Result, Value};

pub const COPY: Effects = Effects::CLIPBOARD;
pub const OPEN_WEB: Effects = Effects::NETWORK.union(Effects::LAUNCHES_APP);
pub const SEARCH: Effects = Effects::NETWORK.union(Effects::UPLOADS_CONTENT).union(Effects::LAUNCHES_APP);
pub const SAVE: Effects = Effects::WRITES_FILES;
pub const LAUNCH: Effects = Effects::LAUNCHES_APP;

pub fn text_of(item: &Item) -> Result<String> {
    item.value.as_text().map(|t| t.into_owned()).ok_or_else(|| LensError::InvalidInput(format!("{} has no text", item.capability)))
}

pub fn image_of(item: &Item) -> Result<&RgbaImage> {
    item.value.as_image().map(|i| &**i).ok_or_else(|| LensError::InvalidInput(format!("{} is not an image", item.capability)))
}

pub fn copy(cx: &ActionContext, text: &str, what: &str) -> Result<ActionOutcome> {
    cx.host.set_clipboard_text(text)?;
    Ok(ActionOutcome::done(format!("Copied {what}")))
}

pub fn copy_image(cx: &ActionContext, img: &RgbaImage) -> Result<ActionOutcome> {
    cx.host.set_clipboard_image(img)?;
    Ok(ActionOutcome::done("Copied image"))
}

pub fn open(cx: &ActionContext, uri: &str) -> Result<ActionOutcome> {
    cx.host.open_uri(uri, false)?;
    Ok(ActionOutcome::done("Opened"))
}

pub fn web_search_url(cx: &ActionContext, query: &str) -> String {
    fill_template(&cx.settings.providers.web_search, &[("query", query)])
}

pub fn search(cx: &ActionContext, query: &str) -> Result<ActionOutcome> {
    cx.host.open_uri(&web_search_url(cx, query), false)?;
    Ok(ActionOutcome::done("Searching the web"))
}

pub fn encode(img: &RgbaImage, format: ImageFormat) -> Result<Vec<u8>> {
    let mut buf = Cursor::new(Vec::new());
    let f = match format {
        ImageFormat::Png => image::ImageFormat::Png,
        ImageFormat::Webp => image::ImageFormat::WebP,
        ImageFormat::Jpeg => {
            // JPEG has no alpha channel.
            let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
            rgb.write_to(&mut buf, image::ImageFormat::Jpeg).map_err(|e| LensError::Failed(e.to_string()))?;
            return Ok(buf.into_inner());
        }
    };
    img.write_to(&mut buf, f).map_err(|e| LensError::Failed(e.to_string()))?;
    Ok(buf.into_inner())
}

pub fn mime(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::Webp => "image/webp",
    }
}

pub fn timestamp_name(prefix: &str, ext: &str) -> String {
    format!("{prefix} {}.{ext}", chrono::Local::now().format("%Y-%m-%d at %H.%M.%S"))
}

pub fn save_image(cx: &ActionContext, img: &RgbaImage, prefix: &str, format: Option<ImageFormat>) -> Result<ActionOutcome> {
    let format = format.unwrap_or(cx.settings.image_format);
    let bytes = encode(img, format)?;
    let directory = cx.param_str("directory").map(Into::into).or_else(|| cx.settings.screenshot_dir.clone());
    let path = cx.host.save_file(SaveRequest { suggested_name: timestamp_name(prefix, format.extension()), bytes, directory, mime: mime(format).into() })?;
    Ok(saved(path, mime(format)))
}

pub fn save_text(cx: &ActionContext, name: &str, text: &str, mime: &str) -> Result<ActionOutcome> {
    let directory = cx.param_str("directory").map(Into::into);
    let path = cx.host.save_file(SaveRequest { suggested_name: name.into(), bytes: text.as_bytes().to_vec(), directory, mime: mime.into() })?;
    Ok(saved(path, mime))
}

pub fn saved(path: std::path::PathBuf, mime: &str) -> ActionOutcome {
    let msg = format!("Saved to {}", path.display());
    ActionOutcome::output(Item::new(lens_core::caps::FILE, Value::File(lens_core::value::FileValue { path, mime: Some(mime.into()) }))).with_message(msg)
}

pub fn parse_format(s: Option<&str>) -> Option<ImageFormat> {
    match s?.to_ascii_lowercase().as_str() {
        "png" => Some(ImageFormat::Png),
        "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
        "webp" => Some(ImageFormat::Webp),
        _ => None,
    }
}

/// Text item output for pure transforms.
pub fn text_output(s: String) -> ActionOutcome {
    ActionOutcome::output(Item::new(lens_core::caps::TEXT, Value::text(s)))
}

/// Escapes a value for iCalendar/vCard text fields.
pub fn ical_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace('\n', "\\n")
}

/// A file-name-safe slug.
pub fn slug(s: &str, max: usize) -> String {
    let mut out: String = s.chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').chars().take(max).collect::<String>().trim_end_matches('-').to_string()
}
