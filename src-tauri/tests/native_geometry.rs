#[cfg(not(target_os = "macos"))]
fn main() {
    println!("Native geometry test requires macOS; skipped.");
}

#[cfg(target_os = "macos")]
fn main() {
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSScreen, NSWindow,
        NSWindowStyleMask,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    if std::env::var_os("MARGIN_TEST_NATIVE_GEOMETRY").is_none() {
        println!("Set MARGIN_TEST_NATIVE_GEOMETRY=1 to test hidden AppKit windows; skipped.");
        return;
    }
    let mtm = MainThreadMarker::new().expect("AppKit must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    let screens = NSScreen::screens(mtm);
    assert!(!screens.is_empty(), "A desktop session is required");
    let initial = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(420.0, 660.0));
    // This process owns one hidden window; AppKit must not release it again on close.
    let window = unsafe {
        let window = NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            initial,
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        );
        window.setReleasedWhenClosed(false);
        window
    };
    for (index, screen) in screens.iter().enumerate() {
        let work = screen.visibleFrame();
        let size = NSSize::new(work.size.width.min(420.0), work.size.height.min(660.0));
        let origin = NSPoint::new(
            work.origin.x + (work.size.width - size.width) / 2.0,
            work.origin.y + (work.size.height - size.height) / 2.0,
        );
        let target = NSRect::new(origin, size);
        window.setFrame_display(window.frameRectForContentRect(target), false);
        let scale = window.backingScaleFactor();
        assert_eq!(scale, screen.backingScaleFactor());
        if scale > 1.0 {
            // The old path sized for a wrongly selected 1x monitor, then divided by this screen's scale.
            window.setContentSize(NSSize::new(size.width / scale, size.height / scale));
            let undersized = window.contentRectForFrameRect(window.frame());
            assert_eq!(undersized.size.width, size.width / scale);
            assert_eq!(undersized.size.height, size.height / scale);
            println!(
                "Screen {index}: reproduced {}x{} points at {scale}x",
                undersized.size.width, undersized.size.height
            );
        }
        window.setFrame_display(window.frameRectForContentRect(target), false);
        let actual = window.contentRectForFrameRect(window.frame());
        assert_eq!(actual, target);
        assert_eq!(window.backingScaleFactor(), screen.backingScaleFactor());
        assert!(!window.isVisible());
        println!(
            "Screen {index}: verified {}x{} points at {scale}x",
            actual.size.width, actual.size.height
        );
    }
    window.close();
}
