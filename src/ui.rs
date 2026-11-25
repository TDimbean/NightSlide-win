use crate::control::{Controller, MAX_STRENGTH_TENTHS};
use std::cell::RefCell;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, POINT, RECT,
};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WIDTH: i32 = 390;
const HEIGHT: i32 = 74;
const TRACK_LEFT: i32 = 62;
const TRACK_RIGHT: i32 = 328;
const TRACK_Y: i32 = 37;
// COLORREF is 0x00BBGGRR. This is RGB(112, 14, 38).
const WINE: u32 = 0x00260e70;
const WINE_DARK: u32 = 0x001a0a43;
const WHITE: u32 = 0x00ffffff;
const TRAY_MESSAGE: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;
const CMD_OPEN: usize = 100;
const CMD_EXIT: usize = 101;
const KEY_REPEAT_TIMER: usize = 1;
const INSTANCE_NAME: &str = "Local\\NightSlide.Tray.9F4F9F78-6C0B-4B19-8852-2CF85E85051D";
const SMOKE_INSTANCE_NAME: &str = "Local\\NightSlide.Smoke.9F4F9F78-6C0B-4B19-8852-2CF85E85051D";

fn isolated_smoke() -> bool {
    std::env::var_os("NIGHTSLIDE_TEST_ISOLATED").is_some()
}

fn host_class() -> &'static str {
    if isolated_smoke() {
        "NightSlideHostSmoke"
    } else {
        "NightSlideHost"
    }
}

fn popup_class() -> &'static str {
    if isolated_smoke() {
        "NightSlidePopupSmoke"
    } else {
        "NightSlidePopup"
    }
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

struct App {
    host: HWND,
    popup: HWND,
    icon: HICON,
    tray_added: bool,
    control: Controller,
    dragging: bool,
    keyboard_adjusting: bool,
    held_arrow: Option<u16>,
    modifier_repeat: bool,
    grab_cursor: HCURSOR,
    owns_grab_cursor: bool,
}

struct Instance(HANDLE);

impl Instance {
    fn acquire() -> Result<Option<Self>, String> {
        let name = wide(if isolated_smoke() {
            SMOKE_INSTANCE_NAME
        } else {
            INSTANCE_NAME
        });
        // SAFETY: Name is NUL-terminated; no security attributes or initial ownership are needed.
        let handle = unsafe { CreateMutexW(null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(format!(
                "Could not reserve the tray instance: {}",
                std::io::Error::last_os_error()
            ));
        }
        // GetLastError must immediately follow CreateMutexW to identify an existing object.
        let already_running = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        let class = wide(host_class());
        let title = wide("NightSlide");
        // Also detects a tray process built before the mutex was introduced.
        let host = unsafe { FindWindowW(class.as_ptr(), title.as_ptr()) };
        if already_running || !host.is_null() {
            if !host.is_null() {
                let mut process_id = 0;
                // SAFETY: The window belongs to the existing tray process.
                unsafe {
                    GetWindowThreadProcessId(host, &mut process_id);
                    AllowSetForegroundWindow(process_id);
                    PostMessageW(host, TRAY_MESSAGE, 0, WM_LBUTTONUP as isize);
                }
            }
            unsafe { CloseHandle(handle) };
            return Ok(None);
        }
        Ok(Some(Self(handle)))
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: This handle came from CreateMutexW and is owned by this process.
        unsafe { CloseHandle(self.0) };
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn with_app(action: impl FnOnce(&mut App)) {
    APP.with(|cell| {
        if let Ok(mut app) = cell.try_borrow_mut()
            && let Some(app) = app.as_mut()
        {
            action(app);
        }
    });
}

pub fn fatal(message: &str) {
    let title = wide("NightSlide");
    let message = wide(message);
    // SAFETY: Both strings are terminated and stay alive for the call.
    unsafe {
        MessageBoxW(
            null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_ICONERROR | MB_OK,
        )
    };
}

pub fn run() -> Result<(), String> {
    let Some(_instance) = Instance::acquire()? else {
        return Ok(());
    };
    // SAFETY: Null requests the current executable module.
    let instance = unsafe { GetModuleHandleW(null()) };
    if instance.is_null() {
        return Err("Could not find the application module".into());
    }
    let host_name = wide(host_class());
    let popup_name = wide(popup_class());
    let mut class = WNDCLASSW {
        hInstance: instance,
        lpfnWndProc: Some(host_proc),
        lpszClassName: host_name.as_ptr(),
        ..Default::default()
    };
    // SAFETY: Class names and callbacks stay alive through registration.
    if unsafe { RegisterClassW(&class) } == 0 {
        return Err("Could not register the tray window".into());
    }
    class.lpfnWndProc = Some(popup_proc);
    class.lpszClassName = popup_name.as_ptr();
    if unsafe { RegisterClassW(&class) } == 0 {
        return Err("Could not register the popup window".into());
    }

    let title = wide("NightSlide");
    // SAFETY: Registered classes and valid module are passed to CreateWindowExW.
    let host = unsafe {
        CreateWindowExW(
            0,
            host_name.as_ptr(),
            title.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            null(),
        )
    };
    if host.is_null() {
        return Err("Could not create the tray window".into());
    }
    let popup = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            popup_name.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            0,
            0,
            WIDTH,
            HEIGHT,
            host,
            null_mut(),
            instance,
            null(),
        )
    };
    if popup.is_null() {
        unsafe { DestroyWindow(host) };
        return Err("Could not create the slider widget".into());
    }
    // Resource 1 is the bundled user-provided .ico (also used by Explorer).
    let icon =
        unsafe { LoadImageW(instance, 1usize as _, IMAGE_ICON, 32, 32, LR_DEFAULTCOLOR) as HICON };
    if icon.is_null() {
        unsafe { DestroyWindow(host) };
        return Err("Could not create the tray icon".into());
    }
    let tray = tray_data(host, icon);
    let grab_cursor = make_grab_cursor(instance);
    let owns_grab_cursor = !grab_cursor.is_null();
    let grab_cursor = if owns_grab_cursor {
        grab_cursor
    } else {
        unsafe { LoadCursorW(null_mut(), IDC_SIZEWE) }
    };
    // SAFETY: The tray data is initialized and lives through this call.
    let tray_added = std::env::var_os("NIGHTSLIDE_TEST_NO_TRAY").is_none();
    if tray_added && unsafe { Shell_NotifyIconW(NIM_ADD, &tray) } == 0 {
        let source = std::io::Error::last_os_error();
        unsafe {
            if owns_grab_cursor {
                DestroyCursor(grab_cursor);
            }
            DestroyIcon(icon);
            DestroyWindow(host)
        };
        return Err(format!(
            "Could not add the notification area icon: {source}"
        ));
    }
    APP.with(|cell| {
        *cell.borrow_mut() = Some(App {
            host,
            popup,
            icon,
            tray_added,
            control: Controller::new(),
            dragging: false,
            keyboard_adjusting: false,
            held_arrow: None,
            modifier_repeat: false,
            grab_cursor,
            owns_grab_cursor,
        })
    });

    // SAFETY: Standard Win32 message loop. GetMessageW returns 0 on WM_QUIT.
    unsafe {
        let mut message = MSG::default();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    APP.with(|cell| *cell.borrow_mut() = None);
    Ok(())
}

fn tray_data(host: HWND, icon: HICON) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: host,
        uID: TRAY_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: TRAY_MESSAGE,
        hIcon: icon,
        ..Default::default()
    };
    let tip = wide("NightSlide · Night Light strength");
    data.szTip[..tip.len()].copy_from_slice(&tip);
    data
}

fn report_error(app: &App, error: &str) {
    if !app.tray_added {
        return;
    }
    let mut data = tray_data(app.host, app.icon);
    data.uFlags = NIF_INFO;
    data.dwInfoFlags = NIIF_ERROR;
    let title = wide("NightSlide");
    data.szInfoTitle[..title.len()].copy_from_slice(&title);
    let message = wide(error);
    let len = message.len().min(data.szInfo.len() - 1);
    data.szInfo[..len].copy_from_slice(&message[..len]);
    // SAFETY: The icon is registered and the notification buffers are NUL-terminated.
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
}

fn hand_pixel(x: i32, y: i32) -> bool {
    (12..=21).contains(&x) && (17..=25).contains(&y)
        || (8..=12).contains(&x) && (15..=21).contains(&y)
        || (10..=13).contains(&x) && (10..=18).contains(&y)
        || (14..=17).contains(&x) && (8..=18).contains(&y)
        || (18..=21).contains(&x) && (9..=18).contains(&y)
        || (22..=25).contains(&x) && (12..=20).contains(&y)
}

fn make_grab_cursor(instance: windows_sys::Win32::Foundation::HINSTANCE) -> HCURSOR {
    let mut and_mask = [0xffu8; 32 * 4];
    let mut xor_mask = [0u8; 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            if hand_pixel(x, y) {
                let index = y as usize * 4 + x as usize / 8;
                let bit = 0x80 >> (x % 8);
                and_mask[index] &= !bit;
                let outline = !hand_pixel(x - 1, y)
                    || !hand_pixel(x + 1, y)
                    || !hand_pixel(x, y - 1)
                    || !hand_pixel(x, y + 1);
                if !outline {
                    xor_mask[index] |= bit;
                }
            }
        }
    }
    // SAFETY: Both masks have four bytes per row for a 32-by-32 monochrome cursor.
    unsafe {
        CreateCursor(
            instance,
            16,
            16,
            32,
            32,
            and_mask.as_ptr().cast(),
            xor_mask.as_ptr().cast(),
        )
    }
}

unsafe extern "system" fn host_proc(hwnd: HWND, msg: u32, w: usize, l: isize) -> isize {
    match msg {
        TRAY_MESSAGE => {
            match l as u32 {
                WM_LBUTTONUP => with_app(show_popup),
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            match w & 0xffff {
                CMD_OPEN => with_app(show_popup),
                CMD_EXIT => {
                    unsafe { DestroyWindow(hwnd) };
                }
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            with_app(|app| {
                let _ = app.control.finish_drag();
                if app.tray_added {
                    let data = tray_data(app.host, app.icon);
                    unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
                }
                unsafe { DestroyIcon(app.icon) };
                if app.owns_grab_cursor {
                    unsafe { DestroyCursor(app.grab_cursor) };
                }
            });
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

fn show_menu(host: HWND) {
    // SAFETY: Created menu and point remain valid through TrackPopupMenu.
    unsafe {
        let mut point = POINT::default();
        GetCursorPos(&mut point);
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        let open = wide("Open NightSlide");
        let exit = wide("Exit");
        AppendMenuW(menu, MF_STRING, CMD_OPEN, open.as_ptr());
        AppendMenuW(menu, MF_STRING, CMD_EXIT, exit.as_ptr());
        SetForegroundWindow(host);
        TrackPopupMenu(menu, TPM_RIGHTBUTTON, point.x, point.y, 0, host, null());
        DestroyMenu(menu);
        PostMessageW(host, WM_NULL, 0, 0);
    }
}

fn show_popup(app: &mut App) {
    if let Err(error) = app.control.refresh() {
        report_error(app, &error);
    }
    app.keyboard_adjusting = false;
    app.held_arrow = None;
    app.modifier_repeat = false;
    let mut icon_rect = RECT::default();
    let identifier = NOTIFYICONIDENTIFIER {
        cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
        hWnd: app.host,
        uID: TRAY_ID,
        ..Default::default()
    };
    // SAFETY: Identifier and output rectangle are valid.
    if unsafe { Shell_NotifyIconGetRect(&identifier, &mut icon_rect) } != 0 {
        let mut cursor = POINT::default();
        unsafe { GetCursorPos(&mut cursor) };
        icon_rect = RECT {
            left: cursor.x,
            right: cursor.x + 1,
            top: cursor.y,
            bottom: cursor.y + 1,
        };
    }
    let center = POINT {
        x: (icon_rect.left + icon_rect.right) / 2,
        y: (icon_rect.top + icon_rect.bottom) / 2,
    };
    let mut monitor = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: The monitor comes from the icon position.
    unsafe {
        GetMonitorInfoW(
            MonitorFromPoint(center, MONITOR_DEFAULTTONEAREST),
            &mut monitor,
        )
    };
    let work = monitor.rcWork;
    let x = (center.x - WIDTH / 2).clamp(work.left, (work.right - WIDTH).max(work.left));
    let above = icon_rect.top - HEIGHT - 10;
    let y = if above >= work.top {
        above
    } else {
        icon_rect.bottom + 10
    }
    .clamp(work.top, (work.bottom - HEIGHT).max(work.top));
    // SAFETY: Popup is a valid top-level window, and this action activates keyboard input.
    unsafe {
        SetWindowPos(app.popup, HWND_TOPMOST, x, y, WIDTH, HEIGHT, SWP_SHOWWINDOW);
        SetForegroundWindow(app.popup);
        SetFocus(app.popup);
        InvalidateRect(app.popup, null(), 0);
    }
}

fn hide_popup(app: &mut App) {
    app.keyboard_adjusting = false;
    app.held_arrow = None;
    app.modifier_repeat = false;
    unsafe { KillTimer(app.popup, KEY_REPEAT_TIMER) };
    if app.dragging {
        app.dragging = false;
        unsafe { ReleaseCapture() };
    }
    if let Err(error) = app.control.finish_drag() {
        report_error(app, &error);
    }
    unsafe { ShowWindow(app.popup, SW_HIDE) };
}

fn apply(app: &mut App, strength: i32, dragging: bool, force: bool) {
    if let Err(error) = app.control.set_strength(strength, dragging, force) {
        report_error(app, &error);
    }
    unsafe { InvalidateRect(app.popup, null(), 0) };
}

fn point_x(l: isize) -> i32 {
    (l as u16) as i16 as i32
}
fn point_y(l: isize) -> i32 {
    ((l >> 16) as u16) as i16 as i32
}
fn slider_value(x: i32) -> i32 {
    ((x - TRACK_LEFT) * MAX_STRENGTH_TENTHS / (TRACK_RIGHT - TRACK_LEFT))
        .clamp(0, MAX_STRENGTH_TENTHS)
}

fn keyboard_target(current: i32, key: u16, shift: bool, control: bool) -> Option<i32> {
    let direction = match key {
        VK_LEFT | VK_DOWN => -1,
        VK_RIGHT | VK_UP => 1,
        VK_HOME => return Some(0),
        VK_END => return Some(MAX_STRENGTH_TENTHS),
        _ => return None,
    };
    let value = if shift {
        if direction < 0 {
            0
        } else {
            MAX_STRENGTH_TENTHS
        }
    } else {
        current + direction * if control { 1 } else { 10 }
    };
    Some(value.clamp(0, MAX_STRENGTH_TENTHS))
}

fn is_arrow(key: u16) -> bool {
    matches!(key, VK_LEFT | VK_RIGHT | VK_DOWN | VK_UP)
}

fn held_arrow_target(current: i32, held: Option<u16>, shift: bool, control: bool) -> Option<i32> {
    held.and_then(|key| keyboard_target(current, key, shift, control))
}

fn key_is_down(key: u16) -> bool {
    unsafe { (GetKeyState(i32::from(key)) as u16 & 0x8000) != 0 }
}

fn slider_hit(x: i32, y: i32) -> bool {
    (TRACK_LEFT - 10..=TRACK_RIGHT + 10).contains(&x) && (TRACK_Y - 20..=TRACK_Y + 20).contains(&y)
}

fn set_widget_cursor(hwnd: HWND, app: &App) {
    // SAFETY: The cursor position is converted into this popup's client coordinates.
    unsafe {
        let cursor = if app.dragging {
            app.grab_cursor
        } else {
            let mut point = POINT::default();
            GetCursorPos(&mut point);
            ScreenToClient(hwnd, &mut point);
            if slider_hit(point.x, point.y) {
                LoadCursorW(null_mut(), IDC_HAND)
            } else {
                LoadCursorW(null_mut(), IDC_ARROW)
            }
        };
        SetCursor(cursor);
    }
}

unsafe extern "system" fn popup_proc(hwnd: HWND, msg: u32, w: usize, l: isize) -> isize {
    match msg {
        WM_PAINT => {
            with_app(|app| paint(hwnd, app));
            0
        }
        WM_ERASEBKGND => 1,
        WM_SETCURSOR => {
            with_app(|app| set_widget_cursor(hwnd, app));
            1
        }
        WM_ACTIVATE if (w as u32 & 0xffff) == WA_INACTIVE => {
            with_app(hide_popup);
            0
        }
        WM_KEYDOWN => {
            let key = w as u16;
            with_app(|app| match key {
                VK_ESCAPE => hide_popup(app),
                VK_TAB => {}
                VK_SHIFT | VK_CONTROL | VK_LSHIFT | VK_RSHIFT | VK_LCONTROL | VK_RCONTROL => {
                    // The arrow's OS repeat can stop when a modifier is pressed later.
                    if l & (1 << 30) == 0
                        && let Some(target) = held_arrow_target(
                            app.control.strength_tenths,
                            app.held_arrow,
                            key_is_down(VK_SHIFT)
                                || matches!(key, VK_SHIFT | VK_LSHIFT | VK_RSHIFT),
                            key_is_down(VK_CONTROL)
                                || matches!(key, VK_CONTROL | VK_LCONTROL | VK_RCONTROL),
                        )
                    {
                        app.modifier_repeat = true;
                        unsafe { SetTimer(hwnd, KEY_REPEAT_TIMER, 40, None) };
                        apply(app, target, true, false);
                    }
                }
                VK_SPACE | VK_RETURN => {
                    app.keyboard_adjusting = false;
                    if let Err(error) = app.control.finish_drag() {
                        report_error(app, &error);
                    }
                    apply(app, app.control.strength_tenths, false, true);
                }
                _ => {
                    if is_arrow(key) && app.held_arrow != Some(key) {
                        app.held_arrow = Some(key);
                        app.modifier_repeat = false;
                        unsafe { KillTimer(hwnd, KEY_REPEAT_TIMER) };
                    }
                    if (!is_arrow(key) || !app.modifier_repeat)
                        && let Some(target) = keyboard_target(
                            app.control.strength_tenths,
                            key,
                            key_is_down(VK_SHIFT),
                            key_is_down(VK_CONTROL),
                        )
                    {
                        app.keyboard_adjusting = true;
                        apply(app, target, true, false);
                    }
                }
            });
            // Paint synchronously after releasing APP's mutable borrow. Repeated
            // WM_KEYDOWN messages otherwise keep WM_PAINT queued until key release.
            unsafe { UpdateWindow(hwnd) };
            0
        }
        WM_KEYUP => {
            if is_arrow(w as u16) || matches!(w as u16, VK_HOME | VK_END) {
                with_app(|app| {
                    if app.held_arrow == Some(w as u16) {
                        app.held_arrow = None;
                        app.modifier_repeat = false;
                        unsafe { KillTimer(hwnd, KEY_REPEAT_TIMER) };
                    }
                    if app.keyboard_adjusting && app.held_arrow.is_none() {
                        app.keyboard_adjusting = false;
                        if !app.dragging
                            && let Err(error) = app.control.finish_drag()
                        {
                            report_error(app, &error);
                        }
                    }
                });
            }
            0
        }
        WM_TIMER if w == KEY_REPEAT_TIMER => {
            with_app(|app| {
                if app.modifier_repeat
                    && let Some(target) = held_arrow_target(
                        app.control.strength_tenths,
                        app.held_arrow,
                        key_is_down(VK_SHIFT),
                        key_is_down(VK_CONTROL),
                    )
                {
                    apply(app, target, true, false);
                }
            });
            unsafe { UpdateWindow(hwnd) };
            0
        }
        WM_LBUTTONDOWN => {
            with_app(|app| {
                let x = point_x(l);
                let y = point_y(l);
                if slider_hit(x, y) {
                    app.dragging = true;
                    unsafe { SetCapture(hwnd) };
                    apply(app, slider_value(x), true, true);
                    set_widget_cursor(hwnd, app);
                }
            });
            0
        }
        WM_MOUSEMOVE => {
            with_app(|app| {
                if app.dragging {
                    apply(app, slider_value(point_x(l)), true, false);
                }
                set_widget_cursor(hwnd, app);
            });
            unsafe { UpdateWindow(hwnd) };
            0
        }
        WM_LBUTTONUP => {
            with_app(|app| {
                if app.dragging {
                    apply(app, slider_value(point_x(l)), true, true);
                    app.dragging = false;
                    unsafe { ReleaseCapture() };
                    if let Err(error) = app.control.finish_drag() {
                        report_error(app, &error);
                    }
                    set_widget_cursor(hwnd, app);
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                }
            });
            0
        }
        WM_CAPTURECHANGED => {
            with_app(|app| {
                if app.dragging {
                    app.dragging = false;
                    if let Err(error) = app.control.finish_drag() {
                        report_error(app, &error);
                    }
                }
            });
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

fn paint(hwnd: HWND, app: &App) {
    // SAFETY: The popup is being painted; selected objects are restored before deletion.
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let target = BeginPaint(hwnd, &mut ps);
        if target.is_null() {
            return;
        }
        let memory = CreateCompatibleDC(target);
        let bitmap = CreateCompatibleBitmap(target, WIDTH, HEIGHT);
        if memory.is_null() || bitmap.is_null() {
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            if !memory.is_null() {
                DeleteDC(memory);
            }
            EndPaint(hwnd, &ps);
            return;
        }
        let old_bitmap = SelectObject(memory, bitmap);
        let old_pen = SelectObject(memory, GetStockObject(NULL_PEN));
        fill(memory, 0, 0, WIDTH, HEIGHT, WINE);

        sun(memory, 29, TRACK_Y);
        moon(memory, 359, TRACK_Y);

        fill(
            memory,
            TRACK_LEFT,
            TRACK_Y - 4,
            TRACK_RIGHT + 1,
            TRACK_Y + 4,
            WINE_DARK,
        );
        let thumb_x = TRACK_LEFT
            + app.control.strength_tenths * (TRACK_RIGHT - TRACK_LEFT) / MAX_STRENGTH_TENTHS;
        if thumb_x > TRACK_LEFT {
            fill(memory, TRACK_LEFT, TRACK_Y - 4, thumb_x, TRACK_Y + 4, WHITE);
        }
        fill(
            memory,
            thumb_x - 5,
            TRACK_Y - 16,
            thumb_x + 6,
            TRACK_Y + 16,
            WHITE,
        );

        BitBlt(target, 0, 0, WIDTH, HEIGHT, memory, 0, 0, SRCCOPY);
        SelectObject(memory, old_pen);
        SelectObject(memory, old_bitmap);
        DeleteObject(bitmap);
        DeleteDC(memory);
        EndPaint(hwnd, &ps);
    }
}

unsafe fn fill(dc: HDC, left: i32, top: i32, right: i32, bottom: i32, color: u32) {
    let brush = unsafe { CreateSolidBrush(color) };
    unsafe {
        FillRect(
            dc,
            &RECT {
                left,
                top,
                right,
                bottom,
            },
            brush,
        );
        DeleteObject(brush);
    }
}

unsafe fn circle(dc: HDC, left: i32, top: i32, right: i32, bottom: i32, color: u32) {
    let brush = unsafe { CreateSolidBrush(color) };
    let old = unsafe { SelectObject(dc, brush) };
    unsafe {
        Ellipse(dc, left, top, right, bottom);
        SelectObject(dc, old);
        DeleteObject(brush);
    }
}

unsafe fn sun(dc: HDC, x: i32, y: i32) {
    unsafe {
        circle(dc, x - 7, y - 7, x + 7, y + 7, WHITE);
        let pen = CreatePen(PS_SOLID, 2, WHITE);
        let old = SelectObject(dc, pen);
        for (dx1, dy1, dx2, dy2) in [
            (0, -17, 0, -11),
            (0, 11, 0, 17),
            (-17, 0, -11, 0),
            (11, 0, 17, 0),
            (-12, -12, -9, -9),
            (9, 9, 12, 12),
            (12, -12, 9, -9),
            (-9, 9, -12, 12),
        ] {
            MoveToEx(dc, x + dx1, y + dy1, null_mut());
            LineTo(dc, x + dx2, y + dy2);
        }
        SelectObject(dc, old);
        DeleteObject(pen);
    }
}

unsafe fn moon(dc: HDC, x: i32, y: i32) {
    unsafe {
        circle(dc, x - 10, y - 10, x + 10, y + 10, WHITE);
        circle(dc, x - 3, y - 14, x + 14, y + 6, WINE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_steps_snap_and_fine_adjust() {
        assert_eq!(keyboard_target(500, VK_RIGHT, false, false), Some(510));
        assert_eq!(keyboard_target(500, VK_LEFT, false, false), Some(490));
        assert_eq!(keyboard_target(500, VK_RIGHT, false, true), Some(501));
        assert_eq!(keyboard_target(500, VK_LEFT, false, true), Some(499));
        assert_eq!(keyboard_target(500, VK_RIGHT, true, false), Some(1000));
        assert_eq!(keyboard_target(500, VK_LEFT, true, false), Some(0));
        assert_eq!(keyboard_target(500, VK_RIGHT, true, true), Some(1000));
        assert_eq!(keyboard_target(0, VK_LEFT, false, true), Some(0));
        assert_eq!(keyboard_target(1000, VK_RIGHT, false, true), Some(1000));
        let after_right = keyboard_target(500, VK_RIGHT, false, false).unwrap();
        assert_eq!(
            held_arrow_target(after_right, Some(VK_RIGHT), false, true),
            Some(511)
        );
        assert_eq!(
            held_arrow_target(after_right, Some(VK_RIGHT), true, false),
            Some(1000)
        );
        assert_eq!(
            held_arrow_target(after_right, Some(VK_LEFT), true, false),
            Some(0)
        );
        assert_eq!(held_arrow_target(after_right, None, true, false), None);
    }

    #[test]
    fn grabbing_cursor_can_be_created() {
        let instance = unsafe { GetModuleHandleW(null()) };
        let cursor = make_grab_cursor(instance);
        assert!(!cursor.is_null());
        unsafe { DestroyCursor(cursor) };
    }
}
