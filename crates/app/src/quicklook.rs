//! Space → Apple's Quick Look (`QLPreviewPanel`) for local files, like Finder.
//!
//! * [`target_for`] maps a pane's selection and cursor to the items the
//!   panel shows (pure, unit-tested).
//! * [`QuickLook`] is the toggle / follow-the-cursor state machine. It talks
//!   to a [`QuickLookBackend`]: [`native::NativeQuickLook`] drives AppKit on
//!   macOS; tests plug in a recorder, so they never touch AppKit.
//! * Remote and archive panes return no target: the app falls back to its
//!   own egui preview (`preview.rs`).
//!
//! Key focus: the panel is ordered front *without* becoming key, so the
//! Manta window keeps keyboard focus and ↑/↓, Space and Esc keep going
//! through egui (Finder behaves the same). If the user clicks into the panel
//! it becomes key; the panel delegate's `previewPanel:handleEvent:` then
//! hands key events back to the Manta window.

use std::path::PathBuf;

use manta_core::{Entry, VfsPath};

use crate::pane::Tab;

/// What the panel shows: local file URLs and the current one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QlTarget {
    pub items: Vec<PathBuf>,
    pub index: usize,
}

/// Which pane / tab / folder a Quick Look session belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QlKey {
    pub pane: usize,
    pub tab_id: u64,
    pub dir: String,
}

impl QlKey {
    pub fn of(pane: usize, tab: &Tab) -> Self {
        QlKey {
            pane,
            tab_id: tab.id,
            dir: tab.loc.path.to_string(),
        }
    }
}

/// Items for Quick Look: several selected entries (in list order, the
/// cursor's one current) or the single cursor / selected entry. Folders are
/// allowed (Quick Look shows their icon and info, like Finder). `None` when
/// any entry has no local path (remote server, archive).
pub fn target_for(
    selected: &[Entry],
    cursor: Option<&Entry>,
    local: impl Fn(&VfsPath) -> Option<PathBuf>,
) -> Option<QlTarget> {
    let (entries, index): (Vec<&Entry>, usize) = if selected.len() > 1 {
        let index = cursor
            .and_then(|c| selected.iter().position(|e| e.name == c.name))
            .unwrap_or(0);
        (selected.iter().collect(), index)
    } else {
        (cursor.or(selected.first()).into_iter().collect(), 0)
    };
    if entries.is_empty() {
        return None;
    }
    let items = entries
        .iter()
        .map(|e| local(&e.path))
        .collect::<Option<Vec<_>>>()?;
    Some(QlTarget { items, index })
}

/// [`target_for`] for a tab (archive panes never use Quick Look).
pub fn tab_target(tab: &Tab) -> Option<QlTarget> {
    if tab.loc.archive.is_some() {
        return None;
    }
    let fs = &tab.loc.fs;
    target_for(&tab.selected_entries(), tab.cursor_entry(), |p| {
        fs.local_path(p)
    })
}

/// The AppKit side, behind a trait so tests don't touch AppKit.
pub trait QuickLookBackend {
    /// Show the panel with these items (reload it when already visible).
    fn show(&mut self, items: &[PathBuf], index: usize) -> Result<(), String>;
    /// Change the current item of the visible panel.
    fn set_index(&mut self, index: usize);
    fn close(&mut self);
    /// The panel is on screen (the user may have closed it themselves).
    fn is_visible(&self) -> bool;
}

/// Result of a state machine step (for logging and tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Opened,
    /// New items (reloadData).
    Reloaded,
    /// Same items, other current one.
    Moved,
    Closed,
    Unchanged,
}

struct Session {
    key: QlKey,
    target: QlTarget,
}

/// Toggle / follow state machine on top of a backend.
pub struct QuickLook {
    backend: Option<Box<dyn QuickLookBackend>>,
    session: Option<Session>,
}

impl QuickLook {
    pub fn new(backend: Option<Box<dyn QuickLookBackend>>) -> Self {
        QuickLook {
            backend,
            session: None,
        }
    }

    /// Native Quick Look on macOS, nothing elsewhere (and in unit tests,
    /// which must never touch AppKit).
    pub fn platform_default() -> Self {
        #[cfg(all(target_os = "macos", not(test)))]
        {
            QuickLook::new(Some(Box::new(native::NativeQuickLook::default())))
        }
        #[cfg(any(not(target_os = "macos"), test))]
        {
            QuickLook::new(None)
        }
    }

    /// A backend exists (macOS).
    pub fn available(&self) -> bool {
        self.backend.is_some()
    }

    /// The panel is open (forgets the session when the user closed it).
    pub fn is_open(&mut self) -> bool {
        if self.session.is_some() && !self.backend.as_ref().is_some_and(|b| b.is_visible()) {
            log::debug!("Quick Look panel was closed outside Manta");
            self.session = None;
        }
        self.session.is_some()
    }

    pub fn open(&mut self, key: QlKey, target: QlTarget) -> Result<Step, String> {
        let backend = self
            .backend
            .as_mut()
            .ok_or_else(|| t!("ql.unavailable").to_string())?;
        backend.show(&target.items, target.index)?;
        log::info!(
            "Quick Look: {} item(s), current {}",
            target.items.len(),
            target.items[target.index].display()
        );
        self.session = Some(Session { key, target });
        Ok(Step::Opened)
    }

    pub fn close(&mut self) -> Step {
        if self.session.take().is_some() {
            if let Some(b) = self.backend.as_mut() {
                b.close();
            }
            return Step::Closed;
        }
        Step::Unchanged
    }

    /// Space: close when open, otherwise open on `target` (if any).
    pub fn toggle(&mut self, key: QlKey, target: Option<QlTarget>) -> Result<Step, String> {
        if self.is_open() {
            return Ok(self.close());
        }
        match target {
            Some(t) => self.open(key, t),
            None => Ok(Step::Unchanged),
        }
    }

    /// Called every frame with the active pane's state: follow the cursor
    /// (and pane switches); close when the folder changed or there's
    /// nothing local to show.
    pub fn follow(&mut self, key: QlKey, target: Option<QlTarget>) -> Step {
        if !self.is_open() {
            return Step::Unchanged;
        }
        let s = self.session.as_ref().expect("open");
        let same_pane = s.key.pane == key.pane;
        if same_pane && s.key != key {
            return self.close(); // navigated elsewhere or switched tab
        }
        let Some(target) = target else {
            return self.close();
        };
        if !same_pane {
            log::debug!("Quick Look follows pane {}", key.pane);
        }
        let s = self.session.as_mut().expect("open");
        s.key = key;
        if s.target == target {
            return Step::Unchanged;
        }
        let backend = self.backend.as_mut().expect("session implies backend");
        if s.target.items == target.items {
            backend.set_index(target.index);
            s.target = target;
            return Step::Moved;
        }
        match backend.show(&target.items, target.index) {
            Ok(()) => {
                s.target = target;
                Step::Reloaded
            }
            Err(e) => {
                log::warn!("Quick Look couldn't update: {e}");
                self.close()
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub mod native {
    //! `QLPreviewPanel` through objc2.
    //!
    //! The controller is an `NSResponder` subclass inserted into the Manta
    //! window's responder chain (`window.nextResponder`), so the panel finds
    //! it through `acceptsPreviewPanelControl:` and calls
    //! `beginPreviewPanelControl:` / `endPreviewPanelControl:`, which set
    //! and clear its data source and delegate (Apple's documented pattern).
    //! Should the panel not pick it up, `show` sets them directly.

    use std::cell::{Cell, RefCell};
    use std::path::{Path, PathBuf};

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{
        define_class, msg_send, sel, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly,
    };
    use objc2_app_kit::{
        NSApplication, NSEvent, NSEventType, NSResponder, NSWindow, NSWindowDelegate,
    };
    use objc2_foundation::{NSInteger, NSString, NSURL};
    use objc2_quick_look_ui::{
        QLPreviewItem, QLPreviewPanel, QLPreviewPanelDataSource, QLPreviewPanelDelegate,
    };

    use super::QuickLookBackend;

    #[derive(Default)]
    pub struct Ivars {
        urls: RefCell<Vec<Retained<NSURL>>>,
        /// Wanted state, applied by `mantaApply` on the next run-loop turn.
        index: Cell<NSInteger>,
        want_visible: Cell<bool>,
        reload: Cell<bool>,
        pending: Cell<bool>,
        opened_at: Cell<Option<std::time::Instant>>,
        /// The Manta window (gets key events back from the panel).
        window: RefCell<Option<Retained<NSWindow>>>,
    }

    define_class!(
        #[unsafe(super(NSResponder, NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "MantaQuickLookController"]
        #[ivars = Ivars]
        pub struct Controller;

        impl Controller {
            /// Runs from the run loop, outside winit's event handler: AppKit
            /// calls made *inside* it (ordering windows, changing key status)
            /// re-enter winit, which panics ("tried to handle event while
            /// another event is currently being handled").
            #[unsafe(method(mantaApply))]
            fn apply_sel(&self) {
                self.apply();
            }

            /// Shortly after opening: if the panel grabbed key status by
            /// itself (it does for folders and generic previews), give it
            /// back to Manta so ↑/↓ keep working.
            #[unsafe(method(mantaKeepKey))]
            fn keep_key_sel(&self) {
                self.keep_key();
            }

            /// Self-test only: make the panel key and type ↓ into it.
            #[unsafe(method(mantaSelfTestKey))]
            fn self_test_key_sel(&self) {
                self.self_test_key();
            }

            #[unsafe(method(acceptsPreviewPanelControl:))]
            fn accepts_control(&self, _panel: Option<&QLPreviewPanel>) -> bool {
                true
            }

            #[unsafe(method(beginPreviewPanelControl:))]
            fn begin_control(&self, panel: Option<&QLPreviewPanel>) {
                log::debug!("Quick Look: beginPreviewPanelControl");
                if let Some(panel) = panel {
                    self.attach(panel);
                }
            }

            #[unsafe(method(endPreviewPanelControl:))]
            fn end_control(&self, panel: Option<&QLPreviewPanel>) {
                log::debug!("Quick Look: endPreviewPanelControl");
                if let Some(panel) = panel {
                    unsafe {
                        panel.setDataSource(None);
                        panel.setDelegate(None);
                    }
                }
            }
        }

        unsafe impl NSObjectProtocol for Controller {}

        unsafe impl QLPreviewPanelDataSource for Controller {
            #[unsafe(method(numberOfPreviewItemsInPreviewPanel:))]
            fn number_of_items(&self, _panel: Option<&QLPreviewPanel>) -> NSInteger {
                self.ivars().urls.borrow().len() as NSInteger
            }

            #[unsafe(method_id(previewPanel:previewItemAtIndex:))]
            fn item_at(
                &self,
                _panel: Option<&QLPreviewPanel>,
                index: NSInteger,
            ) -> Option<Retained<ProtocolObject<dyn QLPreviewItem>>> {
                self.item(index)
            }
        }

        unsafe impl NSWindowDelegate for Controller {}

        unsafe impl QLPreviewPanelDelegate for Controller {
            /// Keys typed while the panel is key go back to Manta, so
            /// ↑/↓ keep moving the cursor and Space / Esc close the panel
            /// through the app's own handling.
            #[unsafe(method(previewPanel:handleEvent:))]
            fn handle_event(&self, _panel: Option<&QLPreviewPanel>, event: Option<&NSEvent>) -> bool {
                self.forward_key_event(event)
            }
        }
    );

    impl Controller {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(Ivars::default());
            unsafe { msg_send![super(this), init] }
        }

        fn item(&self, index: NSInteger) -> Option<Retained<ProtocolObject<dyn QLPreviewItem>>> {
            let urls = self.ivars().urls.borrow();
            let url = urls.get(usize::try_from(index).ok()?)?;
            Some(ProtocolObject::from_retained(url.clone()))
        }

        /// Hand key events from the (key) panel back to the Manta window.
        fn forward_key_event(&self, event: Option<&NSEvent>) -> bool {
            let Some(event) = event else { return false };
            let ty = event.r#type();
            log::debug!("Quick Look: panel event {ty:?}");
            if ty != NSEventType::KeyDown
                && ty != NSEventType::KeyUp
                && ty != NSEventType::FlagsChanged
            {
                return false;
            }
            let window = self.ivars().window.borrow().clone();
            let Some(window) = window.or_else(|| NativeQuickLook::app_window(self.mtm())) else {
                return false;
            };
            log::debug!("Quick Look: forwarding key event to the Manta window");
            window.makeKeyWindow();
            window.sendEvent(event);
            true
        }

        /// Apply the wanted state on the next run-loop turn (coalesced).
        fn schedule(&self) {
            if self.ivars().pending.replace(true) {
                return;
            }
            let none: Option<&AnyObject> = None;
            unsafe {
                let _: () = msg_send![
                    self,
                    performSelectorOnMainThread: sel!(mantaApply),
                    withObject: none,
                    waitUntilDone: false
                ];
            }
        }

        fn apply(&self) {
            let iv = self.ivars();
            iv.pending.set(false);
            let mtm = self.mtm();
            if !iv.want_visible.get() {
                if unsafe { QLPreviewPanel::sharedPreviewPanelExists(mtm) } {
                    if let Some(panel) = NativeQuickLook::panel(mtm) {
                        panel.orderOut(None);
                    }
                }
                return;
            }
            let window = NativeQuickLook::app_window(mtm);
            if let Some(w) = &window {
                NativeQuickLook::insert_into_chain(w, self);
                *iv.window.borrow_mut() = Some(w.clone());
            }
            if log::log_enabled!(log::Level::Trace) {
                if let Some(w) = &window {
                    let mut chain = Vec::new();
                    let mut r: Option<Retained<NSResponder>> = w.firstResponder();
                    while let Some(x) = r {
                        chain.push(x.class().name().to_string_lossy().into_owned());
                        r = unsafe { x.nextResponder() };
                        if chain.len() > 20 {
                            break;
                        }
                    }
                    log::trace!("Quick Look: responder chain {}", chain.join(" → "));
                }
            }
            let Some(panel) = NativeQuickLook::panel(mtm) else {
                log::warn!("Quick Look: no shared preview panel");
                return;
            };
            // Only become key when the user clicks into it.
            panel.setBecomesKeyOnlyIfNeeded(true);
            if !panel.isVisible() {
                // Front but not key: Manta keeps keyboard focus. Showing
                // the panel makes it find our controller in the responder
                // chain (beginPreviewPanelControl: attaches the data source).
                panel.orderFront(None);
                iv.opened_at.set(Some(std::time::Instant::now()));
                for delay in [0.05f64, 0.3, 0.7] {
                    let none: Option<&AnyObject> = None;
                    unsafe {
                        let _: () = msg_send![
                            self,
                            performSelector: sel!(mantaKeepKey),
                            withObject: none,
                            afterDelay: delay
                        ];
                    }
                }
            }
            unsafe {
                let ours = (self as *const Self).cast::<AnyObject>();
                let in_control = panel
                    .currentController()
                    .is_some_and(|c| std::ptr::eq(Retained::as_ptr(&c), ours));
                if !in_control {
                    // Not reachable through the responder chain (no key
                    // window): drive the panel directly.
                    log::debug!("Quick Look: controller not in the responder chain, setting the data source directly");
                    self.attach(&panel);
                }
                if iv.reload.replace(false) {
                    panel.reloadData();
                }
                panel.setCurrentPreviewItemIndex(iv.index.get());
            }
            if let Some(w) = &window {
                if !w.isKeyWindow() {
                    w.makeKeyWindow();
                }
            }
        }

        fn keep_key(&self) {
            let opened = self.ivars().opened_at.get();
            if !opened.is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(1000)) {
                return; // the user may have clicked the panel since
            }
            let mtm = self.mtm();
            let Some(panel) = NativeQuickLook::panel(mtm) else {
                return;
            };
            if panel.isVisible() && panel.isKeyWindow() {
                if let Some(w) = self.ivars().window.borrow().as_ref() {
                    log::debug!("Quick Look: panel took key status, returning it to Manta");
                    w.makeKeyWindow();
                }
            }
        }

        fn self_test_key(&self) {
            let mtm = self.mtm();
            let Some(panel) = NativeQuickLook::panel(mtm) else {
                return;
            };
            panel.makeKeyWindow();
            log::info!("QL self-test: panel made key = {}", panel.isKeyWindow());
            let down = NSString::from_str("\u{F701}");
            for ty in [NSEventType::KeyDown, NSEventType::KeyUp] {
                let ev = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
                    ty,
                    objc2_foundation::NSPoint::new(0.0, 0.0),
                    objc2_app_kit::NSEventModifierFlags::empty(),
                    0.0,
                    panel.windowNumber(),
                    None,
                    &down,
                    &down,
                    false,
                    125, // kVK_DownArrow
                );
                if let Some(ev) = ev {
                    NSApplication::sharedApplication(mtm).postEvent_atStart(&ev, false);
                }
            }
        }

        fn attach(&self, panel: &QLPreviewPanel) {
            unsafe {
                panel.setDataSource(Some(ProtocolObject::from_ref(self)));
                let delegate: &AnyObject = self.as_ref();
                panel.setDelegate(Some(delegate));
            }
        }
    }

    fn is_same(obj: &Retained<AnyObject>, controller: &Retained<Controller>) -> bool {
        std::ptr::eq(
            Retained::as_ptr(obj),
            Retained::as_ptr(controller).cast::<AnyObject>(),
        )
    }

    fn file_url(path: &Path) -> Retained<NSURL> {
        NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
    }

    /// The real panel. Must be used on the main thread (eframe's UI thread
    /// on macOS); calls from elsewhere return an error / do nothing.
    #[derive(Default)]
    pub struct NativeQuickLook {
        controller: Option<Retained<Controller>>,
    }

    impl NativeQuickLook {
        fn controller(&mut self, mtm: MainThreadMarker) -> Retained<Controller> {
            self.controller
                .get_or_insert_with(|| Controller::new(mtm))
                .clone()
        }

        /// The Manta window: key or main window that isn't the panel.
        fn app_window(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
            let app = NSApplication::sharedApplication(mtm);
            let is_panel = |w: &NSWindow| w.isKindOfClass(QLPreviewPanel::class());
            app.keyWindow()
                .filter(|w| !is_panel(w))
                .or_else(|| app.mainWindow().filter(|w| !is_panel(w)))
        }

        /// Put the controller right after the window in its responder chain.
        fn insert_into_chain(window: &NSWindow, controller: &Controller) {
            let ours: &NSResponder = controller.as_ref();
            unsafe {
                let next = window.nextResponder();
                if next.as_deref().is_some_and(|n| std::ptr::eq(n, ours)) {
                    return;
                }
                controller.setNextResponder(next.as_deref());
                window.setNextResponder(Some(ours));
            }
        }

        fn panel(mtm: MainThreadMarker) -> Option<Retained<QLPreviewPanel>> {
            unsafe { QLPreviewPanel::sharedPreviewPanel(mtm) }
        }

        /// Self-test: make the panel key and send it ↓ (on the next run-loop turn).
        pub fn self_test_key(&self) {
            if let Some(c) = &self.controller {
                unsafe {
                    let none: Option<&AnyObject> = None;
                    let _: () = msg_send![
                        &**c,
                        performSelectorOnMainThread: sel!(mantaSelfTestKey),
                        withObject: none,
                        waitUntilDone: false
                    ];
                }
            }
        }

        /// Panel facts for the self-test log.
        pub fn describe(&self) -> String {
            let Some(mtm) = MainThreadMarker::new() else {
                return "not on the main thread".into();
            };
            if !unsafe { QLPreviewPanel::sharedPreviewPanelExists(mtm) } {
                return "panel not created".into();
            }
            let Some(panel) = Self::panel(mtm) else {
                return "no shared panel".into();
            };
            let controller_ok = unsafe { panel.currentController() }.is_some_and(|c| {
                self.controller
                    .as_ref()
                    .is_some_and(|ours| is_same(&c, ours))
            });
            let app_key = Self::app_window(mtm).is_some_and(|w| w.isKeyWindow());
            let f = panel.frame();
            format!(
                "isVisible={} isKeyWindow(panel)={} mantaWindowKey={} controllerInResponderChain={} items={} current={} frame={:.0}x{:.0}",
                panel.isVisible(),
                panel.isKeyWindow(),
                app_key,
                controller_ok,
                unsafe { panel.dataSource() }.map_or(0, |_| self
                    .controller
                    .as_ref()
                    .map_or(0, |c| c.ivars().urls.borrow().len())),
                unsafe { panel.currentPreviewItemIndex() },
                f.size.width,
                f.size.height
            )
        }
    }

    impl QuickLookBackend for NativeQuickLook {
        fn show(&mut self, items: &[PathBuf], index: usize) -> Result<(), String> {
            let mtm = MainThreadMarker::new().ok_or("Quick Look must run on the main thread")?;
            let c = self.controller(mtm);
            let iv = c.ivars();
            *iv.urls.borrow_mut() = items.iter().map(|p| file_url(p)).collect();
            iv.index.set(index as NSInteger);
            iv.reload.set(true);
            iv.want_visible.set(true);
            c.schedule();
            Ok(())
        }

        fn set_index(&mut self, index: usize) {
            if let Some(c) = &self.controller {
                c.ivars().index.set(index as NSInteger);
                c.schedule();
            }
        }

        fn close(&mut self) {
            if let Some(c) = &self.controller {
                c.ivars().want_visible.set(false);
                c.schedule();
            }
        }

        fn is_visible(&self) -> bool {
            let Some(c) = &self.controller else {
                return false;
            };
            if c.ivars().pending.get() {
                return c.ivars().want_visible.get();
            }
            let mtm = c.mtm();
            let exists = unsafe { QLPreviewPanel::sharedPreviewPanelExists(mtm) };
            exists && Self::panel(mtm).is_some_and(|p| p.isVisible())
        }
    }

    /// `MANTA_QL_SELFTEST=file[:file…]`: show the real panel for 2 s in
    /// a tiny window, log what AppKit reports, then quit.
    pub struct SelfTest {
        paths: Vec<PathBuf>,
        ql: NativeQuickLook,
        started: Option<std::time::Instant>,
        stage: u8,
        got_arrow: bool,
        first_frame: Option<std::time::Instant>,
    }

    impl SelfTest {
        pub fn new(spec: &str) -> Self {
            SelfTest {
                paths: std::env::split_paths(spec).collect(),
                ql: NativeQuickLook::default(),
                started: None,
                stage: 0,
                got_arrow: false,
                first_frame: None,
            }
        }

        pub fn run(spec: &str) -> eframe::Result {
            let options = eframe::NativeOptions {
                viewport: eframe::egui::ViewportBuilder::default()
                    .with_title("Manta Quick Look self-test")
                    .with_inner_size([420.0, 140.0]),
                ..Default::default()
            };
            let test = SelfTest::new(spec);
            eframe::run_native(
                "Manta QL self-test",
                options,
                Box::new(|_| Ok(Box::new(test))),
            )
        }
    }

    impl eframe::App for SelfTest {
        fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
            let ctx = ui.ctx().clone();
            ui.label(t!("ql.selftest"));
            for p in &self.paths {
                ui.monospace(p.display().to_string());
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
            if ui.input(|i| i.key_pressed(eframe::egui::Key::ArrowDown)) {
                self.got_arrow = true;
                log::info!("QL self-test: Manta received ↓ typed into the key panel (forwarded by previewPanel:handleEvent:)");
            }
            let first = *self.first_frame.get_or_insert_with(std::time::Instant::now);
            let Some(started) = self.started else {
                // Like a real Space press: wait until the window is key.
                if first.elapsed().as_millis() < 600 {
                    return;
                }
                log::info!(
                    "QL self-test: main thread = {}, {} file(s)",
                    MainThreadMarker::new().is_some(),
                    self.paths.len()
                );
                for p in &self.paths {
                    log::info!(
                        "QL self-test: {} (exists={}, readable={})",
                        p.display(),
                        p.exists(),
                        std::fs::File::open(p).is_ok()
                    );
                }
                if let Err(e) = self.ql.show(&self.paths, 0) {
                    log::error!("QL self-test: show failed: {e}");
                }
                self.started = Some(std::time::Instant::now());
                return;
            };
            let ms = started.elapsed().as_millis();
            let step = |s: &mut u8, n: u8, at: u128| {
                ms >= at && *s < n && {
                    *s = n;
                    true
                }
            };
            if step(&mut self.stage, 1, 400) {
                log::info!("QL self-test: opened: {}", self.ql.describe());
            }
            if step(&mut self.stage, 2, 900) && self.paths.len() > 1 {
                self.ql.set_index(1);
            }
            if step(&mut self.stage, 3, 1100) && self.paths.len() > 1 {
                log::info!("QL self-test: moved to item 2: {}", self.ql.describe());
            }
            if step(&mut self.stage, 4, 1300) {
                self.ql.self_test_key();
            }
            if step(&mut self.stage, 5, 2000) {
                let visible = self.ql.is_visible();
                log::info!(
                    "QL self-test RESULT: panel visible after 2 s = {visible}; arrow forwarded = {}; {}",
                    self.got_arrow,
                    self.ql.describe()
                );
                self.ql.close();
            }
            if step(&mut self.stage, 6, 2400) {
                log::info!(
                    "QL self-test: closed, panel visible now = {}",
                    self.ql.is_visible()
                );
                ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    use manta_core::EntryKind;

    /// Records backend calls; the "panel" stays visible until closed.
    #[derive(Default, Clone)]
    pub struct Recorder {
        pub calls: Rc<RefCell<Vec<String>>>,
        pub visible: Rc<RefCell<bool>>,
    }

    impl QuickLookBackend for Recorder {
        fn show(&mut self, items: &[PathBuf], index: usize) -> Result<(), String> {
            let names: Vec<_> = items
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            self.calls
                .borrow_mut()
                .push(format!("show {} @{index}", names.join(",")));
            *self.visible.borrow_mut() = true;
            Ok(())
        }
        fn set_index(&mut self, index: usize) {
            self.calls.borrow_mut().push(format!("index {index}"));
        }
        fn close(&mut self) {
            self.calls.borrow_mut().push("close".into());
            *self.visible.borrow_mut() = false;
        }
        fn is_visible(&self) -> bool {
            *self.visible.borrow()
        }
    }

    fn entry(name: &str, dir: bool) -> Entry {
        Entry {
            name: name.into(),
            path: VfsPath::new(&format!("/data/{name}")),
            kind: if dir {
                EntryKind::Directory
            } else {
                EntryKind::File
            },
            size: 1,
            modified: None,
            permissions: None,
            link_target: None,
            hidden: false,
        }
    }

    fn local(p: &VfsPath) -> Option<PathBuf> {
        Some(PathBuf::from(p.as_str()))
    }

    fn names(t: &QlTarget) -> Vec<String> {
        t.items
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn single_cursor_entry() {
        let a = entry("a.png", false);
        let t = target_for(std::slice::from_ref(&a), Some(&a), local).unwrap();
        assert_eq!(names(&t), ["a.png"]);
        assert_eq!(t.index, 0);
        assert_eq!(t.items[0], PathBuf::from("/data/a.png"));
    }

    #[test]
    fn multi_select_keeps_list_order_and_cursor_index() {
        let (a, b, c) = (entry("a", false), entry("b", false), entry("c", false));
        let t = target_for(&[a.clone(), b.clone(), c.clone()], Some(&c), local).unwrap();
        assert_eq!(names(&t), ["a", "b", "c"]);
        assert_eq!(t.index, 2);
        // Cursor outside the selection: start at the first item.
        let d = entry("d", false);
        let t = target_for(&[a, b], Some(&d), local).unwrap();
        assert_eq!(t.index, 0);
    }

    #[test]
    fn folders_are_previewed_and_empty_gives_none() {
        let f = entry("Folder", true);
        let t = target_for(std::slice::from_ref(&f), Some(&f), local).unwrap();
        assert_eq!(names(&t), ["Folder"]);
        assert!(target_for(&[], None, local).is_none());
    }

    #[test]
    fn remote_entries_fall_back() {
        let a = entry("a", false);
        assert!(target_for(std::slice::from_ref(&a), Some(&a), |_| None).is_none());
        // Mixed (shouldn't happen, but never show a partial list).
        let b = entry("b", false);
        let only_a = |p: &VfsPath| (p.as_str() == "/data/a").then(|| PathBuf::from(p.as_str()));
        assert!(target_for(&[a, b], None, only_a).is_none());
    }

    fn key(pane: usize, dir: &str) -> QlKey {
        QlKey {
            pane,
            tab_id: pane as u64 + 1,
            dir: dir.into(),
        }
    }

    fn target(items: &[&str], index: usize) -> QlTarget {
        QlTarget {
            items: items
                .iter()
                .map(|n| PathBuf::from(format!("/d/{n}")))
                .collect(),
            index,
        }
    }

    #[test]
    fn toggle_follow_close() {
        let rec = Recorder::default();
        let mut ql = QuickLook::new(Some(Box::new(rec.clone())));
        assert!(!ql.is_open());
        assert_eq!(
            ql.toggle(key(0, "/d"), Some(target(&["a"], 0))),
            Ok(Step::Opened)
        );
        assert!(ql.is_open());
        // Cursor moves: reload with the new item.
        assert_eq!(
            ql.follow(key(0, "/d"), Some(target(&["b"], 0))),
            Step::Reloaded
        );
        assert_eq!(
            ql.follow(key(0, "/d"), Some(target(&["b"], 0))),
            Step::Unchanged
        );
        // Same multi-selection, other current item: just move.
        assert_eq!(
            ql.follow(key(0, "/d"), Some(target(&["a", "b"], 0))),
            Step::Reloaded
        );
        assert_eq!(
            ql.follow(key(0, "/d"), Some(target(&["a", "b"], 1))),
            Step::Moved
        );
        // Space again closes.
        assert_eq!(
            ql.toggle(key(0, "/d"), Some(target(&["a"], 0))),
            Ok(Step::Closed)
        );
        assert!(!ql.is_open());
        assert_eq!(
            ql.follow(key(0, "/d"), Some(target(&["a"], 0))),
            Step::Unchanged
        );
        assert_eq!(
            *rec.calls.borrow(),
            ["show a @0", "show b @0", "show a,b @0", "index 1", "close"]
        );
    }

    #[test]
    fn navigating_or_losing_the_target_closes() {
        let rec = Recorder::default();
        let mut ql = QuickLook::new(Some(Box::new(rec.clone())));
        ql.toggle(key(0, "/d"), Some(target(&["a"], 0))).unwrap();
        assert_eq!(
            ql.follow(key(0, "/other"), Some(target(&["x"], 0))),
            Step::Closed
        );
        ql.toggle(key(0, "/d"), Some(target(&["a"], 0))).unwrap();
        // Switching to the other pane follows it…
        assert_eq!(
            ql.follow(key(1, "/e"), Some(target(&["z"], 0))),
            Step::Reloaded
        );
        // …and a remote pane (no local target) closes it.
        assert_eq!(ql.follow(key(0, "/d"), None), Step::Closed);
        assert!(!ql.is_open());
    }

    #[test]
    fn panel_closed_by_user_is_noticed() {
        let rec = Recorder::default();
        let mut ql = QuickLook::new(Some(Box::new(rec.clone())));
        ql.toggle(key(0, "/d"), Some(target(&["a"], 0))).unwrap();
        *rec.visible.borrow_mut() = false; // red close button
        assert!(!ql.is_open());
        // Space opens it again instead of "closing" it.
        assert_eq!(
            ql.toggle(key(0, "/d"), Some(target(&["a"], 0))),
            Ok(Step::Opened)
        );
    }

    #[test]
    fn no_backend_means_unavailable() {
        let mut ql = QuickLook::new(None);
        assert!(!ql.available());
        assert!(ql.toggle(key(0, "/d"), Some(target(&["a"], 0))).is_err());
        assert_eq!(ql.toggle(key(0, "/d"), None), Ok(Step::Unchanged));
    }
}
