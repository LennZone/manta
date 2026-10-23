//! macOS side: the OS calls behind [`ViewerOs`] and the Apple Event
//! handlers that receive "open folder" and "reveal" requests.
//!
//! Handlers are installed when `NSApplicationWillFinishLaunchingNotification`
//! fires: after AppKit installed its own handlers (which would forward
//! `odoc` to the app delegate; winit's delegate doesn't implement
//! `application:openURLs:`) and before the event that launched the app is
//! dispatched, so a cold start is caught too. The handlers only queue the
//! request ([`super::push_request`]); the app applies it on the next frame.
//!
//! Measured on macOS 26: `NSWorkspace activateFileViewerSelectingURLs` sends
//! `aevt/odoc` with the selected file URLs to the `NSFileViewer` app
//! (launching it first if needed; the event then arrives right after
//! `willFinishLaunching`, before the first frame). `open <folder>` and
//! `open -a Manta <path>` send `aevt/odoc` too. `misc/mvis` (Finder's
//! "reveal") is handled as well for scripts that send it. LaunchServices
//! refuses `public.folder` for any app but Finder there (paramErr -50 from
//! both NSWorkspace and `LSSetDefaultRoleHandlerForContentType`).

use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::mpsc;
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, NSObject, Sel};
use objc2::{class, define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSString, NSURL};

use super::{OpenRequest, RequestKind, ViewerOs};

// ------------------------------------------------------------------ FFI

type CFStringRef = *const AnyObject;
type CFTypeRef = *const std::ffi::c_void;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFPreferencesAnyApplication: CFStringRef;
    static kCFPreferencesCurrentUser: CFStringRef;
    static kCFPreferencesAnyHost: CFStringRef;
    fn CFPreferencesCopyValue(
        key: CFStringRef,
        app: CFStringRef,
        user: CFStringRef,
        host: CFStringRef,
    ) -> CFTypeRef;
    fn CFPreferencesSetValue(
        key: CFStringRef,
        value: CFTypeRef,
        app: CFStringRef,
        user: CFStringRef,
        host: CFStringRef,
    );
    fn CFPreferencesSynchronize(app: CFStringRef, user: CFStringRef, host: CFStringRef) -> u8;
    fn CFRelease(cf: CFTypeRef);
    fn CFGetTypeID(cf: CFTypeRef) -> usize;
    fn CFStringGetTypeID() -> usize;
}

const K_LS_ROLES_ALL: u32 = 0xFFFF_FFFF;

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn LSCopyDefaultRoleHandlerForContentType(content_type: CFStringRef, role: u32) -> CFStringRef;
    fn LSSetDefaultRoleHandlerForContentType(
        content_type: CFStringRef,
        role: u32,
        handler: CFStringRef,
    ) -> i32;
}

// UTType (macOS 11+), for NSWorkspace's content-type API.
#[link(name = "UniformTypeIdentifiers", kind = "framework")]
unsafe extern "C" {}

const FOLDER_UTI: &str = "public.folder";
const NS_FILE_VIEWER: &str = "NSFileViewer";

fn cf(s: &NSString) -> CFStringRef {
    s as *const NSString as CFStringRef
}

/// Take a +1 CFStringRef (Copy/Create rule) as a Rust string.
unsafe fn take_cf_string(r: CFTypeRef) -> Option<String> {
    if r.is_null() {
        return None;
    }
    let out = unsafe {
        if CFGetTypeID(r) == CFStringGetTypeID() {
            Some((*(r as *const NSString)).to_string())
        } else {
            None
        }
    };
    unsafe { CFRelease(r) };
    out
}

fn ns_string(obj: Option<Retained<NSString>>) -> Option<String> {
    obj.map(|s| s.to_string())
}

// ------------------------------------------------------------------ ViewerOs

pub struct MacOs;

fn workspace() -> Retained<AnyObject> {
    unsafe { msg_send![class!(NSWorkspace), sharedWorkspace] }
}

fn app_url(id: &str) -> Option<Retained<NSURL>> {
    let ws = workspace();
    let id = NSString::from_str(id);
    unsafe { msg_send![&*ws, URLForApplicationWithBundleIdentifier: &*id] }
}

impl ViewerOs for MacOs {
    fn bundle_id(&self) -> Option<String> {
        // Only a real .app counts (`cargo run` has no bundle id anyway).
        self.bundle_path()?;
        let bundle: Retained<AnyObject> = unsafe { msg_send![class!(NSBundle), mainBundle] };
        let id: Option<Retained<NSString>> = unsafe { msg_send![&*bundle, bundleIdentifier] };
        ns_string(id).filter(|s| !s.is_empty())
    }

    fn bundle_path(&self) -> Option<PathBuf> {
        let bundle: Retained<AnyObject> = unsafe { msg_send![class!(NSBundle), mainBundle] };
        let path: Option<Retained<NSString>> = unsafe { msg_send![&*bundle, bundlePath] };
        ns_string(path)
            .map(PathBuf::from)
            .filter(|p| p.extension().is_some_and(|e| e == "app"))
    }

    fn home_dir(&self) -> Option<PathBuf> {
        dirs::home_dir()
    }

    fn file_viewer(&self) -> Option<String> {
        let key = NSString::from_str(NS_FILE_VIEWER);
        unsafe {
            take_cf_string(CFPreferencesCopyValue(
                cf(&key),
                kCFPreferencesAnyApplication,
                kCFPreferencesCurrentUser,
                kCFPreferencesAnyHost,
            ))
        }
    }

    fn set_file_viewer(&self, id: Option<&str>) -> Result<(), String> {
        let key = NSString::from_str(NS_FILE_VIEWER);
        let value = id.map(NSString::from_str);
        let ok = unsafe {
            CFPreferencesSetValue(
                cf(&key),
                value
                    .as_deref()
                    .map_or(std::ptr::null(), |v| cf(v) as CFTypeRef),
                kCFPreferencesAnyApplication,
                kCFPreferencesCurrentUser,
                kCFPreferencesAnyHost,
            );
            CFPreferencesSynchronize(
                kCFPreferencesAnyApplication,
                kCFPreferencesCurrentUser,
                kCFPreferencesAnyHost,
            )
        };
        log::info!(
            "default viewer: NSFileViewer := {id:?} (synchronize ok={})",
            ok != 0
        );
        if ok == 0 {
            return Err("CFPreferencesSynchronize failed".into());
        }
        if self.file_viewer().as_deref() != id {
            return Err(format!(
                "NSFileViewer reads back as {:?}",
                self.file_viewer()
            ));
        }
        Ok(())
    }

    fn folder_handler(&self) -> Option<String> {
        let uti = NSString::from_str(FOLDER_UTI);
        unsafe {
            take_cf_string(
                LSCopyDefaultRoleHandlerForContentType(cf(&uti), K_LS_ROLES_ALL) as CFTypeRef,
            )
        }
    }

    fn set_folder_handler(&self, id: &str) -> Result<(), String> {
        let modern = set_handler_workspace(id);
        let result = match modern {
            Ok(()) => Ok(()),
            Err(e) => {
                log::info!("default viewer: NSWorkspace route failed ({e}); using LaunchServices");
                let uti = NSString::from_str(FOLDER_UTI);
                let h = NSString::from_str(id);
                let status = unsafe {
                    LSSetDefaultRoleHandlerForContentType(cf(&uti), K_LS_ROLES_ALL, cf(&h))
                };
                if status == 0 {
                    Ok(())
                } else {
                    Err(format!(
                        "LSSetDefaultRoleHandlerForContentType: OSStatus {status}"
                    ))
                }
            }
        };
        let now = self.folder_handler();
        log::info!("default viewer: public.folder := {id} ({result:?}); reads back {now:?}");
        result?;
        match now {
            Some(now) if now.eq_ignore_ascii_case(id) => Ok(()),
            other => Err(format!("public.folder handler reads back as {other:?}")),
        }
    }

    fn app_exists(&self, id: &str) -> bool {
        app_url(id).is_some()
    }

    fn app_name(&self, id: &str) -> Option<String> {
        let url = app_url(id)?;
        let path: Option<Retained<NSString>> = unsafe { msg_send![&*url, path] };
        let fm: Retained<AnyObject> = unsafe { msg_send![class!(NSFileManager), defaultManager] };
        let path = path?;
        let name: Option<Retained<NSString>> =
            unsafe { msg_send![&*fm, displayNameAtPath: &*path] };
        ns_string(name).map(|n| n.strip_suffix(".app").map(str::to_string).unwrap_or(n))
    }
}

/// `-[NSWorkspace setDefaultApplicationAtURL:toOpenContentType:completionHandler:]`
/// (macOS 12+). Waits up to 5 s for the completion handler.
fn set_handler_workspace(id: &str) -> Result<(), String> {
    let ws = workspace();
    let sel = sel!(setDefaultApplicationAtURL:toOpenContentType:completionHandler:);
    let responds: bool = unsafe { msg_send![&*ws, respondsToSelector: sel] };
    let Some(ut_class) = AnyClass::get(c"UTType") else {
        return Err("no UTType".into());
    };
    if !responds {
        return Err("macOS < 12".into());
    }
    // Our own copy, not whichever app with this id LaunchServices prefers
    // (an older Manta in /Applications may not claim folders).
    let own = MacOs
        .bundle_id()
        .is_some_and(|me| me.eq_ignore_ascii_case(id));
    let url = if own {
        let bundle: Retained<AnyObject> = unsafe { msg_send![class!(NSBundle), mainBundle] };
        unsafe { msg_send![&*bundle, bundleURL] }
    } else {
        app_url(id)
    };
    let url: Retained<NSURL> = url.ok_or_else(|| format!("{id} is not installed"))?;
    let uti = NSString::from_str(FOLDER_UTI);
    let ty: Option<Retained<AnyObject>> = unsafe { msg_send![ut_class, typeWithIdentifier: &*uti] };
    let ty = ty.ok_or("UTType public.folder")?;
    let (tx, rx) = mpsc::channel::<Option<String>>();
    let block = block2::RcBlock::new(move |err: *mut AnyObject| {
        let msg = NonNull::new(err).map(|e| {
            let d: Option<Retained<NSString>> =
                unsafe { msg_send![e.as_ptr(), localizedDescription] };
            ns_string(d).unwrap_or_else(|| "error".into())
        });
        let _ = tx.send(msg);
    });
    unsafe {
        let _: () = msg_send![
            &*ws,
            setDefaultApplicationAtURL: &*url,
            toOpenContentType: &*ty,
            completionHandler: &*block
        ];
    }
    match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(None) => Ok(()),
        Ok(Some(e)) => Err(e),
        Err(_) => Err("no completion within 5 s".into()),
    }
}

// ------------------------------------------------------------------ Apple Events

const fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

fn fourcc_str(v: u32) -> String {
    v.to_be_bytes()
        .iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '?'
            }
        })
        .collect()
}

const K_CORE_EVENT_CLASS: u32 = fourcc(b"aevt");
const K_AE_OPEN_DOCUMENTS: u32 = fourcc(b"odoc");
const K_AE_MISC_STANDARDS: u32 = fourcc(b"misc");
const K_AE_MAKE_OBJECTS_VISIBLE: u32 = fourcc(b"mvis");
const KEY_DIRECT_OBJECT: u32 = fourcc(b"----");
const TYPE_WILD_CARD: u32 = fourcc(b"****");

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MantaAppleEvents"]
    struct EventTarget;

    impl EventTarget {
        #[unsafe(method(willFinishLaunching:))]
        fn will_finish_launching(&self, _note: &AnyObject) {
            install_handlers(self);
        }

        #[unsafe(method(handleEvent:withReplyEvent:))]
        fn handle_event(&self, event: &AnyObject, _reply: &AnyObject) {
            handle(event);
        }
    }
);

thread_local! {
    static TARGET: std::cell::RefCell<Option<Retained<EventTarget>>> = const { std::cell::RefCell::new(None) };
}

/// Call before `eframe::run_native`: installs the handlers as soon as AppKit
/// is about to finish launching. `MANTA_AE_PROBE=1` also logs every
/// other Apple Event (to find out what an app sends).
pub fn register() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let target: Retained<EventTarget> = unsafe { msg_send![EventTarget::alloc(mtm), init] };
    let center: Retained<AnyObject> =
        unsafe { msg_send![class!(NSNotificationCenter), defaultCenter] };
    let name = NSString::from_str("NSApplicationWillFinishLaunchingNotification");
    unsafe {
        let _: () = msg_send![
            &*center,
            addObserver: &*target,
            selector: sel!(willFinishLaunching:),
            name: &*name,
            object: std::ptr::null::<AnyObject>()
        ];
    }
    TARGET.with(|t| *t.borrow_mut() = Some(target));
}

fn install_handlers(target: &EventTarget) {
    let mgr: Retained<AnyObject> =
        unsafe { msg_send![class!(NSAppleEventManager), sharedAppleEventManager] };
    let sel: Sel = sel!(handleEvent:withReplyEvent:);
    let mut events = vec![
        (K_CORE_EVENT_CLASS, K_AE_OPEN_DOCUMENTS),
        (K_AE_MISC_STANDARDS, K_AE_MAKE_OBJECTS_VISIBLE),
    ];
    if std::env::var_os("MANTA_AE_PROBE").is_some() {
        events.push((TYPE_WILD_CARD, TYPE_WILD_CARD));
    }
    for (class, id) in events {
        unsafe {
            let _: () = msg_send![
                &*mgr,
                setEventHandler: target,
                andSelector: sel,
                forEventClass: class,
                andEventID: id
            ];
        }
    }
    log::info!("default viewer: Apple Event handlers installed (aevt/odoc, misc/mvis)");
}

/// Paths in the event's direct object (a file URL or a list of them).
fn direct_paths(event: &AnyObject) -> Vec<PathBuf> {
    let obj: Option<Retained<AnyObject>> =
        unsafe { msg_send![event, paramDescriptorForKeyword: KEY_DIRECT_OBJECT] };
    let Some(obj) = obj else { return Vec::new() };
    let n: isize = unsafe { msg_send![&*obj, numberOfItems] };
    let items: Vec<Retained<AnyObject>> = if n > 0 {
        (1..=n)
            .filter_map(|i| unsafe { msg_send![&*obj, descriptorAtIndex: i] })
            .collect()
    } else {
        vec![obj]
    };
    items
        .iter()
        .filter_map(|d| {
            let url: Option<Retained<NSURL>> = unsafe { msg_send![&**d, fileURLValue] };
            let path: Option<Retained<NSString>> =
                url.and_then(|u| unsafe { msg_send![&*u, path] });
            ns_string(path).map(PathBuf::from)
        })
        .collect()
}

fn handle(event: &AnyObject) {
    let class: u32 = unsafe { msg_send![event, eventClass] };
    let id: u32 = unsafe { msg_send![event, eventID] };
    let paths = direct_paths(event);
    let kind = match (class, id) {
        (K_CORE_EVENT_CLASS, K_AE_OPEN_DOCUMENTS) => Some(RequestKind::Open),
        (K_AE_MISC_STANDARDS, K_AE_MAKE_OBJECTS_VISIBLE) => Some(RequestKind::Reveal),
        _ => None,
    };
    log::info!(
        "default viewer: Apple Event {}/{} with {} path(s): {:?}",
        fourcc_str(class),
        fourcc_str(id),
        paths.len(),
        paths
    );
    if kind.is_none() || paths.is_empty() {
        let d: Option<Retained<NSString>> = unsafe { msg_send![event, description] };
        let d = ns_string(d).unwrap_or_default();
        log::info!(
            "default viewer: event detail: {}",
            d.chars().take(600).collect::<String>()
        );
    }
    let Some(kind) = kind else { return };
    if paths.is_empty() {
        return;
    }
    super::push_request(OpenRequest { kind, paths });
    activate();
}

/// Bring Manta to the front (another app asked us to show something).
pub fn activate() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    unsafe {
        let _: () = msg_send![&*app, activateIgnoringOtherApps: Bool::YES];
    }
}
