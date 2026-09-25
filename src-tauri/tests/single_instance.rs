//! Launching ComfyVault while it is already open brings the open window to
//! the front and starts nothing else.
//!
//! This drives the real built program on a real Windows desktop, so it runs
//! only when asked, with the program named in `COMFYVAULT_EXE`:
//!
//! ```text
//! COMFYVAULT_EXE='C:\scratch\comfyvault.exe' WSLENV=COMFYVAULT_EXE/p \
//!   ./single-instance-tests.exe --ignored --nocapture
//! ```
//!
//! The program opens the vault named in this user's settings, as it does for
//! a person. The test only minimizes its window, launches it a second time,
//! and closes it.

#![cfg(windows)]

use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, EnumWindows, GetForegroundWindow,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, PeekMessageW,
    PostMessageW, SetForegroundWindow, ShowWindow, MSG, PM_REMOVE, SW_MINIMIZE, WM_CLOSE, WS_POPUP,
    WS_VISIBLE,
};

/// A small window of this test's own, in front, standing in for the shell a
/// person launches programs from: Explorer, the Start menu or the taskbar.
/// Windows lets only a launch from the program in front bring a window
/// forward, so a launch from this test in the background would prove nothing.
///
/// It has no system menu. When Windows refuses a plain request for the
/// foreground, the program taps Alt, and a
/// window with a system menu would take that as "open the menu" and stop.
struct Shell(HWND);

impl Shell {
    fn in_front() -> Shell {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let title: Vec<u16> = "launcher\0".encode_utf16().collect();
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_POPUP | WS_VISIBLE,
                0,
                0,
                200,
                100,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null(), "could not make the launcher window");
        // Joining the input of the program in front lets this window take
        // its place, the way a click would.
        unsafe {
            let front = GetForegroundWindow();
            let theirs = GetWindowThreadProcessId(front, std::ptr::null_mut());
            let mine = GetCurrentThreadId();
            AttachThreadInput(mine, theirs, 1);
            SetForegroundWindow(hwnd);
            AttachThreadInput(mine, theirs, 0);
        }
        let shell = Shell(hwnd);
        wait_for("the launcher to come to the front", 10, || {
            pump();
            (unsafe { GetForegroundWindow() } == hwnd).then_some(())
        });
        shell
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.0) };
    }
}

fn pump() {
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
        unsafe { DispatchMessageW(&msg) };
    }
}

/// The program under test, closed when the test ends, pass or fail.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(w) = main_window(self.0.id()) {
            unsafe { PostMessageW(w, WM_CLOSE, 0, 0) };
        }
        let until = Instant::now() + Duration::from_secs(10);
        while Instant::now() < until {
            if let Ok(Some(_)) = self.0.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        // Only the copy this test started.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn program() -> PathBuf {
    std::env::var_os("COMFYVAULT_EXE")
        .map(PathBuf::from)
        .expect("set COMFYVAULT_EXE to the built comfyvault.exe")
}

/// How many copies of ComfyVault are running, under any file name the
/// program was given here.
fn copies_running() -> usize {
    let name = program().file_name().unwrap().to_string_lossy().to_string();
    let out = Command::new("tasklist.exe")
        .args(["/FI", &format!("IMAGENAME eq {name}"), "/FO", "CSV", "/NH"])
        .output()
        .expect("run tasklist");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.to_lowercase().starts_with(&format!("\"{}\"", name.to_lowercase())))
        .count()
}

/// The visible window titled "ComfyVault" that this process owns.
fn main_window(pid: u32) -> Option<HWND> {
    struct Search {
        pid: u32,
        found: Vec<HWND>,
    }
    unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> i32 {
        let s = &mut *(data as *mut Search);
        let mut owner = 0;
        GetWindowThreadProcessId(hwnd, &mut owner);
        if owner == s.pid && IsWindowVisible(hwnd) != 0 {
            let mut buf = [0u16; 64];
            let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            let title = std::ffi::OsString::from_wide(&buf[..n.max(0) as usize]);
            if title == "ComfyVault" {
                s.found.push(hwnd);
            }
        }
        1
    }
    let mut s = Search { pid, found: Vec::new() };
    unsafe { EnumWindows(Some(each), &mut s as *mut Search as LPARAM) };
    assert!(s.found.len() <= 1, "one copy opened {} windows", s.found.len());
    s.found.pop()
}

fn wait_for<T>(what: &str, secs: u64, mut f: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < until, "gave up after {secs}s waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Starts the program, waits for its window, and, when `settle` is set,
/// waits the few seconds a first start takes to finish.
fn start_first(settle: bool) -> (Running, HWND) {
    assert_eq!(
        copies_running(),
        0,
        "ComfyVault is already running. Close it first: a second launch would \
         hand over to that copy, and this test would be judging it instead"
    );
    let first = Running(Command::new(program()).spawn().expect("start ComfyVault"));
    let window = wait_for("the first window", 60, || main_window(first.0.id()));
    if settle {
        std::thread::sleep(Duration::from_secs(4));
    }
    (first, window)
}

/// Launches the program again from a launcher in front, and checks that the
/// second copy handed over and left, and that the open window came forward.
fn launch_again_and_check(mut first: Running, window: HWND) {
    let pid = first.0.id();
    let shell = Shell::in_front();
    let mut second = Running(Command::new(program()).spawn().expect("launch it again"));
    let status = wait_for("the second launch to exit", 20, || {
        pump();
        second.0.try_wait().unwrap()
    });
    assert!(status.success(), "the second launch exited with {status}");
    assert!(first.0.try_wait().unwrap().is_none(), "the open copy closed");
    assert_eq!(copies_running(), 1, "a second copy is still running");

    let until = Instant::now() + Duration::from_secs(10);
    loop {
        pump();
        let front = unsafe { GetForegroundWindow() };
        let minimized = unsafe { IsIconic(window) } != 0;
        if !minimized && front == window {
            break;
        }
        if Instant::now() > until {
            let mut owner = 0;
            unsafe { GetWindowThreadProcessId(front, &mut owner) };
            panic!(
                "the open window did not come to the front: minimized {minimized}, \
                 the front window belongs to process {owner}, ComfyVault is {pid}"
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(main_window(pid), Some(window), "the open copy made another window");
    drop(shell);
}

#[test]
#[ignore = "drives the real built program on a desktop"]
fn launching_it_again_brings_the_minimized_window_back_and_starts_nothing() {
    let (first, window) = start_first(true);
    // Out of the way, the way a person leaves it before launching it again.
    unsafe { ShowWindow(window, SW_MINIMIZE) };
    wait_for("the window to minimize", 10, || (unsafe { IsIconic(window) } != 0).then_some(()));
    launch_again_and_check(first, window);
}

#[test]
#[ignore = "drives the real built program on a desktop"]
fn two_quick_launches_leave_one_window_in_front() {
    // The second launch lands while the first copy is still starting, before
    // its window can be shown. The hand-over must wait for it, not be lost.
    let (first, window) = start_first(false);
    launch_again_and_check(first, window);
}
