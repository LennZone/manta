//! AppKit side of [`super`]: `NSDraggingSession` + `NSDraggingSource`, file
//! URLs for local items, `NSFilePromiseProvider` for remote ones.
//!
//! Everything here runs on the main thread except the promise delegate's
//! `writePromiseToURL:`, which AppKit calls on our background
//! `NSOperationQueue`.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{
    define_class, msg_send, sel, AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly,
    Message,
};
use objc2_app_kit::{
    NSApplication, NSDragOperation, NSDraggingContext, NSDraggingFormation, NSDraggingItem,
    NSDraggingSession, NSDraggingSource, NSEvent, NSEventModifierFlags, NSEventType,
    NSFilePromiseProvider, NSFilePromiseProviderDelegate, NSImage, NSPasteboardWriting, NSView,
    NSWindow, NSWorkspace,
};
use objc2_foundation::{
    NSArray, NSDictionary, NSError, NSInteger, NSOperationQueue, NSPoint, NSRect, NSSize, NSString,
    NSURL,
};

use super::{
    emit, pasteboard_plan, set_active, source_mask, write_promise, DragEvent, DragItem, DropOp,
    RemoteItem,
};

/// Size of each item's drag image (points).
const ICON: f64 = 64.0;

pub struct SourceIvars {
    pending: RefCell<Vec<DragItem>>,
    /// Local paths of the running session (for `Ended`).
    local: RefCell<Vec<PathBuf>>,
    all_local: Cell<bool>,
    items: Cell<usize>,
    promises: Cell<usize>,
    mask_logged: Cell<bool>,
    /// Promise delegates must outlive the session: the provider only holds
    /// them weakly and the destination may ask for the file after the drop.
    delegates: RefCell<Vec<Retained<PromiseDelegate>>>,
    /// Content view the session started on (gets the mouse-up afterwards).
    view: RefCell<Option<Retained<NSView>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MantaDragSource"]
    #[ivars = SourceIvars]
    pub struct Source;

    impl Source {
        /// Runs from the run loop, outside winit's event handler (AppKit
        /// calls made inside it may re-enter winit, which panics).
        #[unsafe(method(mantaBeginDrag))]
        fn begin_sel(&self) {
            if let Err(e) = self.begin_now() {
                set_active(false);
                emit(DragEvent::Failed(e));
            }
        }

        #[unsafe(method(mantaReleaseMouse))]
        fn release_sel(&self) {
            self.release_mouse();
        }

        /// Self-test: put the window's centre under the mouse.
        #[unsafe(method(mantaSelfTestCenter))]
        fn self_test_center_sel(&self) {
            self.self_test_center();
        }

        /// Self-test: move a little and let go (drops onto our own window).
        #[unsafe(method(mantaSelfTestDrop))]
        fn self_test_drop_sel(&self) {
            self.self_test_drop();
        }
    }

    unsafe impl NSObjectProtocol for Source {}

    unsafe impl NSDraggingSource for Source {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn mask(&self, _session: &NSDraggingSession, context: NSDraggingContext) -> NSDragOperation {
            let iv = self.ivars();
            let outside = context == NSDraggingContext::OutsideApplication;
            let cmd = NSEvent::modifierFlags_class().contains(NSEventModifierFlags::Command);
            let mask = source_mask(outside, iv.all_local.get(), cmd);
            if !iv.mask_logged.replace(true) {
                emit(DragEvent::MaskRequested { outside_app: outside, mask });
            }
            NSDragOperation(mask as _)
        }

        #[unsafe(method(draggingSession:willBeginAtPoint:))]
        fn will_begin(&self, _session: &NSDraggingSession, _point: NSPoint) {
            let iv = self.ivars();
            emit(DragEvent::Started { items: iv.items.get(), promises: iv.promises.get() });
        }

        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn ended(&self, _session: &NSDraggingSession, _point: NSPoint, operation: NSDragOperation) {
            set_active(false);
            let op = DropOp::from_bits(operation.0 as u64);
            emit(DragEvent::Ended { op, local: self.ivars().local.borrow().clone() });
            // AppKit swallowed the mouse-up: hand egui one on the next turn.
            let none: Option<&AnyObject> = None;
            unsafe {
                let _: () = msg_send![
                    self,
                    performSelector: sel!(mantaReleaseMouse),
                    withObject: none,
                    afterDelay: 0.0f64
                ];
            }
        }
    }
);

thread_local! {
    static SOURCE: RefCell<Option<Retained<Source>>> = const { RefCell::new(None) };
}

fn source(mtm: MainThreadMarker) -> Retained<Source> {
    SOURCE.with(|s| {
        s.borrow_mut()
            .get_or_insert_with(|| {
                let this = Source::alloc(mtm).set_ivars(SourceIvars {
                    pending: RefCell::new(Vec::new()),
                    local: RefCell::new(Vec::new()),
                    all_local: Cell::new(true),
                    items: Cell::new(0),
                    promises: Cell::new(0),
                    mask_logged: Cell::new(false),
                    delegates: RefCell::new(Vec::new()),
                    view: RefCell::new(None),
                });
                unsafe { msg_send![super(this), init] }
            })
            .clone()
    })
}

/// Queue a session start for the next run-loop turn. False off the main
/// thread or while a session is running.
pub fn begin(items: Vec<DragItem>) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("native drag: begin called off the main thread");
        return false;
    };
    let src = source(mtm);
    *src.ivars().pending.borrow_mut() = items;
    set_active(true);
    let none: Option<&AnyObject> = None;
    unsafe {
        let _: () = msg_send![
            &*src,
            performSelectorOnMainThread: sel!(mantaBeginDrag),
            withObject: none,
            waitUntilDone: false
        ];
    }
    true
}

/// Screen mouse location → the app window under it and the point in its
/// content view (AppKit view coordinates).
fn window_under_mouse(
    mtm: MainThreadMarker,
) -> Option<(Retained<NSWindow>, Retained<NSView>, NSPoint)> {
    let app = NSApplication::sharedApplication(mtm);
    let mouse = NSEvent::mouseLocation();
    let number = NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(mouse, 0, mtm);
    let windows = app.windows();
    let window = windows
        .iter()
        .find(|w| w.windowNumber() == number && w.contentView().is_some())
        .or_else(|| app.keyWindow())
        .or_else(|| app.mainWindow())
        .or_else(|| first_visible_window(&app))?;
    let view = window.contentView()?;
    let p = view.convertPoint_fromView(window.convertPointFromScreen(mouse), None);
    Some((window, view, p))
}

/// Fallback when the app isn't active (no key/main window, e.g. launched
/// from a terminal in the background): the first visible window with a
/// content view.
fn first_visible_window(app: &NSApplication) -> Option<Retained<NSWindow>> {
    app.windows()
        .iter()
        .find(|w| w.isVisible() && w.contentView().is_some())
}

/// Mouse position in the content view of the window under it, top-left
/// origin, in AppKit points (= winit logical pixels). `None` off the main
/// thread or with no window.
pub fn mouse_in_window() -> Option<(f64, f64)> {
    let mtm = MainThreadMarker::new()?;
    let (_, view, p) = window_under_mouse(mtm)?;
    let y = if view.isFlipped() {
        p.y
    } else {
        view.frame().size.height - p.y
    };
    Some((p.x, y))
}

/// (⌥, ⌘) held right now (works while another app owns the drag).
pub fn modifier_keys() -> (bool, bool) {
    let f = NSEvent::modifierFlags_class();
    (
        f.contains(NSEventModifierFlags::Option),
        f.contains(NSEventModifierFlags::Command),
    )
}

fn file_url(path: &Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
}

fn icon_for(item: &DragItem) -> Retained<NSImage> {
    let ws = NSWorkspace::sharedWorkspace();
    let icon = match item {
        DragItem::Local(p) => ws.iconForFile(&NSString::from_str(&p.to_string_lossy())),
        DragItem::Remote(r) => {
            let ty = if r.is_dir {
                super::UTI_FOLDER.to_string()
            } else {
                Path::new(&r.name).extension().map_or_else(
                    || super::UTI_DATA.to_string(),
                    |e| e.to_string_lossy().into_owned(),
                )
            };
            #[allow(deprecated)]
            ws.iconForFileType(&NSString::from_str(&ty))
        }
    };
    icon.setSize(NSSize::new(ICON, ICON));
    icon
}

impl Source {
    fn begin_now(&self) -> Result<(), String> {
        let mtm = self.mtm();
        let iv = self.ivars();
        let items = std::mem::take(&mut *iv.pending.borrow_mut());
        if items.is_empty() {
            return Err("nothing to drag".into());
        }
        let (window, view, at) = window_under_mouse(mtm).ok_or("no window to drag from")?;
        let app = NSApplication::sharedApplication(mtm);
        let event = app
            .currentEvent()
            .filter(|e| matches!(e.r#type(), NSEventType::LeftMouseDragged | NSEventType::LeftMouseDown))
            .or_else(|| {
                let ts = app.currentEvent().map_or(0.0, |e| e.timestamp());
                NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                    NSEventType::LeftMouseDragged,
                    window.convertPointFromScreen(NSEvent::mouseLocation()),
                    NSEventModifierFlags::empty(),
                    ts,
                    window.windowNumber(),
                    None,
                    0,
                    1,
                    1.0,
                )
            })
            .ok_or("couldn't create a drag event")?;

        // Keep delegates whose download may still be running, drop the rest.
        iv.delegates
            .borrow_mut()
            .retain(|d| d.ivars().busy.load(Ordering::SeqCst));
        let plan = pasteboard_plan(&items);
        let mut drag_items = Vec::with_capacity(items.len());
        let mut local = Vec::new();
        let mut promises = 0;
        for (i, (item, entry)) in items.iter().zip(&plan).enumerate() {
            let writer: Retained<ProtocolObject<dyn NSPasteboardWriting>> = match (item, entry) {
                (DragItem::Local(p), _) => {
                    local.push(p.clone());
                    ProtocolObject::from_retained(file_url(p))
                }
                (DragItem::Remote(r), super::PasteboardEntry::Promise { file_type, .. }) => {
                    promises += 1;
                    let d = PromiseDelegate::new(r.clone());
                    let provider = NSFilePromiseProvider::initWithFileType_delegate(
                        NSFilePromiseProvider::alloc(),
                        &NSString::from_str(file_type),
                        ProtocolObject::from_ref(&*d),
                    );
                    iv.delegates.borrow_mut().push(d);
                    ProtocolObject::from_retained(provider)
                }
                (DragItem::Remote(_), _) => unreachable!("remote items map to promises"),
            };
            let di = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), &writer);
            // A slight offset per item; AppKit stacks them (and badges the
            // count) once the drag moves.
            let off = (i.min(4) as f64) * 3.0;
            let frame = NSRect::new(
                NSPoint::new(at.x - ICON / 2.0 + off, at.y - ICON / 2.0 + off),
                NSSize::new(ICON, ICON),
            );
            let icon = icon_for(item);
            unsafe { di.setDraggingFrame_contents(frame, Some(&icon)) };
            drag_items.push(di);
        }
        iv.all_local.set(promises == 0);
        iv.items.set(items.len());
        iv.promises.set(promises);
        iv.mask_logged.set(false);
        *iv.local.borrow_mut() = local;
        *iv.view.borrow_mut() = Some(view.clone());

        let array = NSArray::from_retained_slice(&drag_items);
        let session = view.beginDraggingSessionWithItems_event_source(
            &array,
            &event,
            ProtocolObject::from_ref(self),
        );
        session.setAnimatesToStartingPositionsOnCancelOrFail(true);
        if items.len() > 1 {
            session.setDraggingFormation(NSDraggingFormation::Stack);
        }
        log::info!(
            "native drag: session on {} with {} item(s) ({} promise(s)), event {:?}",
            window.title(),
            items.len(),
            promises,
            event.r#type()
        );
        Ok(())
    }

    /// Send the content view (winit) the mouse-up AppKit kept for itself,
    /// so egui's pointer isn't stuck "pressed".
    fn release_mouse(&self) {
        let Some(view) = self.ivars().view.borrow_mut().take() else {
            return;
        };
        let Some(window) = view.window() else { return };
        if NSEvent::pressedMouseButtons() & 1 != 0 {
            return; // still down: the real mouse-up will come
        }
        let ev = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            NSEventType::LeftMouseUp,
            window.convertPointFromScreen(NSEvent::mouseLocation()),
            NSEventModifierFlags::empty(),
            NSApplication::sharedApplication(self.mtm()).currentEvent().map_or(0.0, |e| e.timestamp()),
            window.windowNumber(),
            None,
            0,
            1,
            0.0,
        );
        if let Some(ev) = ev {
            log::debug!("native drag: synthesised mouse-up for winit");
            unsafe {
                let _: () = msg_send![&*view, mouseUp: &*ev];
            }
        }
    }
}

/// Self-test helpers, run on the next run-loop turn.
pub fn self_test(step: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let src = source(mtm);
    let sel = if step == "center" {
        sel!(mantaSelfTestCenter)
    } else {
        sel!(mantaSelfTestDrop)
    };
    let none: Option<&AnyObject> = None;
    unsafe {
        let _: () = msg_send![&*src, performSelectorOnMainThread: sel, withObject: none, waitUntilDone: false];
    }
}

impl Source {
    fn self_test_center(&self) {
        let app = NSApplication::sharedApplication(self.mtm());
        let Some(window) = app
            .keyWindow()
            .or_else(|| app.mainWindow())
            .or_else(|| first_visible_window(&app))
        else {
            return;
        };
        let m = NSEvent::mouseLocation();
        let f = window.frame();
        window.setFrameOrigin(NSPoint::new(
            m.x - f.size.width / 2.0,
            m.y - f.size.height / 2.0,
        ));
        log::info!(
            "drag self-test: window centred under the mouse at ({:.0}, {:.0})",
            m.x,
            m.y
        );
    }

    fn self_test_drop(&self) {
        let app = NSApplication::sharedApplication(self.mtm());
        let Some(window) = app
            .keyWindow()
            .or_else(|| app.mainWindow())
            .or_else(|| first_visible_window(&app))
        else {
            return;
        };
        let at = window.convertPointFromScreen(NSEvent::mouseLocation());
        let ts = app.currentEvent().map_or(0.0, |e| e.timestamp());
        for (ty, dx) in [
            (NSEventType::LeftMouseDragged, 4.0),
            (NSEventType::LeftMouseDragged, 8.0),
            (NSEventType::LeftMouseUp, 8.0),
        ] {
            let ev = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                ty,
                NSPoint::new(at.x + dx, at.y),
                NSEventModifierFlags::empty(),
                ts,
                window.windowNumber(),
                None,
                0,
                1,
                if ty == NSEventType::LeftMouseUp { 0.0 } else { 1.0 },
            );
            if let Some(ev) = ev {
                app.postEvent_atStart(&ev, false);
            }
        }
        log::info!("drag self-test: posted drag + mouse-up inside the window");
    }
}

// ---------------------------------------------------------------------------
// File promises

pub struct PromiseIvars {
    item: RemoteItem,
    /// A download is running (keep the delegate alive).
    busy: AtomicBool,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MantaFilePromiseDelegate"]
    #[ivars = PromiseIvars]
    pub struct PromiseDelegate;

    unsafe impl NSObjectProtocol for PromiseDelegate {}

    unsafe impl NSFilePromiseProviderDelegate for PromiseDelegate {
        #[unsafe(method_id(filePromiseProvider:fileNameForType:))]
        fn file_name(
            &self,
            _provider: &NSFilePromiseProvider,
            _ty: &NSString,
        ) -> Retained<NSString> {
            NSString::from_str(&super::promise_file_name(&self.ivars().item.name))
        }

        /// Called on our background queue: download, then report.
        #[unsafe(method(filePromiseProvider:writePromiseToURL:completionHandler:))]
        fn write(
            &self,
            _provider: &NSFilePromiseProvider,
            url: &NSURL,
            handler: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            let _keep: Retained<Self> = self.retain();
            let iv = self.ivars();
            iv.busy.store(true, Ordering::SeqCst);
            let result = match url.to_file_path() {
                Some(dest) => write_promise(&iv.item, &dest, &mut emit),
                None => Err("the destination isn't a file URL".into()),
            };
            iv.busy.store(false, Ordering::SeqCst);
            match result {
                Ok(()) => handler.call((std::ptr::null_mut(),)),
                Err(msg) => {
                    let err = ns_error(&msg);
                    handler.call((Retained::as_ptr(&err).cast_mut(),));
                }
            }
        }

        #[unsafe(method_id(operationQueueForFilePromiseProvider:))]
        fn queue(&self, _provider: &NSFilePromiseProvider) -> Retained<NSOperationQueue> {
            NSOperationQueue::new()
        }
    }
);

// The ivars are `Send + Sync` (an `Arc` closure and an atomic); AppKit calls
// the delegate from its operation queue.
unsafe impl Send for PromiseDelegate {}
unsafe impl Sync for PromiseDelegate {}

impl PromiseDelegate {
    fn new(item: RemoteItem) -> Retained<Self> {
        let this = Self::alloc().set_ivars(PromiseIvars {
            item,
            busy: AtomicBool::new(false),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Self-test: drive a real promise delegate through its Objective-C entry
/// points (`fileNameForType:`, `operationQueueForFilePromiseProvider:`,
/// `writePromiseToURL:completionHandler:`) like a destination would, once
/// succeeding and once failing. Returns a summary or the first problem.
pub fn self_test_promises(dir: &Path) -> Result<String, String> {
    use std::sync::{Arc, Mutex};
    let mut out = Vec::new();
    for fail in [false, true] {
        let item = RemoteItem {
            backend: "self-test".into(),
            name: if fail {
                "broken/name.txt".into()
            } else {
                "promised.txt".into()
            },
            size: None,
            is_dir: false,
            fetch: Arc::new(move |dest: &Path| {
                if fail {
                    Err("simulated network error".into())
                } else {
                    std::fs::write(dest, b"promise ok").map_err(|e| e.to_string())
                }
            }),
        };
        let d = PromiseDelegate::new(item);
        let provider = NSFilePromiseProvider::initWithFileType_delegate(
            NSFilePromiseProvider::alloc(),
            &NSString::from_str(super::UTI_DATA),
            ProtocolObject::from_ref(&*d),
        );
        let name: Retained<NSString> = unsafe {
            msg_send![&*d, filePromiseProvider: &*provider, fileNameForType: &*NSString::from_str(super::UTI_DATA)]
        };
        let queue: Retained<NSOperationQueue> =
            unsafe { msg_send![&*d, operationQueueForFilePromiseProvider: &*provider] };
        let dest = dir.join(name.to_string());
        let url = file_url(&dest);
        let got: Arc<Mutex<Option<Option<String>>>> = Arc::default();
        let got2 = got.clone();
        let block = block2::RcBlock::new(move |err: *mut NSError| {
            let e = unsafe { err.as_ref() }.map(|e| e.localizedDescription().to_string());
            *got2.lock().unwrap() = Some(e);
        });
        unsafe {
            let _: () = msg_send![&*d, filePromiseProvider: &*provider, writePromiseToURL: &*url, completionHandler: &*block];
        }
        let result = got
            .lock()
            .unwrap()
            .clone()
            .ok_or("completion handler not called")?;
        match (fail, result) {
            (false, None) if std::fs::read(&dest).ok().as_deref() == Some(b"promise ok") => {
                out.push(format!("{name} written"));
            }
            (true, Some(msg)) if msg.contains("simulated") && !dest.exists() => {
                out.push(format!("{name} failed with NSError \"{msg}\""));
            }
            (f, r) => return Err(format!("unexpected promise result (fail={f}): {r:?}")),
        }
        let _ = queue;
    }
    Ok(out.join(", "))
}

fn ns_error(msg: &str) -> Retained<NSError> {
    let key: &NSString = unsafe { objc2_foundation::NSLocalizedDescriptionKey };
    let value = NSString::from_str(msg);
    let info = NSDictionary::<NSString, AnyObject>::from_slices(&[key], &[value.as_ref()]);
    unsafe {
        NSError::errorWithDomain_code_userInfo(
            &NSString::from_str("app.manta.drag"),
            1 as NSInteger,
            Some(&info),
        )
    }
}
