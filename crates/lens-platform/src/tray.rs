//! The tray icon: shows that Lens is running in the background and gives
//! quick access to capture, settings, start at login, restart and quit.
//!
//! Linux uses StatusNotifierItem over D-Bus (KDE, Waybar, Quickshell, GNOME
//! with the AppIndicator extension, …); Windows and macOS use the
//! notification area and the menu bar (`tray-icon`).

/// What the user picked in the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    Capture,
    Settings,
    Restart,
    Quit,
}

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

/// Flips start at login; returns whether it is now on.
fn toggle_autostart() -> bool {
    let r = match crate::autostart::launch_path() {
        Ok(exe) if !crate::autostart::is_enabled() => crate::autostart::enable(&exe),
        Ok(_) => crate::autostart::disable(),
        Err(e) => Err(e),
    };
    if let Err(e) = r {
        eprintln!("arcade-lens: start at login: {e}");
    }
    crate::autostart::is_enabled()
}

impl Tray {
    /// Shows the tray icon. `on_event` runs on the tray's thread (Linux) or
    /// the main thread. On Windows and macOS this must be called on the main
    /// thread while its event loop runs. On Linux, if no tray is running yet
    /// (early at login), the icon appears once one starts.
    pub fn spawn(icons: Vec<TrayIcon>, shortcut: Option<String>, on_event: impl Fn(TrayEvent) + Send + Sync + 'static) -> Result<Tray, String> {
        #[cfg(target_os = "linux")]
        {
            use ksni::blocking::TrayMethods;
            let item = sni::Item::new(icons, shortcut, Box::new(on_event));
            let handle = item.assume_sni_available(true).spawn().map_err(|e| e.to_string())?;
            return Ok(Tray { handle });
        }
        #[cfg(any(windows, target_os = "macos"))]
        return native::Native::new(icons, shortcut, Box::new(on_event)).map(|native| Tray { native });
        #[allow(unreachable_code)]
        {
            let _ = (icons, shortcut, on_event);
            Err("no tray on this platform".into())
        }
    }

    /// Updates the shortcut shown in the tooltip.
    pub fn set_shortcut(&self, shortcut: Option<String>) {
        #[cfg(target_os = "linux")]
        self.handle.update(|t| t.shortcut = shortcut);
        #[cfg(any(windows, target_os = "macos"))]
        self.native.set_shortcut(shortcut);
        #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
        let _ = shortcut;
    }

    /// Updates the Start at Login check after it changed elsewhere.
    pub fn set_autostart(&self, on: bool) {
        #[cfg(target_os = "linux")]
        self.handle.update(|t| t.autostart = on);
        #[cfg(any(windows, target_os = "macos"))]
        self.native.set_autostart(on);
        #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
        let _ = on;
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
    use ksni::menu::{CheckmarkItem, StandardItem};
    use ksni::{Icon, MenuItem, ToolTip};

    use super::{TrayEvent, TrayIcon};
    use crate::autostart;

    pub struct Item {
        icons: Vec<Icon>,
        pub shortcut: Option<String>,
        pub autostart: bool,
        on_event: Box<dyn Fn(TrayEvent) + Send + Sync>,
    }

    impl Item {
        pub fn new(icons: Vec<TrayIcon>, shortcut: Option<String>, on_event: Box<dyn Fn(TrayEvent) + Send + Sync>) -> Item {
            let icons = icons
                .into_iter()
                .map(|i| Icon {
                    width: i.size as i32,
                    height: i.size as i32,
                    // RGBA → ARGB, which is what StatusNotifierItem carries.
                    data: i.rgba.as_chunks::<4>().0.iter().flat_map(|&[r, g, b, a]| [a, r, g, b]).collect(),
                })
                .collect();
            Item { icons, shortcut, autostart: autostart::is_enabled(), on_event }
        }

        fn action(label: &str, event: TrayEvent) -> MenuItem<Item> {
            StandardItem { label: label.into(), activate: Box::new(move |t: &mut Item| (t.on_event)(event)), ..Default::default() }.into()
        }
    }

    impl ksni::Tray for Item {
        fn id(&self) -> String {
            "arcade-lens".into()
        }

        fn title(&self) -> String {
            "Arcade Lens".into()
        }

        fn icon_pixmap(&self) -> Vec<Icon> {
            self.icons.clone()
        }

        fn tool_tip(&self) -> ToolTip {
            let description = match &self.shortcut {
                Some(s) => format!("Press {s} to select anything on screen"),
                None => "Running in the background".into(),
            };
            ToolTip { title: "Arcade Lens".into(), description, icon_name: String::new(), icon_pixmap: Vec::new() }
        }

        fn activate(&mut self, _x: i32, _y: i32) {
            (self.on_event)(TrayEvent::Settings);
        }

        fn secondary_activate(&mut self, _x: i32, _y: i32) {
            (self.on_event)(TrayEvent::Capture);
        }

        fn menu_about_to_show(&mut self) {
            // Settings may have changed it since the menu was built.
            self.autostart = autostart::is_enabled();
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            vec![
                Self::action("Capture Screen", TrayEvent::Capture),
                Self::action("Settings…", TrayEvent::Settings),
                MenuItem::Separator,
                CheckmarkItem {
                    label: "Start at Login".into(),
                    checked: self.autostart,
                    activate: Box::new(|t: &mut Item| t.autostart = super::toggle_autostart()),
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                Self::action("Restart Arcade Lens", TrayEvent::Restart),
                Self::action("Quit Arcade Lens", TrayEvent::Quit),
            ]
        }
    }
}

#[cfg(any(windows, target_os = "macos"))]
mod native {
    use std::sync::Arc;

    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    use super::{TrayEvent, TrayIcon};

    pub struct Native {
        icon: tray_icon::TrayIcon,
        login: CheckMenuItem,
    }

    fn tooltip(shortcut: Option<&str>) -> String {
        match shortcut {
            Some(s) => format!("Arcade Lens: press {s} to select anything on screen"),
            None => "Arcade Lens".into(),
        }
    }

    impl Native {
        pub fn new(icons: Vec<TrayIcon>, shortcut: Option<String>, on_event: Box<dyn Fn(TrayEvent) + Send + Sync>) -> Result<Native, String> {
            let on_event: Arc<dyn Fn(TrayEvent) + Send + Sync> = on_event.into();
            let capture = MenuItem::new("Capture Screen", true, None);
            let settings = MenuItem::new("Settings…", true, None);
            let login = CheckMenuItem::new("Start at Login", true, crate::autostart::is_enabled(), None);
            let restart = MenuItem::new("Restart Arcade Lens", true, None);
            let quit = MenuItem::new("Quit Arcade Lens", true, None);
            let menu = Menu::new();
            menu.append_items(&[&capture, &settings, &PredefinedMenuItem::separator(), &login, &PredefinedMenuItem::separator(), &restart, &quit])
                .map_err(|e| e.to_string())?;

            let actions: Vec<(MenuId, TrayEvent)> = vec![
                (capture.id().clone(), TrayEvent::Capture),
                (settings.id().clone(), TrayEvent::Settings),
                (restart.id().clone(), TrayEvent::Restart),
                (quit.id().clone(), TrayEvent::Quit),
            ];
            let login_id = login.id().clone();
            let f = on_event.clone();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                if e.id == login_id {
                    // The menu toggles its own check mark.
                    super::toggle_autostart();
                } else if let Some((_, event)) = actions.iter().find(|(id, _)| *id == e.id) {
                    f(*event);
                }
            }));
            // Windows: a click opens Settings and the menu is on right-click.
            // macOS: a click opens the menu, as menu bar items do.
            TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                    if cfg!(windows) {
                        on_event(TrayEvent::Settings);
                    }
                }
            }));

            // The largest icon; the system scales it to the tray.
            let best = icons.into_iter().max_by_key(|i| i.size).ok_or("no tray icon")?;
            let image = Icon::from_rgba(best.rgba, best.size, best.size).map_err(|e| e.to_string())?;
            let icon = TrayIconBuilder::new()
                .with_icon(image)
                .with_tooltip(tooltip(shortcut.as_deref()))
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(cfg!(target_os = "macos"))
                .build()
                .map_err(|e| e.to_string())?;
            Ok(Native { icon, login })
        }

        pub fn set_shortcut(&self, shortcut: Option<String>) {
            let _ = self.icon.set_tooltip(Some(tooltip(shortcut.as_deref())));
        }

        pub fn set_autostart(&self, on: bool) {
            self.login.set_checked(on);
        }
    }
}
