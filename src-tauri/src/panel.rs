//! The notification-area icon and the panel it opens on right-click.
//!
//! Slint's own tray icon can only open a native Windows menu, which cannot be
//! styled. This icon reports the button and the cursor instead, so a right-click
//! opens a Slint panel drawn like the settings window. The panel exists only
//! while it is open.

use crate::{alerts, launch, view, Theme, TrayPanel, L};
use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
use slint::ComponentHandle;
use std::cell::{Cell, RefCell};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateIcon, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow,
    GetCursorPos, RegisterClassW, RegisterWindowMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
    HC_ACTION, HHOOK, HWND_MESSAGE, MSLLHOOKSTRUCT, WH_MOUSE_LL, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_APP, WM_CONTEXTMENU, WM_LBUTTONUP, WM_MBUTTONUP, WM_RBUTTONUP, WM_XBUTTONUP, WNDCLASSW,
};

const WM_TRAY: u32 = WM_APP + 7;
/// Logical width of the panel; its height follows the content.
const PANEL_WIDTH: f32 = 320.0;
/// Width left for the 24-hour chart inside the panel's paddings.
const CHART_WIDTH: f32 = PANEL_WIDTH - 32.0;

struct Icon {
    hwnd: HWND,
    data: NOTIFYICONDATAW,
}

struct Panel {
    window: TrayPanel,
    /// Where the right-click happened, in physical screen pixels.
    cursor: (i32, i32),
    height: f32,
    shown: Option<(view::PanelView, Vec<String>, bool, bool, bool)>,
    _tick: slint::Timer,
    _click_away: Option<ClickAway>,
}

/// Closes the panel on a click anywhere else on the desktop, for as long as it
/// lives. The lost-focus event alone is not enough: Windows often refuses the
/// foreground to a tray-only process, and a window that never had the focus is
/// never told it lost it.
struct ClickAway(HHOOK);

impl ClickAway {
    fn install() -> Option<Self> {
        unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), Some(instance.into()), 0)
                .ok()
                .map(Self)
        }
    }
}

impl Drop for ClickAway {
    fn drop(&mut self) {
        let _ = unsafe { UnhookWindowsHookEx(self.0) };
    }
}

/// Sees every mouse event on the desktop while the panel is open, on the
/// event-loop thread. It must return at once: Windows drops a slow hook.
unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // On release, like the tray icon's own messages: a click on the icon then
    // closes the panel whichever of the two arrives first, however long it lasts.
    let released = matches!(
        wparam.0 as u32,
        WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP | WM_XBUTTONUP
    );
    if code == HC_ACTION as i32 && released {
        let point = unsafe { (*(lparam.0 as *const MSLLHOOKSTRUCT)).pt };
        if !rect_contains(PANEL_RECT.get(), (point.x, point.y)) {
            let _ = slint::invoke_from_event_loop(close);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn rect_contains(rect: (i32, i32, i32, i32), point: (i32, i32)) -> bool {
    (rect.0..rect.2).contains(&point.0) && (rect.1..rect.3).contains(&point.1)
}

thread_local! {
    // All of this lives on the event-loop thread, which also runs the icon's window procedure.
    static ICON: RefCell<Option<Icon>> = const { RefCell::new(None) };
    static PANEL: RefCell<Option<Panel>> = const { RefCell::new(None) };
    /// Set while the panel window is created, for the window attributes hook.
    static CREATING_PANEL: Cell<bool> = const { Cell::new(false) };
    static CLOSED_AT: Cell<Option<Instant>> = const { Cell::new(None) };
    /// The open panel on screen (left, top, right, bottom), in physical pixels.
    static PANEL_RECT: Cell<(i32, i32, i32, i32)> = const { Cell::new((0, 0, 0, 0)) };
}

/// Selects Slint's backend with a hook that makes the panel a borderless,
/// always-on-top tool window with rounded corners and no taskbar button.
pub(crate) fn select_backend() -> Result<(), slint::PlatformError> {
    use winit::platform::windows::{CornerPreference, WindowAttributesExtWindows};
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .with_winit_window_attributes_hook(|attributes| {
            if !CREATING_PANEL.get() {
                return attributes;
            }
            attributes
                .with_decorations(false)
                .with_resizable(false)
                .with_window_level(winit::window::WindowLevel::AlwaysOnTop)
                .with_skip_taskbar(true)
                .with_undecorated_shadow(true)
                .with_corner_preference(CornerPreference::Round)
        })
        .select()
}

/// The tray picture as RGBA pixels, width and height.
pub(crate) fn tray_rgba() -> Option<(Vec<u8>, u32, u32)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!("../ui/tray.png")));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().ok()?;
    let mut pixels = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut pixels).ok()?;
    pixels.truncate(info.buffer_size());
    (info.color_type == png::ColorType::Rgba && info.bit_depth == png::BitDepth::Eight).then_some((
        pixels,
        info.width,
        info.height,
    ))
}

pub(crate) fn add_icon(
    rgba: &[u8],
    width: u32,
    height: u32,
    tip: &str,
) -> windows::core::Result<()> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: w!("ClaudeRpcTray"),
            ..Default::default()
        };
        RegisterClassW(&class);
        // A message-only window: it only receives the icon's mouse events.
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("ClaudeRpcTray"),
            PCWSTR::null(),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance.into()),
            None,
        )?;
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: icon_handle(rgba, width, height)?,
            ..Default::default()
        };
        write_tip(&mut data, tip);
        Shell_NotifyIconW(NIM_ADD, &data).ok()?;
        ICON.with(|slot| *slot.borrow_mut() = Some(Icon { hwnd, data }));
    }
    Ok(())
}

/// Event-loop thread only.
pub(crate) fn set_tip(tip: &str) {
    ICON.with(|slot| {
        if let Some(icon) = slot.borrow_mut().as_mut() {
            write_tip(&mut icon.data, tip);
            unsafe {
                let _ = Shell_NotifyIconW(NIM_MODIFY, &icon.data);
            }
        }
    });
}

pub(crate) fn shutdown() {
    if let Some(panel) = PANEL.with(|slot| slot.borrow_mut().take()) {
        let _ = panel.window.hide();
    }
    if let Some(icon) = ICON.with(|slot| slot.borrow_mut().take()) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &icon.data);
            let _ = DestroyIcon(icon.data.hIcon);
            let _ = DestroyWindow(icon.hwnd);
        }
    }
}

fn write_tip(data: &mut NOTIFYICONDATAW, tip: &str) {
    data.szTip = [0; 128];
    for (slot, unit) in data.szTip.iter_mut().zip(tip.encode_utf16().take(127)) {
        *slot = unit;
    }
}

fn icon_handle(
    rgba: &[u8],
    width: u32,
    height: u32,
) -> windows::core::Result<windows::Win32::UI::WindowsAndMessaging::HICON> {
    let bgra: Vec<u8> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|px| [px[2], px[1], px[0], px[3]])
        .collect();
    // The alpha channel draws the shape; the AND mask (1 bit per pixel, rows padded to 16 bits) stays clear.
    let mask = vec![0u8; width.div_ceil(16) as usize * 2 * height as usize];
    unsafe {
        CreateIcon(
            None,
            width as i32,
            height as i32,
            1,
            32,
            mask.as_ptr(),
            bgra.as_ptr(),
        )
    }
}

fn taskbar_created() -> u32 {
    static ID: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *ID.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_TRAY {
        // Slint work runs from its own event-loop turn, not inside this window procedure.
        match lparam.0 as u32 & 0xFFFF {
            WM_LBUTTONUP => {
                let _ = slint::invoke_from_event_loop(crate::open_settings);
            }
            WM_RBUTTONUP | WM_CONTEXTMENU => {
                let mut cursor = POINT::default();
                let _ = unsafe { GetCursorPos(&mut cursor) };
                let _ = slint::invoke_from_event_loop(move || toggle((cursor.x, cursor.y)));
            }
            _ => {}
        }
        return LRESULT(0);
    }
    if msg == taskbar_created() {
        // Explorer restarted and forgot the icon.
        ICON.with(|slot| {
            if let Some(icon) = slot.borrow().as_ref() {
                let _ = unsafe { Shell_NotifyIconW(NIM_ADD, &icon.data) };
            }
        });
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn toggle(cursor: (i32, i32)) {
    if PANEL.with(|slot| slot.borrow().is_some()) {
        close();
        return;
    }
    // A right-click on the icon first takes the focus from the open panel, which
    // closes it; that same click must not open it again.
    if CLOSED_AT
        .get()
        .is_some_and(|at| at.elapsed() < Duration::from_millis(400))
    {
        return;
    }
    open(cursor);
}

fn open(cursor: (i32, i32)) {
    CREATING_PANEL.set(true);
    let window = TrayPanel::new();
    CREATING_PANEL.set(false);
    let Ok(window) = window else {
        return;
    };
    crate::apply_theme(&window.global::<Theme>());
    wire(&window);
    window.window().on_winit_window_event(|_, event| {
        if matches!(event, winit::event::WindowEvent::Focused(false)) {
            close();
        }
        EventResult::Propagate
    });
    let tick = slint::Timer::default();
    tick.start(slint::TimerMode::Repeated, Duration::from_secs(1), refresh);
    let mut panel = Panel {
        window,
        cursor,
        height: 0.0,
        shown: None,
        _tick: tick,
        _click_away: None,
    };
    panel.update();
    let _ = panel.window.show();
    // The text wraps to the width set above, which changes the height: place again
    // now instead of at the next tick, or the panel opens partly below the screen.
    panel.place();
    panel._click_away = ClickAway::install();
    // Slint creates the real window when this event-loop turn ends, so the border
    // colour and the keyboard focus (for Escape) wait for it.
    let weak = panel.window.as_weak();
    let _ = slint::spawn_local(async move {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if let Ok(frame) = window.window().winit_window().await {
            crate::paint_frame(window.window(), &window.global::<Theme>());
            frame.focus_window();
        }
    });
    PANEL.with(|slot| *slot.borrow_mut() = Some(panel));
}

/// Closes the panel from a fresh event-loop turn: this often runs inside one of its own callbacks.
pub(crate) fn close() {
    slint::Timer::single_shot(Duration::ZERO, || {
        if let Some(panel) = PANEL.with(|slot| slot.borrow_mut().take()) {
            CLOSED_AT.set(Some(Instant::now()));
            let _ = panel.window.hide();
        }
    });
}

/// Shows the current status and settings if the panel is open.
pub(crate) fn refresh() {
    PANEL.with(|slot| {
        if let Some(panel) = slot.borrow_mut().as_mut() {
            panel.update();
        }
    });
}

fn wire(window: &TrayPanel) {
    window.on_toggle_pause(crate::toggle_pause);
    window.on_pause(|value| crate::pause(&value));
    window.on_set_mode(|mode| crate::set_mode(&mode));
    window.on_open_desktop(|| {
        close();
        let _ = launch::open_desktop();
    });
    window.on_open_code(|| {
        close();
        let _ = launch::open_code();
    });
    window.on_open_settings(|| {
        close();
        crate::open_settings();
    });
    window.on_update(|| {
        if crate::update_state().latest.is_some() {
            close();
            let _ = crate::open_link("latest");
        } else {
            crate::check_update(true);
            refresh();
        }
    });
    window.on_quit(crate::quit);
    window.on_dismiss(close);
}

impl Panel {
    fn update(&mut self) {
        let config = crate::read_config();
        let french = alerts::is_french(&config.language);
        let view = view::panel_view(
            &crate::read_status(),
            &config,
            &crate::update_state(),
            CHART_WIDTH,
            crate::now_ms(),
            view::local_offset_ms(),
            french,
        );
        let prefs = crate::read_prefs();
        let state = (
            view,
            prefs.tray_hidden.clone(),
            french,
            crate::DESKTOP_INSTALLED.load(Ordering::Relaxed),
            crate::CODE_INSTALLED.load(Ordering::Relaxed),
        );
        if self.shown.as_ref() != Some(&state) {
            let window = &self.window;
            let view = &state.0;
            window.global::<L>().set_french(french);
            window.set_show(crate::tray_shown(&prefs));
            window.set_desktop_installed(state.3);
            window.set_code_installed(state.4);
            window.set_model_line(view.model_line.as_str().into());
            window.set_session_line(view.session_line.as_str().into());
            window.set_client_on(view.client_on);
            window.set_client_label(view.client_label.as_str().into());
            window.set_has_hero(view.hero.is_some());
            if let Some(hero) = &view.hero {
                window.set_hero(crate::limit(hero));
            }
            window.set_plan(view.plan.as_str().into());
            window.set_alert(view.alert.as_str().into());
            window.set_alert_level(view.alert_level);
            let (line, area, peak) = view.spark.clone().unwrap_or_default();
            window.set_spark_line(line.into());
            window.set_spark_area(area.into());
            window.set_spark_peak(peak.into());
            window.set_limits(crate::limits(&view.limits));
            window.set_credits(view.credits.as_str().into());
            window.set_credits_percent(view.credits_percent);
            window.set_credits_level(view.credits_level);
            window.set_paused(view.paused);
            window.set_pause_state(view.pause_state.as_str().into());
            window.set_mode(view.mode.as_str().into());
            window.set_update_label(view.update_label.as_str().into());
            window.set_update_ready(view.update_ready);
            window.set_discord_pre(view.discord[0].as_str().into());
            window.set_discord_user(view.discord[1].as_str().into());
            window.set_discord_post(view.discord[2].as_str().into());
            self.shown = Some(state);
        }
        self.place();
    }

    /// Sizes the window to its content and anchors it next to the tray icon.
    fn place(&mut self) {
        let height = self.window.get_wanted_height();
        if height == self.height {
            return;
        }
        self.height = height;
        let (work, scale, edge) = monitor_work_area(self.cursor);
        // 12px off the taskbar and the screen edges, like Windows 11's own flyouts.
        let margin = (12.0 * scale).round() as i32;
        let size = (
            (PANEL_WIDTH * scale).round() as i32,
            // Never taller than the screen: the lowest rows are then cut off.
            ((height * scale).round() as i32).min(work.3 - work.1 - 2 * margin),
        );
        let (x, y) = panel_position(self.cursor, size, work, margin, edge);
        let window = self.window.window();
        window.set_size(slint::PhysicalSize::new(size.0 as u32, size.1 as u32));
        window.set_position(slint::PhysicalPosition::new(x, y));
        PANEL_RECT.set((x, y, x + size.0, y + size.1));
    }
}

/// The work area (left, top, right, bottom) of the monitor under `point`, its
/// scale factor, and the screen edge the taskbar sits on (`ABE_*`: 0 left,
/// 1 top, 2 right, 3 bottom).
fn monitor_work_area(point: (i32, i32)) -> ((i32, i32, i32, i32), f32, u32) {
    use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_GETTASKBARPOS, APPBARDATA};
    unsafe {
        let monitor = MonitorFromPoint(
            POINT {
                x: point.0,
                y: point.1,
            },
            MONITOR_DEFAULTTONEAREST,
        );
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work = if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let r = info.rcWork;
            (r.left, r.top, r.right, r.bottom)
        } else {
            (i32::MIN / 2, i32::MIN / 2, i32::MAX / 2, i32::MAX / 2)
        };
        let mut bar = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            ..Default::default()
        };
        // An auto-hiding taskbar slides away once the panel has the focus, so
        // no room is kept for it: the gap it would leave reads as a misplaced panel.
        let edge = if SHAppBarMessage(ABM_GETTASKBARPOS, &mut bar) != 0 {
            bar.uEdge
        } else {
            3
        };
        let (mut dpi_x, mut dpi_y) = (96, 96);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        (work, dpi_x as f32 / 96.0, edge)
    }
}

/// Top-left corner of a panel of `size` opened from a tray click at `cursor`.
/// It docks `margin` away from the taskbar's `edge` and is centred on the
/// cursor along it. The cursor's distance from the taskbar is ignored: an icon
/// in the hidden-icons flyout sits well above the taskbar, and anchoring to
/// it would leave the panel hanging in mid-screen.
fn panel_position(
    cursor: (i32, i32),
    size: (i32, i32),
    work: (i32, i32, i32, i32),
    margin: i32,
    edge: u32,
) -> (i32, i32) {
    let (left, top) = (work.0 + margin, work.1 + margin);
    let right = (work.2 - margin - size.0).max(left);
    let bottom = (work.3 - margin - size.1).max(top);
    let x = (cursor.0 - size.0 / 2).clamp(left, right);
    let y = (cursor.1 - size.1 / 2).clamp(top, bottom);
    match edge {
        0 => (left, y),
        1 => (x, top),
        2 => (right, y),
        _ => (x, bottom),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_floats_next_to_the_taskbar_on_every_edge() {
        let size = (480, 600);
        // Bottom taskbar: 12px above it, centred on the icon.
        assert_eq!(
            panel_position((1500, 1060), size, (0, 0, 1920, 1032), 12, 3),
            (1260, 420)
        );
        // Icon in the hidden-icons flyout, 150px above the taskbar: still docked.
        assert_eq!(
            panel_position((1800, 880), size, (0, 0, 1920, 1032), 12, 3),
            (1428, 420)
        );
        // Top taskbar: 12px below it.
        assert_eq!(
            panel_position((1500, 20), size, (0, 48, 1920, 1080), 12, 1),
            (1260, 60)
        );
        // Left and right taskbars: 12px beside them.
        assert_eq!(
            panel_position((30, 500), size, (60, 0, 1920, 1080), 12, 0),
            (72, 200)
        );
        assert_eq!(
            panel_position((1900, 1070), size, (0, 0, 1860, 1080), 12, 2),
            (1368, 468)
        );
        // A monitor left of the main one has negative coordinates.
        assert_eq!(
            panel_position((-50, 1060), size, (-1920, 0, 0, 1040), 12, 3),
            (-492, 428)
        );
    }

    #[test]
    fn a_click_outside_the_panel_is_told_apart_from_one_inside() {
        let rect = (1260, 420, 1740, 1020);
        assert!(rect_contains(rect, (1260, 420)));
        assert!(rect_contains(rect, (1739, 1019)));
        // The right and bottom edges are the first pixels outside.
        assert!(!rect_contains(rect, (1740, 700)));
        assert!(!rect_contains(rect, (1500, 1020)));
        assert!(!rect_contains(rect, (1259, 700)));
        assert!(!rect_contains(rect, (30, 1430)));
    }

    #[test]
    fn tray_picture_decodes_to_rgba() {
        let (pixels, width, height) = tray_rgba().unwrap();
        assert_eq!(pixels.len(), (width * height * 4) as usize);
    }
}
