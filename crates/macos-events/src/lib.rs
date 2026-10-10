//! macOS Apple Events: workbooks opened from Finder (context menu, drag onto the Dock
//! icon) and Dock-icon reopen.
//!
//! winit 0.30 does not answer the `odoc` ("open documents") Apple Event, so without help
//! macOS reports *"cannot open files in this format"*: launch-time documents are handed
//! to `NSDocumentController` (which fails for a non-document app and blocks the process
//! in a modal alert), and events for a running instance reach no one. This crate
//!
//! - adds `application:openURLs:`, `application:openFiles:`, `application:openFile:` and
//!   `applicationShouldHandleReopen:` to `NSObject`, so whatever delegate is active
//!   (winit's) answers and the paths land in this crate's queue,
//! - registers `NSAppleEventManager` handlers for `odoc`/`rapp` at install time and
//!   again when AppKit finishes launching,
//!
//! and the UI drains the queue every frame.
//!
//! **This is the repository's only `unsafe` module.** The Apple Event API has no safe
//! binding, so the workspace-level `unsafe_code = "forbid"` is deliberately not applied
//! here (approved exception; see AGENTS.md "Never crash"). Containment rules:
//!
//! - the public API is safe; every `unsafe` block wraps a generated `objc2` call or an
//!   FFI constant,
//! - no `unwrap`/`expect`/`panic` (workspace clippy denies are re-declared in
//!   `Cargo.toml`), failures degrade to "no events",
//! - handler bodies are wrapped in `catch_unwind`, so a bug cannot unwind into
//!   Objective-C and abort the process,
//! - input-derived data (the event payload) is bounded: at most a few thousand paths,
//!   and a path that fails to convert is skipped.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::sync::mpsc::Receiver;

/// An open request that arrived from the operating system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenEvent {
    /// Documents to open: Finder "Open With", double-click, drag onto the Dock icon.
    Files(Vec<String>),
    /// Dock-icon click while running: reopen the last workbook if none is open.
    Reopen,
}

/// Installed Apple Event handlers. Install once, then poll every frame.
pub struct OpenEvents {
    events: Receiver<OpenEvent>,
}

impl OpenEvents {
    /// Registers handlers for `odoc` (open documents) and `rapp` (Dock reopen).
    ///
    /// Call once, on the main thread, before the application runs. The handlers are
    /// installed immediately and re-installed when AppKit finishes launching (see the
    /// module docs for why). Events already in flight are queued and appear in
    /// [`OpenEvents::poll`]. A second call replaces the handlers but keeps the first
    /// queue.
    pub fn install(notify: impl Fn() + Send + 'static) -> OpenEvents {
        #[cfg(target_os = "macos")]
        let events = imp::install(Box::new(notify));
        #[cfg(not(target_os = "macos"))]
        let events = {
            let _ = notify;
            let (_tx, rx) = std::sync::mpsc::channel();
            OpenEvents { events: rx }
        };
        events
    }

    /// Events queued since the previous call, in arrival order.
    pub fn poll(&self) -> Vec<OpenEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = self.events.try_recv() {
            out.push(ev);
        }
        out
    }

    /// Replaces the wake-up callback (e.g. once the egui context exists).
    pub fn set_notify(&self, notify: impl Fn() + Send + 'static) {
        #[cfg(target_os = "macos")]
        imp::set_notify(Box::new(notify));
        #[cfg(not(target_os = "macos"))]
        let _ = notify;
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Mutex;
    use std::sync::OnceLock;
    use std::sync::mpsc::{Sender, channel};

    use block2::RcBlock;
    use core::ffi::c_char;
    use core::ptr::NonNull;
    use objc2::MainThreadMarker;
    use objc2::MainThreadOnly;
    use objc2::define_class;
    use objc2::ffi::class_addMethod;
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool, NSObject, Sel};
    use objc2::sel;
    use objc2_core_services::{kAEOpenDocuments, kAEReopenApplication, kCoreEventClass, keyDirectObject};
    use objc2_foundation::{
        NSAppleEventDescriptor, NSAppleEventManager, NSArray, NSNotification, NSNotificationCenter, NSObjectProtocol, NSString, NSURL,
    };

    use super::{OpenEvent, OpenEvents};

    /// Upper bound on paths parsed from one event (input is hostile; a real
    /// launch passes a handful).
    const MAX_FILES: isize = 4096;

    static QUEUE: OnceLock<Mutex<Sender<OpenEvent>>> = OnceLock::new();
    static NOTIFY: OnceLock<Mutex<Box<dyn Fn() + Send>>> = OnceLock::new();

    pub fn install(notify: Box<dyn Fn() + Send>) -> OpenEvents {
        let (tx, rx) = channel();
        let _ = QUEUE.set(Mutex::new(tx));
        let _ = NOTIFY.set(Mutex::new(notify));

        // Apple Events are dispatched on the main thread; without one (never in the
        // app, which installs on the main thread) degrade to a queue nobody reads.
        let Some(mtm) = MainThreadMarker::new() else { return OpenEvents { events: rx } };

        // SAFETY: main thread, verified above; see `register_handlers`.
        unsafe { register_handlers(mtm) };
        // SAFETY: adds process-local delegate catches; see `install_delegate_catches`.
        unsafe { install_delegate_catches() };

        // AppKit may install its own default handlers for the same events when it
        // finishes launching, replacing the ones registered here; re-register when the
        // launch finishes. The notification fires at the end of `finishLaunching`.
        //
        // SAFETY: sharedApplication on the main thread; the msg_sends match the
        // NSApplication/NSNotificationCenter signatures. With `queue: nil` the block
        // runs synchronously on the thread that posts the notification — the main
        // thread, which is what `register_handlers` requires.
        unsafe {
            let app: Option<Retained<AnyObject>> = msg_send![objc2::class!(NSApplication), sharedApplication];
            let running = match &app {
                Some(app) => {
                    let r: bool = msg_send![app, isRunning];
                    r
                }
                None => false,
            };
            if !running {
                let block = RcBlock::new(move |_note: NonNull<NSNotification>| {
                    if let Some(mtm) = MainThreadMarker::new() {
                        // SAFETY: the notification is posted on the main thread; see
                        // `register_handlers`. (Already inside an unsafe block.)
                        register_handlers(mtm);
                    }
                });
                let center = NSNotificationCenter::defaultCenter();
                let name = NSString::from_str("NSApplicationDidFinishLaunchingNotification");
                let observer = center.addObserverForName_object_queue_usingBlock(Some(&name), None, None, &block);
                // The observer must live for the rest of the process; nothing removes it.
                std::mem::forget(observer);
            }
        }

        OpenEvents { events: rx }
    }

    /// Installs a fresh `Handler` for `odoc` and `rapp`. A later call replaces the
    /// previous registration (AppKit's defaults included) but events already queued
    /// are kept.
    ///
    /// SAFETY: main thread only (`mtm` proves it); `handler` is leaked, so the
    /// registration target outlives every event; handler bodies never unwind
    /// (`catch_unwind`); the selectors are the ones declared on `Handler`.
    unsafe fn register_handlers(mtm: MainThreadMarker) {
        let handler: Retained<Handler> = msg_send![Handler::alloc(mtm), init];
        let manager = NSAppleEventManager::sharedAppleEventManager();
        let _: () = msg_send![&manager,
            setEventHandler: &*handler,
            andSelector: sel!(openDocuments:),
            forEventClass: kCoreEventClass,
            andEventID: kAEOpenDocuments,
        ];
        let _: () = msg_send![&manager,
            setEventHandler: &*handler,
            andSelector: sel!(reopenApplication:),
            forEventClass: kCoreEventClass,
            andEventID: kAEReopenApplication,
        ];
        // `NSAppleEventManager` may or may not retain its target; this guarantees the
        // handler stays alive either way. One small object per registration.
        std::mem::forget(handler);
    }

    pub fn set_notify(notify: Box<dyn Fn() + Send>) {
        if let Some(slot) = NOTIFY.get() {
            *slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = notify;
        }
    }

    fn dispatch(event: OpenEvent) {
        if let Some(q) = QUEUE.get() {
            let tx = q.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = tx.send(event);
        }
        if let Some(n) = NOTIFY.get() {
            (n.lock().unwrap_or_else(std::sync::PoisonError::into_inner))();
        }
    }

    /// Extracts the file paths from an `odoc` event: the direct object is a list of
    /// file descriptors (or a single one). Paths that fail to convert are skipped.
    fn files_of(event: Option<&NSAppleEventDescriptor>) -> Vec<String> {
        let Some(event) = event else { return Vec::new() };
        let Some(direct) = event.paramDescriptorForKeyword(keyDirectObject) else {
            return Vec::new();
        };
        let count = direct.numberOfItems().min(MAX_FILES);
        if count <= 0 {
            // Not a list: the direct object is one descriptor.
            return path_of(&direct).into_iter().collect();
        }
        (1..=count).filter_map(|i| direct.descriptorAtIndex(i).and_then(|d| path_of(&d))).collect()
    }

    /// Paths from an `NSArray<NSString>` (the `application:openFiles:` delegate
    /// callback). Input is hostile: nil array, non-strings and huge arrays are
    /// tolerated.
    ///
    /// SAFETY: `files` must be null or a valid `NSArray<NSString>`.
    unsafe fn paths_of_nsarray(files: *mut NSArray<NSString>) -> Vec<String> {
        if files.is_null() {
            return Vec::new();
        }
        // SAFETY: null-checked above; the reference lives for the call.
        let files: &NSArray<NSString> = unsafe { &*files };
        let count = files.count().min(MAX_FILES as usize);
        (0..count).map(|i| files.objectAtIndex(i).to_string()).collect()
    }

    /// Path of a single `NSString` (the `application:openFile:` callback).
    ///
    /// SAFETY: `file` must be null or a valid `NSString`.
    unsafe fn path_of_nsstring(file: *mut NSString) -> Vec<String> {
        if file.is_null() {
            return Vec::new();
        }
        // SAFETY: null-checked above; the reference lives for the call.
        vec![unsafe { &*file }.to_string()]
    }

    /// Paths from an `NSArray<NSURL>` (the `application:openURLs:` callback).
    ///
    /// SAFETY: `urls` must be null or a valid `NSArray<NSURL>`.
    unsafe fn paths_of_nsurls(urls: *mut NSArray<NSURL>) -> Vec<String> {
        if urls.is_null() {
            return Vec::new();
        }
        // SAFETY: null-checked above; the reference lives for the call.
        let urls: &NSArray<NSURL> = unsafe { &*urls };
        let count = urls.count().min(MAX_FILES as usize);
        (0..count).filter_map(|i| urls.objectAtIndex(i).path().map(|p| p.to_string())).collect()
    }

    // --- Delegate catches -------------------------------------------------------------
    //
    // On modern macOS the launch-time documents are NOT dispatched through
    // `NSAppleEventManager` at all: LaunchServices hands them to AppKit, which checks
    // whether the application delegate answers `application:openFiles:` (and the URL /
    // singular variants) and otherwise falls back to `NSDocumentController` — which
    // fails for a non-document app and blocks the whole process in a modal error
    // alert. winit owns the delegate, so instead of wrapping it, the methods are added
    // to `NSObject` itself (process-local): every delegate then answers, including
    // winit's, and the IMPs route the paths into this crate's queue and ignore `self`.

    /// `-(void)application:(NSApplication *)sender openFiles:(NSArray<NSString *> *)files`
    unsafe extern "C-unwind" fn imp_open_files(_this: *mut AnyObject, _cmd: Sel, _sender: *mut AnyObject, files: *mut NSArray<NSString>) {
        // Never unwind into Objective-C.
        let _ = catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: the runtime hands us the array from the callback.
            let paths = unsafe { paths_of_nsarray(files) };
            dispatch(OpenEvent::Files(paths));
        }));
    }

    /// `-(void)application:(NSApplication *)sender openFile:(NSString *)file`
    unsafe extern "C-unwind" fn imp_open_file(_this: *mut AnyObject, _cmd: Sel, _sender: *mut AnyObject, file: *mut NSString) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: the runtime hands us the string from the callback.
            let paths = unsafe { path_of_nsstring(file) };
            dispatch(OpenEvent::Files(paths));
        }));
    }

    /// `-(void)application:(NSApplication *)sender openURLs:(NSArray<NSURL *> *)urls`
    unsafe extern "C-unwind" fn imp_open_urls(_this: *mut AnyObject, _cmd: Sel, _sender: *mut AnyObject, urls: *mut NSArray<NSURL>) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: the runtime hands us the array from the callback.
            let paths = unsafe { paths_of_nsurls(urls) };
            dispatch(OpenEvent::Files(paths));
        }));
    }

    /// `-(BOOL)applicationShouldHandleReopen:(NSApplication *)sender
    ///     hasVisibleWindows:(BOOL)flag`
    unsafe extern "C-unwind" fn imp_reopen(_this: *mut AnyObject, _cmd: Sel, _sender: *mut AnyObject, _flag: Bool) -> Bool {
        let _ = catch_unwind(AssertUnwindSafe(|| dispatch(OpenEvent::Reopen)));
        // Let AppKit do its normal activation/reopen bookkeeping as well.
        Bool::from(true)
    }

    /// Adds the delegate-catch methods to `NSObject` (idempotent: `class_addMethod`
    /// keeps the existing method if one is already present).
    ///
    /// SAFETY: `NSObject` is a valid class; the IMPs match the declared signatures;
    /// the type encodings are the standard ones for the methods above; the `transmute`
    /// calls only change the fn-pointer type (all fn pointers are `unsafe
    /// extern "C-unwind"` and pointer-sized), never the code.
    unsafe fn install_delegate_catches() {
        // SAFETY: `NSObject` is a valid, live class.
        let cls: *mut AnyClass = objc2::class!(NSObject) as *const AnyClass as *mut AnyClass;
        // `(void)(id self, SEL cmd, id sender, id files)`
        let void_two_args = c"v@:@@@".as_ptr() as *const c_char;
        // `(BOOL)(id self, SEL cmd, id sender, BOOL flag)`
        let bool_two_args = c"c@:@@c".as_ptr() as *const c_char;
        // Coerce each fn item to its fn pointer first (fn items are zero-sized types).
        let imp_files: objc2::runtime::Imp = unsafe {
            core::mem::transmute(imp_open_files as unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut NSArray<NSString>))
        };
        let imp_file: objc2::runtime::Imp =
            unsafe { core::mem::transmute(imp_open_file as unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut NSString)) };
        let imp_urls: objc2::runtime::Imp =
            unsafe { core::mem::transmute(imp_open_urls as unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut NSArray<NSURL>)) };
        let imp_re: objc2::runtime::Imp =
            unsafe { core::mem::transmute(imp_reopen as unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, Bool) -> Bool) };
        unsafe {
            class_addMethod(cls, sel!(application:openFiles:), imp_files, void_two_args);
            class_addMethod(cls, sel!(application:openFile:), imp_file, void_two_args);
            class_addMethod(cls, sel!(application:openURLs:), imp_urls, void_two_args);
            class_addMethod(cls, sel!(applicationShouldHandleReopen:hasVisibleWindows:), imp_re, bool_two_args);
        }
    }
    /// `fileURLValue` coerces `furl` and `alis` descriptors to a URL (Foundation does
    /// the coercion; see NSAppleEventDescriptor.h). Cyrillic and other non-ASCII paths
    /// survive: NSString converts to UTF-8.
    fn path_of(desc: &NSAppleEventDescriptor) -> Option<String> {
        let url = desc.fileURLValue()?;
        let path = url.path()?;
        Some(path.to_string())
    }

    define_class!(
        // SAFETY: the superclass NSObject has no subclassing requirements; the class has
        // no instance state and no `Drop` (the queue lives in process globals).
        #[unsafe(super(NSObject))]
        #[thread_kind = objc2::MainThreadOnly]
        struct Handler;

        unsafe impl NSObjectProtocol for Handler {}

        impl Handler {
            /// kAEOpenDocuments: files handed over by Finder / Launch Services.
            #[unsafe(method(openDocuments:))]
            unsafe fn open_documents(&self, event: Option<&NSAppleEventDescriptor>) {
                // Never unwind into Objective-C.
                let _ = catch_unwind(AssertUnwindSafe(|| dispatch(OpenEvent::Files(files_of(event)))));
            }

            /// kAEReopenApplication: the Dock icon was clicked while running.
            #[unsafe(method(reopenApplication:))]
            unsafe fn reopen_application(&self, _event: Option<&NSAppleEventDescriptor>) {
                let _ = catch_unwind(AssertUnwindSafe(|| dispatch(OpenEvent::Reopen)));
            }
        }
    );
}
