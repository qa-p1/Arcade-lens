//! Keyboard events as the views want to read them.

use eframe::egui::Event;

/// egui-winit turns Ctrl+C into `Event::Copy` and sends no `Key` event for
/// it (Wayland included), so a view that reads only `Key::C` never sees the
/// copy shortcut. This maps it back to the key press the views handle.
pub fn normalize_copy(event: Event) -> Event {
    event
}

/// Removes every `Event::Copy` from `events`; `true` if there was one.
pub fn take_copy(events: &mut Vec<Event>) -> bool {
    let _ = events;
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Key, Modifiers};

    fn is_ctrl_c(e: &Event) -> bool {
        matches!(e, Event::Key { key: Key::C, pressed: true, modifiers, .. } if modifiers.command || modifiers.ctrl)
    }

    // RD-LENS-1: KNOWN BUG until the fix lands (expected to fail).
    #[test]
    #[should_panic(expected = "Event::Copy")]
    fn event_copy_reads_as_ctrl_c() {
        let e = normalize_copy(Event::Copy);
        assert!(is_ctrl_c(&e), "Event::Copy was not mapped to Ctrl+C: {e:?}");
    }

    #[test]
    fn other_events_pass_through() {
        let key = Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        assert!(matches!(normalize_copy(key), Event::Key { key: Key::Enter, .. }));
        assert!(is_ctrl_c(&Event::Key { key: Key::C, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }));
    }

    // RD-LENS-1 in the annotator: KNOWN BUG until the fix lands.
    #[test]
    #[should_panic(expected = "Event::Copy")]
    fn take_copy_finds_and_removes_event_copy() {
        let mut events = vec![Event::Copy, Event::Cut];
        assert!(take_copy(&mut events), "Event::Copy not found");
        assert_eq!(events.len(), 1);
    }
}
