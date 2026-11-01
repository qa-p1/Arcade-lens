//! The tray icon: shows that Lens is running in the background and gives
//! quick access to capture, settings, restart and quit. Its tooltip is the
//! tagline; a click captures (Windows, Linux); on macOS it opens the menu.
//!
//! Linux uses StatusNotifierItem over D-Bus (KDE, Waybar, Quickshell, GNOME
//! with the AppIndicator extension, …); Windows and macOS use the
//! notification area and the menu bar (`tray-icon`).

/// What the user picked in the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    Capture,
    CaptureText,
    PickColor,
    Settings,
    Restart,
    Quit,
}

/// The tray menu, in the standard order (`None` is a separator).
pub const MENU: [Option<(&str, TrayEvent)>; 7] = [
    Some(("Open Arcade Lens", TrayEvent::Capture)),
    Some(("Capture text", TrayEvent::CaptureText)),
    Some(("Pick color", TrayEvent::PickColor)),
    Some(("Settings", TrayEvent::Settings)),
    None,
    Some(("Restart Arcade Lens", TrayEvent::Restart)),
    Some(("Quit Arcade Lens", TrayEvent::Quit)),
];

/// A square icon as unpremultiplied RGBA.
pub struct TrayIcon {
    pub size: u32,
    pub rgba: Vec<u8>,
}

pub struct Tray {
    #[cfg(target_os = "linux")]
    handle: ksni::blocking::Handle<sni::Item>,
    #[cfg(any(windows, target_os = "macos"))]
    native: native::Native,
}

impl Tray {
    /// Shows the tray icon. `on_event` runs on the tray's thread (Linux) or
    /// the main thread. On Windows and macOS this must be called on the main
    /// thread while its event loop runs. On Linux, if no tray is running yet
    /// (early at login), the icon appears once one starts.
    pub fn spawn(icons: Vec<TrayIcon>, on_event: impl Fn(TrayEvent) + Send + Sync + 'static) -> Result<Tray, String> {
        #[cfg(target_os = "linux")]
        {
            use ksni::blocking::TrayMethods;
            let item = sni::Item::new(icons, Box::new(on_event));
            let handle = item.assume_sni_available(true).spawn().map_err(|e| e.to_string())?;
            return Ok(Tray { handle });
        }
        #[cfg(any(windows, target_os = "macos"))]
        return native::Native::new(icons, Box::new(on_event)).map(|native| Tray { native });
        #[allow(unreachable_code)]
        {
            let _ = (icons, on_event);
            Err("no tray on this platform".into())
        }
    }

    /// Keeps the native object alive while Link temporarily owns its icon.
    /// On Linux callers drop the tray instead: StatusNotifierItem hosts still
    /// show Passive items, so only unregistering hides the icon.
    pub fn set_visible(&self, visible: bool) {
        #[cfg(target_os = "linux")]
        self.handle.update(|t| t.visible = visible);
        #[cfg(any(windows, target_os = "macos"))]
        self.native.set_visible(visible);
        #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
        let _ = visible;
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        self.handle.shutdown().wait();
    }
}

#[cfg(target_os = "linux")]
mod sni {
    use ksni::menu::StandardItem;
    use ksni::{Icon, MenuItem, ToolTip};

    use super::{TrayEvent, TrayIcon, MENU};

    pub struct Item {
        icons: Vec<Icon>,
        pub visible: bool,
        on_event: Box<dyn Fn(TrayEvent) + Send + Sync>,
    }

    impl Item {
        pub fn new(icons: Vec<TrayIcon>, on_event: Box<dyn Fn(TrayEvent) + Send + Sync>) -> Item {
            let icons = icons
                .into_iter()
                .map(|i| Icon {
                    width: i.size as i32,
                    height: i.size as i32,
                    // RGBA → ARGB, which is what StatusNotifierItem carries.
                    data: i.rgba.as_chunks::<4>().0.iter().flat_map(|&[r, g, b, a]| [a, r, g, b]).collect(),
                })
                .collect();
            Item { icons, visible: true, on_event }
        }

        fn action(label: &str, event: TrayEvent) -> MenuItem<Item> {
            StandardItem { label: label.into(), activate: Box::new(move |t: &mut Item| (t.on_event)(event)), ..Default::default() }.into()
        }
    }

    impl ksni::Tray for Item {
        fn id(&self) -> String {
            crate::autostart::OS_ID.into()
        }

        fn title(&self) -> String {
            "Arcade Lens".into()
        }

        fn status(&self) -> ksni::Status {
            if self.visible {
                ksni::Status::Active
            } else {
                ksni::Status::Passive
            }
        }

        fn icon_pixmap(&self) -> Vec<Icon> {
            self.icons.clone()
        }

        fn tool_tip(&self) -> ToolTip {
            ToolTip { title: "Arcade Lens".into(), description: crate::autostart::TAGLINE.into(), icon_name: String::new(), icon_pixmap: Vec::new() }
        }

        /// A click captures; a middle click opens Settings.
        fn activate(&mut self, _x: i32, _y: i32) {
            (self.on_event)(TrayEvent::Capture);
        }

        fn secondary_activate(&mut self, _x: i32, _y: i32) {
            (self.on_event)(TrayEvent::Settings);
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            MENU.iter().map(|item| item.map_or(MenuItem::Separator, |(label, event)| Self::action(label, event))).collect()
        }
    }
}

#[cfg(any(windows, target_os = "macos"))]
mod native {
    use std::sync::Arc;

    use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    use super::{TrayEvent, TrayIcon, MENU};

    pub struct Native {
        icon: tray_icon::TrayIcon,
    }

    impl Native {
        pub fn new(icons: Vec<TrayIcon>, on_event: Box<dyn Fn(TrayEvent) + Send + Sync>) -> Result<Native, String> {
            let on_event: Arc<dyn Fn(TrayEvent) + Send + Sync> = on_event.into();
            let menu = Menu::new();
            let mut actions: Vec<(MenuId, TrayEvent)> = Vec::new();
            for item in MENU {
                match item {
                    Some((label, event)) => {
                        let entry = MenuItem::new(label, true, None);
                        actions.push((entry.id().clone(), event));
                        menu.append(&entry).map_err(|e| e.to_string())?;
                    }
                    None => menu.append(&PredefinedMenuItem::separator()).map_err(|e| e.to_string())?,
                }
            }
            let f = on_event.clone();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                if let Some((_, event)) = actions.iter().find(|(id, _)| *id == e.id) {
                    f(*event);
                }
            }));
            // Windows: a click captures and the menu is on right-click.
            // macOS: a click opens the menu, as menu bar items do.
            TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                    if cfg!(windows) {
                        on_event(TrayEvent::Capture);
                    }
                }
            }));

            // The largest icon; the system scales it to the tray.
            let best = icons.into_iter().max_by_key(|i| i.size).ok_or("no tray icon")?;
            let image = Icon::from_rgba(best.rgba, best.size, best.size).map_err(|e| e.to_string())?;
            let icon = TrayIconBuilder::new()
                .with_icon(image)
                .with_tooltip(crate::autostart::TAGLINE)
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(cfg!(target_os = "macos"))
                // A template image: macOS tints it for the menu bar.
                .with_icon_as_template(cfg!(target_os = "macos"))
                .build()
                .map_err(|e| e.to_string())?;
            Ok(Native { icon })
        }

        pub fn set_visible(&self, visible: bool) {
            let _ = self.icon.set_visible(visible);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_follows_the_standard_order_and_words() {
        let labels: Vec<&str> = MENU.iter().map(|i| i.map_or("—", |(l, _)| l)).collect();
        assert_eq!(labels, ["Open Arcade Lens", "Capture text", "Pick color", "Settings", "—", "Restart Arcade Lens", "Quit Arcade Lens"]);
        assert_eq!(MENU[0].map(|(_, e)| e), Some(TrayEvent::Capture));
    }
}
