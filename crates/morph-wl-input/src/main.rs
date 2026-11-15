//! `morph-wl-input`: keyboard and pointer input for the Wayland key suite
//! (`scripts/xvfb-check.py --wayland`), the counterpart of xdotool on X11.
//!
//! It connects to `WAYLAND_DISPLAY` as a virtual keyboard
//! (zwp_virtual_keyboard_v1, with the standard evdev/US keymap so keycodes
//! mean what they mean on a real keyboard: `KP_1` is the keypad's key, not
//! a re-mapped one as with wtype) and a virtual pointer
//! (zwlr_virtual_pointer_v1), then reads one command a line from stdin and
//! answers `ok` once the compositor has it:
//!
//! ```text
//! key CODE 1|0          evdev key code (xkb keycode - 8) pressed or released
//! move X Y WIDTH HEIGHT pointer to X,Y of a WIDTHxHEIGHT output layout
//! button CODE 1|0       evdev button (272 left, 273 right, 274 middle)
//! scroll STEPS          wheel steps, positive down
//! sync                  just answer
//! ```

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    match linux::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("morph-wl-input: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    eprintln!("morph-wl-input: Wayland is Linux only");
    std::process::ExitCode::FAILURE
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io::{BufRead, Write};
    use std::os::fd::AsFd;
    use std::time::Instant;

    use wayland_client::protocol::{wl_pointer, wl_registry, wl_seat};
    use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
    use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{zwp_virtual_keyboard_manager_v1 as vkm, zwp_virtual_keyboard_v1 as vk};
    use wayland_protocols_wlr::virtual_pointer::v1::client::{zwlr_virtual_pointer_manager_v1 as vpm, zwlr_virtual_pointer_v1 as vp};
    use xkbcommon::xkb;

    #[derive(Default)]
    struct Globals {
        seat: Option<wl_seat::WlSeat>,
        keyboards: Option<vkm::ZwpVirtualKeyboardManagerV1>,
        pointers: Option<vpm::ZwlrVirtualPointerManagerV1>,
    }

    impl Dispatch<wl_registry::WlRegistry, ()> for Globals {
        fn event(g: &mut Self, registry: &wl_registry::WlRegistry, event: wl_registry::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
            if let wl_registry::Event::Global { name, interface, version } = event {
                match interface.as_str() {
                    "wl_seat" if g.seat.is_none() => g.seat = Some(registry.bind(name, version.min(7), qh, ())),
                    "zwp_virtual_keyboard_manager_v1" => g.keyboards = Some(registry.bind(name, 1, qh, ())),
                    "zwlr_virtual_pointer_manager_v1" => g.pointers = Some(registry.bind(name, version.min(2), qh, ())),
                    _ => {}
                }
            }
        }
    }

    impl Dispatch<wl_seat::WlSeat, ()> for Globals {
        fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
    }
    delegate_noop!(Globals: ignore vkm::ZwpVirtualKeyboardManagerV1);
    delegate_noop!(Globals: ignore vk::ZwpVirtualKeyboardV1);
    delegate_noop!(Globals: ignore vpm::ZwlrVirtualPointerManagerV1);
    delegate_noop!(Globals: ignore vp::ZwlrVirtualPointerV1);

    /// The keymap every keyboard has unless configured otherwise.
    fn keymap() -> Result<(xkb::Keymap, String), String> {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_names(&context, "evdev", "pc105", "us", "", None, xkb::KEYMAP_COMPILE_NO_FLAGS)
            .ok_or("no evdev/us keymap (is xkeyboard-config installed?)")?;
        let text = keymap.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1);
        Ok((keymap, text))
    }

    /// The keymap as the file descriptor the protocol wants.
    fn keymap_file(text: &str) -> Result<std::fs::File, String> {
        let mut file = tempfile()?;
        file.write_all(text.as_bytes()).and_then(|()| file.write_all(&[0])).map_err(|e| e.to_string())?;
        Ok(file)
    }

    fn tempfile() -> Result<std::fs::File, String> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let path = dir.join(format!("morph-wl-input-{}.xkb", std::process::id()));
        let file = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let _ = std::fs::remove_file(&path);
        Ok(file)
    }

    pub fn run() -> Result<(), String> {
        let conn = Connection::connect_to_env().map_err(|e| format!("no Wayland display: {e}"))?;
        let mut queue = conn.new_event_queue();
        let qh = queue.handle();
        conn.display().get_registry(&qh, ());
        let mut g = Globals::default();
        queue.roundtrip(&mut g).map_err(|e| e.to_string())?;
        let seat = g.seat.clone().ok_or("the compositor has no seat")?;
        let keyboard = g
            .keyboards
            .as_ref()
            .ok_or("the compositor has no virtual keyboards (a wlroots compositor such as sway does)")?
            .create_virtual_keyboard(&seat, &qh, ());
        let pointer = g
            .pointers
            .as_ref()
            .ok_or("the compositor has no virtual pointers (a wlroots compositor such as sway does)")?
            .create_virtual_pointer(Some(&seat), &qh, ());
        let (map, text) = keymap()?;
        let file = keymap_file(&text)?;
        keyboard.keymap(xkb::KEYMAP_FORMAT_TEXT_V1, file.as_fd(), u32::try_from(text.len() + 1).map_err(|e| e.to_string())?);
        let mut state = xkb::State::new(&map);
        queue.roundtrip(&mut g).map_err(|e| e.to_string())?;
        let start = Instant::now();
        let time = || u32::try_from(start.elapsed().as_millis() % u128::from(u32::MAX)).unwrap_or(0);
        let mut out = std::io::stdout();
        writeln!(out, "ready").and_then(|()| out.flush()).map_err(|e| e.to_string())?;
        for line in std::io::stdin().lock().lines() {
            let line = line.map_err(|e| e.to_string())?;
            let words: Vec<&str> = line.split_whitespace().collect();
            let num = |i: usize| words.get(i).and_then(|w| w.parse::<f64>().ok());
            let answer = match (words.first().copied(), num(1), num(2)) {
                (Some("key"), Some(code), Some(down)) => {
                    let code = code as u32;
                    let pressed = down != 0.0;
                    let direction = if pressed { xkb::KeyDirection::Down } else { xkb::KeyDirection::Up };
                    keyboard.key(time(), code, u32::from(pressed));
                    state.update_key(xkb::Keycode::new(code + 8), direction);
                    keyboard.modifiers(
                        state.serialize_mods(xkb::STATE_MODS_DEPRESSED),
                        state.serialize_mods(xkb::STATE_MODS_LATCHED),
                        state.serialize_mods(xkb::STATE_MODS_LOCKED),
                        state.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE),
                    );
                    "ok"
                }
                (Some("move"), Some(x), Some(y)) => match (num(3), num(4)) {
                    (Some(w), Some(h)) => {
                        pointer.motion_absolute(time(), x as u32, y as u32, w as u32, h as u32);
                        pointer.frame();
                        "ok"
                    }
                    _ => "error: move X Y WIDTH HEIGHT",
                },
                (Some("button"), Some(code), Some(down)) => {
                    let state = if down != 0.0 { wl_pointer::ButtonState::Pressed } else { wl_pointer::ButtonState::Released };
                    pointer.button(time(), code as u32, state);
                    pointer.frame();
                    "ok"
                }
                (Some("scroll"), Some(steps), _) => {
                    let t = time();
                    pointer.axis_source(wl_pointer::AxisSource::Wheel);
                    pointer.axis_discrete(t, wl_pointer::Axis::VerticalScroll, steps * 15.0, steps as i32);
                    pointer.frame();
                    "ok"
                }
                (Some("sync"), _, _) => "ok",
                _ => "error: unknown command",
            };
            queue.roundtrip(&mut g).map_err(|e| e.to_string())?;
            writeln!(out, "{answer}").and_then(|()| out.flush()).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
