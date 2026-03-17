use codex_core::features::Feature;
use codex_core::features::Features;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WindowAttentionPolicy {
    pub focus_terminal_window: bool,
    pub move_terminal_window_to_primary_monitor: bool,
}

impl WindowAttentionPolicy {
    pub(crate) fn from_features(features: &Features) -> Self {
        Self {
            focus_terminal_window: features.enabled(Feature::FocusTerminalWindow),
            move_terminal_window_to_primary_monitor: features
                .enabled(Feature::MoveTerminalWindowToPrimaryMonitor),
        }
    }

    pub(crate) fn enabled(self) -> bool {
        self.focus_terminal_window || self.move_terminal_window_to_primary_monitor
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WindowAttentionOutcome {
    pub moved: bool,
    pub focused: bool,
    pub flashed: bool,
}

impl WindowAttentionOutcome {
    pub(crate) fn changed(self) -> bool {
        self.moved || self.focused || self.flashed
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FocusOutcome {
    focused: bool,
    flashed: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WindowAttentionController;

impl WindowAttentionController {
    pub(crate) fn apply(self, policy: WindowAttentionPolicy) -> WindowAttentionOutcome {
        if !policy.enabled() {
            return WindowAttentionOutcome::default();
        }
        apply_window_attention(&mut OsWindowOps, policy)
    }
}

trait WindowOps {
    type Handle: Copy;

    fn resolve_host_window(&mut self) -> Option<Self::Handle>;

    fn move_to_primary_monitor(&mut self, handle: Self::Handle) -> std::io::Result<bool>;

    fn focus_window(&mut self, handle: Self::Handle) -> std::io::Result<FocusOutcome>;
}

fn apply_window_attention<T: WindowOps>(
    ops: &mut T,
    policy: WindowAttentionPolicy,
) -> WindowAttentionOutcome {
    let Some(handle) = ops.resolve_host_window() else {
        return WindowAttentionOutcome::default();
    };

    let mut outcome = WindowAttentionOutcome::default();

    if policy.move_terminal_window_to_primary_monitor {
        match ops.move_to_primary_monitor(handle) {
            Ok(moved) => outcome.moved = moved,
            Err(err) => tracing::warn!(error = %err, "failed to move terminal window"),
        }
    }

    if policy.focus_terminal_window {
        match ops.focus_window(handle) {
            Ok(focus_outcome) => {
                outcome.focused = focus_outcome.focused;
                outcome.flashed = focus_outcome.flashed;
            }
            Err(err) => tracing::warn!(error = %err, "failed to focus terminal window"),
        }
    }

    outcome
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RectBounds {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl RectBounds {
    fn width(self) -> i32 {
        (self.right - self.left).max(0)
    }

    fn height(self) -> i32 {
        (self.bottom - self.top).max(0)
    }

    fn has_area(self) -> bool {
        self.width() > 0 && self.height() > 0
    }
}

fn center_rect_in_work_area(window: RectBounds, work_area: RectBounds) -> RectBounds {
    let width = window.width().min(work_area.width()).max(0);
    let height = window.height().min(work_area.height()).max(0);
    let left = work_area.left + (work_area.width() - width) / 2;
    let top = work_area.top + (work_area.height() - height) / 2;
    RectBounds {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

struct OsWindowOps;

#[cfg(not(windows))]
impl WindowOps for OsWindowOps {
    type Handle = ();

    fn resolve_host_window(&mut self) -> Option<Self::Handle> {
        None
    }

    fn move_to_primary_monitor(&mut self, _handle: Self::Handle) -> std::io::Result<bool> {
        Ok(false)
    }

    fn focus_window(&mut self, _handle: Self::Handle) -> std::io::Result<FocusOutcome> {
        Ok(FocusOutcome::default())
    }
}

#[cfg(windows)]
mod os {
    use super::center_rect_in_work_area;
    use super::FocusOutcome;
    use super::OsWindowOps;
    use super::RectBounds;
    use super::WindowOps;
    use std::ffi::OsString;
    use std::io;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::GetMonitorInfoW;
    use windows_sys::Win32::Graphics::Gdi::MONITORINFO;
    use windows_sys::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST;
    use windows_sys::Win32::Graphics::Gdi::MONITOR_DEFAULTTOPRIMARY;
    use windows_sys::Win32::Graphics::Gdi::MonitorFromPoint;
    use windows_sys::Win32::Graphics::Gdi::MonitorFromWindow;
    use windows_sys::Win32::System::Console::GetConsoleWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::FLASHW_TRAY;
    use windows_sys::Win32::UI::WindowsAndMessaging::FLASHWINFO;
    use windows_sys::Win32::UI::WindowsAndMessaging::FlashWindowEx;
    use windows_sys::Win32::UI::WindowsAndMessaging::GA_ROOTOWNER;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetAncestor;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
    use windows_sys::Win32::UI::WindowsAndMessaging::IsIconic;
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible;
    use windows_sys::Win32::UI::WindowsAndMessaging::IsZoomed;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_RESTORE;
    use windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE;
    use windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOOWNERZORDER;
    use windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOZORDER;
    use windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::SetWindowPos;
    use windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow;

    impl WindowOps for OsWindowOps {
        type Handle = HWND;

        fn resolve_host_window(&mut self) -> Option<Self::Handle> {
            let console = unsafe { GetConsoleWindow() };
            if console == 0 {
                return None;
            }

            let root_owner = unsafe { GetAncestor(console, GA_ROOTOWNER) };
            [root_owner, console]
                .into_iter()
                .find(|handle| is_candidate_window(*handle))
        }

        fn move_to_primary_monitor(&mut self, handle: Self::Handle) -> io::Result<bool> {
            let primary_monitor =
                unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
            if primary_monitor == 0 {
                return Err(io::Error::last_os_error());
            }

            let current_monitor = unsafe { MonitorFromWindow(handle, MONITOR_DEFAULTTONEAREST) };
            if current_monitor == primary_monitor {
                return Ok(false);
            }

            restore_if_needed(handle);
            let current_rect = get_window_rect(handle)?;
            if !current_rect.has_area() {
                return Ok(false);
            }

            let work_area = monitor_work_area(primary_monitor)?;
            let centered = center_rect_in_work_area(current_rect, work_area);
            let width = centered.width();
            let height = centered.height();
            if width <= 0 || height <= 0 {
                return Ok(false);
            }

            let moved = unsafe {
                SetWindowPos(
                    handle,
                    0,
                    centered.left,
                    centered.top,
                    width,
                    height,
                    SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE,
                )
            };
            if moved == 0 {
                return Err(io::Error::last_os_error());
            }

            Ok(true)
        }

        fn focus_window(&mut self, handle: Self::Handle) -> io::Result<FocusOutcome> {
            restore_if_needed(handle);

            let focused = unsafe { SetForegroundWindow(handle) } != 0;
            if focused {
                return Ok(FocusOutcome {
                    focused: true,
                    flashed: false,
                });
            }

            let flash_info = FLASHWINFO {
                cbSize: size_of::<FLASHWINFO>() as u32,
                hwnd: handle,
                dwFlags: FLASHW_TRAY,
                uCount: 3,
                dwTimeout: 0,
            };
            let flashed = unsafe { FlashWindowEx(&flash_info) } != 0;
            Ok(FocusOutcome {
                focused: false,
                flashed,
            })
        }
    }

    fn is_candidate_window(handle: HWND) -> bool {
        if handle == 0 {
            return false;
        }
        if unsafe { IsWindowVisible(handle) } == 0 {
            return false;
        }
        if desktop_window_class(handle).as_deref() == Some("#32769") {
            return false;
        }
        get_window_rect(handle).is_ok_and(RectBounds::has_area)
    }

    fn desktop_window_class(handle: HWND) -> Option<String> {
        let mut buf = [0u16; 256];
        let len = unsafe { GetClassNameW(handle, buf.as_mut_ptr(), buf.len() as i32) };
        if len <= 0 {
            return None;
        }
        Some(
            OsString::from_wide(&buf[..len as usize])
                .to_string_lossy()
                .into_owned(),
        )
    }

    fn restore_if_needed(handle: HWND) {
        if unsafe { IsIconic(handle) } != 0 || unsafe { IsZoomed(handle) } != 0 {
            unsafe {
                ShowWindow(handle, SW_RESTORE);
            }
        }
    }

    fn get_window_rect(handle: HWND) -> io::Result<RectBounds> {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let ok = unsafe { GetWindowRect(handle, &mut rect) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(RectBounds::from(rect))
    }

    fn monitor_work_area(monitor: isize) -> io::Result<RectBounds> {
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            rcWork: RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            dwFlags: 0,
        };
        let ok = unsafe { GetMonitorInfoW(monitor, &mut info) };
        if ok == 0 {
            let err = unsafe { GetLastError() };
            return Err(io::Error::from_raw_os_error(err as i32));
        }
        Ok(RectBounds::from(info.rcWork))
    }

    impl From<RECT> for RectBounds {
        fn from(value: RECT) -> Self {
            Self {
                left: value.left,
                top: value.top,
                right: value.right,
                bottom: value.bottom,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FocusOutcome;
    use super::RectBounds;
    use super::WindowAttentionOutcome;
    use super::WindowAttentionPolicy;
    use super::WindowOps;
    use codex_core::features::Feature;
    use codex_core::features::Features;

    #[test]
    fn policy_reads_feature_flags() {
        let mut features = Features::default();
        features.enable(Feature::FocusTerminalWindow);
        features.enable(Feature::MoveTerminalWindowToPrimaryMonitor);

        let policy = WindowAttentionPolicy::from_features(&features);
        assert!(policy.focus_terminal_window);
        assert!(policy.move_terminal_window_to_primary_monitor);
        assert!(policy.enabled());
    }

    #[test]
    fn center_rect_preserves_size_when_it_fits() {
        let window = RectBounds {
            left: 0,
            top: 0,
            right: 800,
            bottom: 600,
        };
        let work = RectBounds {
            left: 1920,
            top: 0,
            right: 3840,
            bottom: 1040,
        };

        let centered = super::center_rect_in_work_area(window, work);
        assert_eq!(centered.width(), 800);
        assert_eq!(centered.height(), 600);
        assert_eq!(centered.left, 2480);
        assert_eq!(centered.top, 220);
    }

    #[test]
    fn center_rect_clamps_oversized_windows() {
        let window = RectBounds {
            left: 10,
            top: 10,
            right: 2510,
            bottom: 1510,
        };
        let work = RectBounds {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };

        let centered = super::center_rect_in_work_area(window, work);
        assert_eq!(centered, work);
    }

    struct FakeWindowOps {
        steps: Vec<&'static str>,
        handle: Option<u8>,
        move_result: Result<bool, std::io::ErrorKind>,
        focus_result: Result<FocusOutcome, std::io::ErrorKind>,
    }

    impl Default for FakeWindowOps {
        fn default() -> Self {
            Self {
                steps: Vec::new(),
                handle: None,
                move_result: Ok(false),
                focus_result: Ok(FocusOutcome::default()),
            }
        }
    }

    impl WindowOps for FakeWindowOps {
        type Handle = u8;

        fn resolve_host_window(&mut self) -> Option<Self::Handle> {
            self.steps.push("resolve");
            self.handle
        }

        fn move_to_primary_monitor(&mut self, _handle: Self::Handle) -> std::io::Result<bool> {
            self.steps.push("move");
            self.move_result.map_err(std::io::Error::from)
        }

        fn focus_window(&mut self, _handle: Self::Handle) -> std::io::Result<FocusOutcome> {
            self.steps.push("focus");
            self.focus_result.map_err(std::io::Error::from)
        }
    }

    #[test]
    fn attention_moves_before_focusing() {
        let mut ops = FakeWindowOps {
            handle: Some(7),
            move_result: Ok(true),
            focus_result: Ok(FocusOutcome {
                focused: true,
                flashed: false,
            }),
            ..Default::default()
        };

        let outcome = super::apply_window_attention(
            &mut ops,
            WindowAttentionPolicy {
                focus_terminal_window: true,
                move_terminal_window_to_primary_monitor: true,
            },
        );

        assert_eq!(ops.steps, vec!["resolve", "move", "focus"]);
        assert_eq!(
            outcome,
            WindowAttentionOutcome {
                moved: true,
                focused: true,
                flashed: false,
            }
        );
    }

    #[test]
    fn focus_still_runs_when_move_fails() {
        let mut ops = FakeWindowOps {
            handle: Some(3),
            move_result: Err(std::io::ErrorKind::PermissionDenied),
            focus_result: Ok(FocusOutcome {
                focused: false,
                flashed: true,
            }),
            ..Default::default()
        };

        let outcome = super::apply_window_attention(
            &mut ops,
            WindowAttentionPolicy {
                focus_terminal_window: true,
                move_terminal_window_to_primary_monitor: true,
            },
        );

        assert_eq!(ops.steps, vec!["resolve", "move", "focus"]);
        assert_eq!(
            outcome,
            WindowAttentionOutcome {
                moved: false,
                focused: false,
                flashed: true,
            }
        );
    }

    #[test]
    fn missing_host_window_is_a_noop() {
        let mut ops = FakeWindowOps::default();

        let outcome = super::apply_window_attention(
            &mut ops,
            WindowAttentionPolicy {
                focus_terminal_window: true,
                move_terminal_window_to_primary_monitor: true,
            },
        );

        assert_eq!(ops.steps, vec!["resolve"]);
        assert_eq!(outcome, WindowAttentionOutcome::default());
    }
}
