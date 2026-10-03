//! Pixel-level recognizers. All of them work on the captured region alone
//! (plus window geometry from the platform), so they apply to any app.

pub mod codes;
pub mod color;
pub mod inspect;
pub mod kind;
pub mod media;
pub mod window;

use lens_core::registry::PluginRegistrar;

pub fn register(r: &mut PluginRegistrar) {
    r.recognizer(color::ColorRecognizer);
    r.recognizer(color::PaletteRecognizer);
    r.recognizer(inspect::InspectRecognizer);
    r.recognizer(kind::ImageKindRecognizer);
    r.recognizer(window::WindowRecognizer);
    r.recognizer(codes::CodeRecognizer);
    r.recognizer(media::MediaFrameRecognizer);
}
