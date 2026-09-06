//! `--screenshot` (PM-100): the app captures its own window and exits, for
//! scripted/manual visual verification without external window-hunting.
//!
//! External automation (`GetWindowRect` + `PrintWindow` from PowerShell, say)
//! has to find the right window first — by title or by enumerating handles —
//! and a naive fallback that gets that wrong can end up capturing an
//! unrelated window or the whole screen. A self-probe never has that failure
//! mode: it already has the exact `Window` it just created.
//!
//! Still implemented via Win32 `PrintWindow` rather than gpui's own frame
//! buffer — gpui's `capture_screenshot` exists only on its headless/visual-test
//! harness (`HeadlessAppContext`/`VisualTestContext`), not on a normally
//! windowed `App`, and standing up that harness for a production binary is a
//! bigger undertaking than this debug affordance calls for. `raw-window-handle`
//! gets us the native `HWND` straight from gpui's `Window`, so nothing here
//! hunts for it externally.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{App, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Wait for the window to settle, capture it to `out`, then quit. Call once,
/// right after the window is created.
pub fn schedule_screenshot(window: &mut Window, cx: &mut App, out: PathBuf) {
    let hwnd = match window_hwnd(window) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("--screenshot: {e}");
            cx.quit();
            return;
        }
    };

    cx.spawn(async move |cx| {
        // Layout/ticket-store loading can take a frame or two; a flat delay
        // is simpler than chaining `on_next_frame` calls and plenty for a
        // debug tool.
        cx.background_executor().timer(Duration::from_millis(500)).await;

        match capture_window(hwnd, &out) {
            Ok(()) => eprintln!("pm: screenshot saved to {}", out.display()),
            Err(e) => eprintln!("--screenshot: {e}"),
        }

        let _ = cx.update(|cx| cx.quit());
    })
    .detach();
}

/// The native window handle as a plain integer — `Send`-safe, unlike
/// `raw_window_handle::WindowHandle`'s borrow of `Window`, so it can cross
/// into the spawned task above.
fn window_hwnd(window: &Window) -> anyhow::Result<isize> {
    // `HandleError` doesn't implement `std::error::Error`, so `?` can't convert it.
    let handle = HasWindowHandle::window_handle(window).map_err(|e| anyhow::anyhow!("{e}"))?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Ok(h.hwnd.get()),
        _ => anyhow::bail!("only implemented for Win32 windows (PM-100)"),
    }
}

#[cfg(windows)]
fn capture_window(hwnd_raw: isize, out: &Path) -> anyhow::Result<()> {
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
        ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows_sys::Win32::Storage::Xps::PrintWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;

    // Not exported as a named constant by windows-sys 0.59; this is
    // PW_RENDERFULLCONTENT, needed so the capture includes gpui's own
    // (non-GDI, GPU-composited) content instead of coming back blank.
    const PW_RENDERFULLCONTENT: u32 = 2;

    // SAFETY: every handle created here (`screen_dc`, `mem_dc`, `bitmap`) is
    // released/deleted before returning on every path, including errors —
    // each fallible step below bails out through the cleanup at the bottom
    // rather than an early `?` that would leak a GDI handle.
    unsafe {
        let hwnd = hwnd_raw as HWND;
        let mut rect: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut rect) == 0 {
            anyhow::bail!("GetWindowRect failed");
        }
        let width = (rect.right - rect.left).max(1);
        let height = (rect.bottom - rect.top).max(1);

        let screen_dc = GetDC(std::ptr::null_mut());
        if screen_dc.is_null() {
            anyhow::bail!("GetDC failed");
        }
        let mem_dc = CreateCompatibleDC(screen_dc);
        let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
        let prev = SelectObject(mem_dc, bitmap);

        let printed = PrintWindow(hwnd, mem_dc, PW_RENDERFULLCONTENT);

        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Negative height requests a top-down DIB, so rows come out in
            // normal (not upside-down) image order.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };

        let mut buf = vec![0u8; (width * height * 4) as usize];
        let copied = GetDIBits(
            screen_dc,
            bitmap,
            0,
            height as u32,
            buf.as_mut_ptr() as *mut core::ffi::c_void,
            &mut bmi,
            DIB_RGB_COLORS,
        );

        SelectObject(mem_dc, prev);
        DeleteObject(bitmap);
        DeleteDC(mem_dc);
        ReleaseDC(std::ptr::null_mut(), screen_dc);

        if printed == 0 {
            anyhow::bail!("PrintWindow failed");
        }
        if copied == 0 {
            anyhow::bail!("GetDIBits failed");
        }

        // GDI gives BGRA; the `image` crate's RgbaImage wants RGBA.
        for px in buf.chunks_exact_mut(4) {
            px.swap(0, 2);
        }

        let img = image::RgbaImage::from_raw(width as u32, height as u32, buf)
            .ok_or_else(|| anyhow::anyhow!("buffer size didn't match the captured image"))?;
        img.save(out)?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn capture_window(_hwnd: isize, _out: &Path) -> anyhow::Result<()> {
    anyhow::bail!("--screenshot is only implemented for Windows for now (PM-100)")
}
