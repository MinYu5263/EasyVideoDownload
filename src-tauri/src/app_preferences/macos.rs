use objc2::{
    class,
    encode::Encode,
    ffi, msg_send,
    runtime::{AnyObject, Imp, Sel},
    sel,
};
use std::{ffi::CString, sync::OnceLock};

static APPLICATION: OnceLock<tauri::AppHandle> = OnceLock::new();

// Cocoa's Quit menu, Cmd+Q and Dock Quit bypass Tauri's ExitRequested event.
// Cancel Cocoa termination and let the ordinary Rust confirmation/drain path
// call AppHandle::exit(0), which ends the runtime without calling terminate:.
unsafe extern "C-unwind" fn should_terminate(
    _delegate: &AnyObject,
    _selector: Sel,
    _application: *mut AnyObject,
) -> usize {
    let result = std::panic::catch_unwind(|| {
        if let Some(app) = APPLICATION.get() {
            let handle = app.clone();
            if let Err(error) = app.run_on_main_thread(move || super::request_exit(&handle)) {
                super::report_error(app, "exitFailed", error);
            }
        }
    });
    if result.is_err() {
        log::error!("nativeQuitFailed: could not handle the Cocoa quit request");
    }
    // Always cancel, including repeated Quit requests while tasks are draining.
    // Returning TerminateNow during that interval would cut the drain short.
    0
}

pub(super) fn install(app: &tauri::AppHandle) -> Result<(), String> {
    // Tauri runs setup on the main thread after its Cocoa delegate is installed.
    if objc2::MainThreadMarker::new().is_none() {
        return Err("The native quit handler must be installed on the main thread".into());
    }
    let selector = sel!(applicationShouldTerminate:);
    let types = CString::new(format!("{}@:@", usize::ENCODING)).map_err(|e| e.to_string())?;
    unsafe {
        let application: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let delegate: *mut AnyObject = msg_send![application, delegate];
        if delegate.is_null() {
            return Err("The Cocoa application delegate is unavailable".into());
        }
        let class = ffi::object_getClass(delegate).cast_mut();
        if !ffi::class_getInstanceMethod(class, selector).is_null() {
            return Err("The Cocoa delegate already has a quit handler".into());
        }
        APPLICATION
            .set(app.clone())
            .map_err(|_| "The native quit handler is already installed")?;
        // The signature is NSUInteger (object, selector, object). objc2 provides
        // the platform's NSUInteger encoding; only the erased IMP needs a cast.
        let implementation: Imp = std::mem::transmute(
            should_terminate
                as unsafe extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject) -> usize,
        );
        if !ffi::class_addMethod(class, selector, implementation, types.as_ptr()).as_bool() {
            return Err("Could not install the Cocoa quit handler".into());
        }
    }
    Ok(())
}
