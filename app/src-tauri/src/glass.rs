use tauri::AppHandle;

#[cfg(target_os = "macos")]
mod native {
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    };

    use objc2::{
        MainThreadMarker, msg_send,
        rc::Retained,
        runtime::{AnyObject, Sel},
        sel,
    };
    use objc2_app_kit::{
        NSAppKitVersionNumber, NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
        NSAutoresizingMaskOptions, NSGlassEffectView, NSGlassEffectViewStyle, NSView, NSVisualEffectBlendingMode,
        NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow, NSWindowOrderingMode,
    };
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use tauri::{AppHandle, Manager};

    use crate::{index::lock, widget::LABEL};

    const MATERIAL: NSVisualEffectMaterial = NSVisualEffectMaterial::Popover;
    const GLASS_STYLE: NSGlassEffectViewStyle = NSGlassEffectViewStyle::Clear;
    const LIQUID_GLASS_APPKIT: f64 = 2685.0;

    enum Glass {
        Liquid(Retained<NSGlassEffectView>),
        Material(Retained<NSVisualEffectView>),
    }
    unsafe impl Send for Glass {}

    impl Glass {
        fn view(&self) -> &NSView {
            match self {
                Glass::Liquid(view) => view,
                Glass::Material(view) => view,
            }
        }

        fn round(&self, radius: f64) {
            match self {
                Glass::Liquid(view) => view.setCornerRadius(radius),
                Glass::Material(view) => round_material(view, radius),
            }
        }
    }

    static GLASS: Mutex<Option<Glass>> = Mutex::new(None);
    static MADE: AtomicBool = AtomicBool::new(false);

    pub fn available() -> bool {
        MADE.load(Ordering::SeqCst)
    }

    fn on_main(app: &AppHandle, then: impl FnOnce(&NSWindow) + Send + 'static) {
        let Some(window) = app.get_webview_window(LABEL) else {
            return;
        };
        let on_window = window.clone();
        let _ = window.run_on_main_thread(move || {
            let Ok(pointer) = on_window.ns_window() else { return };
            then(unsafe { &*pointer.cast::<NSWindow>() });
        });
    }

    pub fn install(app: &AppHandle) {
        if std::env::var_os("MESA_NO_GLASS").is_some_and(|value| !value.is_empty()) {
            return;
        }
        on_main(app, |window| {
            let Some(mtm) = MainThreadMarker::new() else { return };
            let Some(content) = window.contentView() else { return };
            let liquid = unsafe { NSAppKitVersionNumber } >= LIQUID_GLASS_APPKIT;
            let glass = if liquid {
                let view = NSGlassEffectView::new(mtm);
                view.setStyle(GLASS_STYLE);
                Glass::Liquid(view)
            } else {
                let view = NSVisualEffectView::new(mtm);
                view.setMaterial(MATERIAL);
                view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
                view.setState(NSVisualEffectState::Active);
                Glass::Material(view)
            };
            let view = glass.view();
            view.setAutoresizingMask(NSAutoresizingMaskOptions::empty());
            view.setHidden(true);
            content.addSubview_positioned_relativeTo(view, NSWindowOrderingMode::Below, None);
            *lock(&GLASS) = Some(glass);
            MADE.store(true, Ordering::SeqCst);
        });
    }

    fn round_material(view: &NSVisualEffectView, radius: f64) {
        let own: Sel = sel!(setCornerRadius:);
        unsafe {
            let responds: bool = msg_send![view, respondsToSelector: own];
            if responds {
                let () = msg_send![view, setCornerRadius: radius];
            } else {
                view.setWantsLayer(true);
                let layer: Option<Retained<AnyObject>> = msg_send![view, layer];
                if let Some(layer) = layer {
                    let () = msg_send![&*layer, setCornerRadius: radius];
                    let () = msg_send![&*layer, setMasksToBounds: true];
                }
            }
        }
    }

    pub fn set_box(app: &AppHandle, rect: [f64; 4], radius: f64) {
        on_main(app, move |window| {
            let guard = lock(&GLASS);
            let Some(glass) = guard.as_ref() else { return };
            let Some(content) = window.contentView() else { return };
            let height = content.bounds().size.height;
            let [left, top, right, bottom] = rect;
            let frame = CGRect::new(
                CGPoint::new(left, height - bottom),
                CGSize::new(right - left, bottom - top),
            );
            glass.view().setFrame(frame);
            glass.round(radius);
            glass.view().setHidden(false);
        });
    }

    pub fn hide(app: &AppHandle) {
        on_main(app, |_| {
            if let Some(glass) = lock(&GLASS).as_ref() {
                glass.view().setHidden(true);
            }
        });
    }

    pub fn set_dark(app: &AppHandle, dark: bool) {
        on_main(app, move |window| {
            let name = unsafe {
                if dark {
                    NSAppearanceNameDarkAqua
                } else {
                    NSAppearanceNameAqua
                }
            };
            let appearance = NSAppearance::appearanceNamed(name);
            window.setAppearance(appearance.as_deref());
        });
    }
}

pub fn available() -> bool {
    #[cfg(target_os = "macos")]
    return native::available();
    #[cfg(not(target_os = "macos"))]
    false
}

pub fn install(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    native::install(app);
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

pub fn set_box(app: &AppHandle, rect: [f64; 4], radius: f64) {
    #[cfg(target_os = "macos")]
    native::set_box(app, rect, radius);
    #[cfg(not(target_os = "macos"))]
    let _ = (app, rect, radius);
}

pub fn hide(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    native::hide(app);
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

pub fn set_dark(app: &AppHandle, dark: bool) {
    #[cfg(target_os = "macos")]
    native::set_dark(app, dark);
    #[cfg(not(target_os = "macos"))]
    let _ = (app, dark);
}
