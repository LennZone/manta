use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::*;

pub(crate) const ME: &str = "com.manta-filemanager.manta";
pub(crate) const FORKLIFT: &str = "com.binarynights.ForkLift";

/// In-memory macOS: one global NSFileViewer, one folder handler, a set of
/// installed apps, and a log of the calls in order.
pub(crate) struct FakeOs {
    bundle: Option<(String, PathBuf)>,
    /// `set_folder_handler` fails (LaunchServices refused).
    folder_fails: bool,
    state: Mutex<FakeState>,
}

#[derive(Default)]
struct FakeState {
    file_viewer: Option<String>,
    folder: Option<String>,
    installed: HashMap<String, String>,
    calls: Vec<String>,
}

impl FakeOs {
    fn new(app_path: &str) -> Self {
        let mut installed = HashMap::new();
        installed.insert(FORKLIFT.to_string(), "ForkLift".to_string());
        installed.insert(FINDER_ID.to_string(), "Finder".to_string());
        installed.insert(ME.to_string(), "Manta".to_string());
        FakeOs {
            bundle: Some((ME.into(), PathBuf::from(app_path))),
            folder_fails: false,
            state: Mutex::new(FakeState {
                file_viewer: Some(FORKLIFT.into()),
                // LaunchServices reports lower-case ids.
                folder: Some(FINDER_ID.into()),
                installed,
                calls: Vec::new(),
            }),
        }
    }
    fn st(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state.lock().unwrap()
    }
}

impl ViewerOs for FakeOs {
    fn bundle_id(&self) -> Option<String> {
        self.bundle.as_ref().map(|b| b.0.clone())
    }
    fn bundle_path(&self) -> Option<PathBuf> {
        self.bundle.as_ref().map(|b| b.1.clone())
    }
    fn home_dir(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/Users/anna"))
    }
    fn file_viewer(&self) -> Option<String> {
        self.st().file_viewer.clone()
    }
    fn set_file_viewer(&self, id: Option<&str>) -> Result<(), String> {
        let mut s = self.st();
        s.calls.push(format!("fv={id:?}"));
        s.file_viewer = id.map(str::to_string);
        Ok(())
    }
    fn folder_handler(&self) -> Option<String> {
        self.st().folder.clone()
    }
    fn set_folder_handler(&self, id: &str) -> Result<(), String> {
        let mut s = self.st();
        s.calls.push(format!("folder={id}"));
        if self.folder_fails {
            return Err("OSStatus -50".into());
        }
        s.folder = Some(id.to_lowercase());
        Ok(())
    }
    fn app_exists(&self, id: &str) -> bool {
        self.st()
            .installed
            .keys()
            .any(|k| k.eq_ignore_ascii_case(id))
    }
    fn app_name(&self, id: &str) -> Option<String> {
        self.st()
            .installed
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(id))
            .map(|(_, v)| v.clone())
    }
}

pub(crate) fn bundled() -> FakeOs {
    FakeOs::new("/Applications/Manta.app")
}

// ------------------------------------------------------------------ availability

#[test]
fn available_only_from_applications_folders() {
    assert_eq!(available(&bundled()), Ok(()));
    assert_eq!(
        available(&FakeOs::new("/Users/anna/Applications/Manta.app")),
        Ok(())
    );
    let dmg = FakeOs::new("/Volumes/Manta/Manta.app");
    assert_eq!(
        available(&dmg),
        Err(Unavailable::NotInApplications(PathBuf::from(
            "/Volumes/Manta/Manta.app"
        )))
    );
    let mut unbundled = bundled();
    unbundled.bundle = None;
    let r = available(&unbundled);
    assert!(
        matches!(
            r,
            Err(Unavailable::NotBundled) | Err(Unavailable::Unsupported)
        ),
        "{r:?}"
    );
    assert!(available(&Unsupported).is_err());
}

#[test]
fn holder_and_is_default() {
    let os = bundled();
    assert_eq!(
        current_holder(&os),
        Holder {
            id: FORKLIFT.into(),
            name: "ForkLift".into()
        }
    );
    assert!(!is_default(&os));
    os.st().file_viewer = None;
    assert_eq!(current_holder(&os).name, "Finder");
    os.st().file_viewer = Some(ME.into());
    assert!(is_default(&os), "NSFileViewer is what makes it the default");
    assert!(!holds_folders(&os));
    os.st().folder = Some(ME.into());
    assert!(holds_folders(&os));
}

// ------------------------------------------------------------------ save / restore

#[test]
fn enable_saves_previous_holders_before_switching() {
    let os = bundled();
    let mut prefs = ViewerPrefs::default();
    let saved: RefCell<Vec<(ViewerPrefs, usize)>> = RefCell::new(Vec::new());
    let mut save = |p: &ViewerPrefs| {
        // How many OS changes happened before this save.
        saved.borrow_mut().push((p.clone(), os.st().calls.len()));
        Ok(())
    };
    set_default(&os, &mut prefs, true, &mut save).unwrap();
    let saved = saved.into_inner();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].1, 0, "prefs saved before any OS change");
    assert_eq!(saved[0].0.prev_file_viewer.as_deref(), Some(FORKLIFT));
    assert_eq!(saved[0].0.prev_folder_handler.as_deref(), Some(FINDER_ID));
    assert_eq!(
        os.st().calls,
        vec![format!("fv={:?}", Some(ME)), format!("folder={ME}")]
    );
    assert!(is_default(&os));

    // Enabling again keeps the original previous holders (not ourselves).
    set_default(&os, &mut prefs, true, &mut |_| Ok(())).unwrap();
    assert_eq!(prefs.prev_file_viewer.as_deref(), Some(FORKLIFT));
    assert_eq!(prefs.prev_folder_handler.as_deref(), Some(FINDER_ID));
}

#[test]
fn enable_stops_when_saving_fails() {
    let os = bundled();
    let mut prefs = ViewerPrefs::default();
    let r = set_default(&os, &mut prefs, true, &mut |_| Err("disk full".into()));
    assert_eq!(r, Err("disk full".into()));
    assert!(
        os.st().calls.is_empty(),
        "nothing changed without a saved backup"
    );
}

#[test]
fn folder_role_refused_keeps_file_viewer() {
    // macOS 26: LaunchServices answers paramErr for public.folder.
    let mut os = bundled();
    os.folder_fails = true;
    let mut prefs = ViewerPrefs::default();
    set_default(&os, &mut prefs, true, &mut |_| Ok(())).unwrap();
    assert!(is_default(&os), "Show in Finder works");
    assert!(!holds_folders(&os));
    assert_eq!(os.folder_handler().as_deref(), Some(FINDER_ID));
    // Switching off touches only what is ours.
    os.st().calls.clear();
    set_default(&os, &mut prefs, false, &mut |_| Ok(())).unwrap();
    assert_eq!(os.file_viewer().as_deref(), Some(FORKLIFT));
    assert_eq!(os.st().calls, vec![format!("fv={:?}", Some(FORKLIFT))]);
}

#[test]
fn enable_refused_when_unavailable() {
    let os = FakeOs::new("/Users/anna/Downloads/Manta.app");
    let mut prefs = ViewerPrefs::default();
    assert!(set_default(&os, &mut prefs, true, &mut |_| Ok(())).is_err());
    assert!(os.st().calls.is_empty());
}

#[test]
fn disable_restores_exact_previous_values() {
    let os = bundled();
    let mut prefs = ViewerPrefs::default();
    set_default(&os, &mut prefs, true, &mut |_| Ok(())).unwrap();
    assert_eq!(restore_target(&os, &prefs).name, "ForkLift");
    set_default(&os, &mut prefs, false, &mut |_| Ok(())).unwrap();
    assert_eq!(os.file_viewer().as_deref(), Some(FORKLIFT));
    assert_eq!(os.folder_handler().as_deref(), Some(FINDER_ID));
    assert_eq!(
        prefs.prev_file_viewer, None,
        "backup cleared after restoring"
    );
}

#[test]
fn disable_with_previous_app_gone_falls_back_to_finder() {
    let os = bundled();
    os.st().folder = Some("com.binarynights.forklift".into());
    let mut prefs = ViewerPrefs::default();
    set_default(&os, &mut prefs, true, &mut |_| Ok(())).unwrap();
    os.st().installed.remove(FORKLIFT);
    assert_eq!(restore_target(&os, &prefs).name, "Finder");
    set_default(&os, &mut prefs, false, &mut |_| Ok(())).unwrap();
    assert_eq!(os.file_viewer(), None, "NSFileViewer removed");
    assert_eq!(os.folder_handler().as_deref(), Some(FINDER_ID));
}

#[test]
fn disable_with_no_previous_viewer_removes_it() {
    let os = bundled();
    os.st().file_viewer = None;
    let mut prefs = ViewerPrefs::default();
    set_default(&os, &mut prefs, true, &mut |_| Ok(())).unwrap();
    assert_eq!(prefs.prev_file_viewer, None);
    set_default(&os, &mut prefs, false, &mut |_| Ok(())).unwrap();
    assert_eq!(os.file_viewer(), None);
}

#[test]
fn disable_leaves_values_another_app_took_meanwhile() {
    let os = bundled();
    let mut prefs = ViewerPrefs::default();
    set_default(&os, &mut prefs, true, &mut |_| Ok(())).unwrap();
    os.st().file_viewer = Some("com.cocoatech.PathFinder".into());
    os.st().calls.clear();
    set_default(&os, &mut prefs, false, &mut |_| Ok(())).unwrap();
    assert_eq!(
        os.file_viewer().as_deref(),
        Some("com.cocoatech.PathFinder")
    );
    assert_eq!(os.st().calls, vec![format!("folder={FINDER_ID}")]);
}

// ------------------------------------------------------------------ prompt

#[test]
fn prompt_state_machine() {
    let mut p = ViewerPrefs::default();
    assert!(should_prompt(true, false, &p), "first launch asks");
    assert!(!should_prompt(false, false, &p), "unavailable: never asks");
    assert!(!should_prompt(true, true, &p), "already default: no prompt");

    for i in 1..=MAX_NOT_NOW {
        record_choice(&mut p, PromptChoice::NotNow);
        assert_eq!(p.not_now_count, i);
        assert_eq!(
            should_prompt(true, false, &p),
            i < MAX_NOT_NOW,
            "after {i} × not now"
        );
    }

    let mut p = ViewerPrefs::default();
    record_choice(&mut p, PromptChoice::Never);
    assert_eq!(p.prompt_state, PromptState::Never);
    assert!(!should_prompt(true, false, &p));

    let mut p = ViewerPrefs::default();
    record_choice(&mut p, PromptChoice::SetDefault);
    assert_eq!(p.prompt_state, PromptState::Answered);
    assert!(
        !should_prompt(true, false, &p),
        "answered: even if it was undone later"
    );
}

#[test]
fn controller_prompts_once_and_persists_answer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("default-viewer.json");
    let os: Arc<dyn ViewerOs> = Arc::new(bundled());
    let mut dv = DefaultViewer::new(os.clone(), Some(path.clone()));
    dv.maybe_prompt();
    assert!(dv.prompt_open);
    dv.answer(PromptChoice::NotNow);
    assert!(!dv.prompt_open);
    assert_eq!(ViewerPrefs::load(&path).not_now_count, 1);

    // Next launch asks again; "Set as default" switches in the background.
    let mut dv = DefaultViewer::new(os.clone(), Some(path.clone()));
    dv.maybe_prompt();
    assert!(dv.prompt_open);
    dv.answer(PromptChoice::SetDefault);
    assert!(dv.status.busy);
    assert_eq!(dv.wait(), Some(Ok(())));
    assert!(dv.status.is_default);
    let saved = ViewerPrefs::load(&path);
    assert_eq!(saved.prompt_state, PromptState::Answered);
    assert_eq!(saved.prev_file_viewer.as_deref(), Some(FORKLIFT));

    // Already default: no prompt.
    let mut dv = DefaultViewer::new(os.clone(), Some(path.clone()));
    dv.maybe_prompt();
    assert!(!dv.prompt_open);
    assert_eq!(dv.status.restore_to.name, "ForkLift");

    // Switching off in Settings restores ForkLift.
    dv.switch(false);
    assert_eq!(dv.wait(), Some(Ok(())));
    assert!(!dv.status.is_default);
    assert_eq!(os.file_viewer().as_deref(), Some(FORKLIFT));
    assert_eq!(dv.status.holder.name, "ForkLift");
}

#[test]
fn viewer_prefs_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dv.json");
    assert_eq!(ViewerPrefs::load(&path), ViewerPrefs::default());
    let p = ViewerPrefs {
        prev_file_viewer: Some(FORKLIFT.into()),
        prev_folder_handler: Some(FINDER_ID.into()),
        prompt_state: PromptState::Never,
        not_now_count: 2,
    };
    p.save(&path).unwrap();
    assert_eq!(ViewerPrefs::load(&path), p);
    let json = std::fs::read_to_string(&path).unwrap();
    assert!(json.contains("\"prompt_state\": \"never\""), "{json}");
}

// ------------------------------------------------------------------ requests

fn dirs_are(dirs: &'static [&'static str]) -> impl Fn(&Path) -> bool {
    move |p: &Path| dirs.iter().any(|d| Path::new(d) == p)
}

#[test]
fn plan_folder_file_and_many() {
    let is_dir = dirs_are(&["/Users/anna/Downloads", "/Users/anna/Music"]);
    let open = |paths: &[&str]| OpenRequest {
        kind: RequestKind::Open,
        paths: paths.iter().map(PathBuf::from).collect(),
    };
    let reveal = |paths: &[&str]| OpenRequest {
        kind: RequestKind::Reveal,
        paths: paths.iter().map(PathBuf::from).collect(),
    };

    // A folder opens as itself.
    assert_eq!(
        plan(&open(&["/Users/anna/Downloads"]), &is_dir),
        vec![OpenPlan {
            dir: "/Users/anna/Downloads".into(),
            select: vec![]
        }]
    );
    // Two folders: two tabs.
    assert_eq!(
        plan(
            &open(&["/Users/anna/Downloads", "/Users/anna/Music"]),
            &is_dir
        )
        .len(),
        2
    );
    // A file: its parent with it selected.
    assert_eq!(
        plan(&reveal(&["/Users/anna/Downloads/a.pdf"]), &is_dir),
        vec![OpenPlan {
            dir: "/Users/anna/Downloads".into(),
            select: vec!["a.pdf".into()]
        }]
    );
    // Revealing a folder selects it in its parent (like Finder).
    assert_eq!(
        plan(&reveal(&["/Users/anna/Downloads"]), &is_dir),
        vec![OpenPlan {
            dir: "/Users/anna".into(),
            select: vec!["Downloads".into()]
        }]
    );
    // Several files: parent of the first, all of them that live there.
    assert_eq!(
        plan(
            &reveal(&[
                "/Users/anna/Downloads/a.pdf",
                "/Users/anna/Downloads/b.png",
                "/tmp/elsewhere.txt"
            ]),
            &is_dir
        ),
        vec![OpenPlan {
            dir: "/Users/anna/Downloads".into(),
            select: vec!["a.pdf".into(), "b.png".into()]
        }]
    );
    // Relative paths and empty requests are ignored.
    assert!(plan(&open(&["relative/x"]), &is_dir).is_empty());
    assert!(plan(&open(&[]), &is_dir).is_empty());
}

#[test]
fn request_queue_drains() {
    // Other tests may push too; only check ours arrive in order.
    let a = OpenRequest {
        kind: RequestKind::Open,
        paths: vec!["/q/a".into()],
    };
    let b = OpenRequest {
        kind: RequestKind::Reveal,
        paths: vec!["/q/b".into()],
    };
    push_request(a.clone());
    push_request(b.clone());
    let got: Vec<OpenRequest> = take_requests()
        .into_iter()
        .filter(|r| r.paths[0].starts_with("/q"))
        .collect();
    assert_eq!(got, vec![a, b]);
}

// ------------------------------------------------------------------ migration from RustLift

#[test]
fn old_bundle_id_role_moves_to_the_new_id() {
    let os = bundled();
    os.st().file_viewer = Some(OLD_BUNDLE_ID.into());
    os.st().folder = Some(OLD_BUNDLE_ID.into());
    let mut prefs = ViewerPrefs {
        prev_file_viewer: Some(FORKLIFT.into()),
        prompt_state: PromptState::Answered,
        ..Default::default()
    };
    let done = adopt_old_bundle_id(&os, &mut prefs);
    assert_eq!(done.len(), 2, "{done:?}");
    assert_eq!(os.file_viewer().as_deref(), Some(ME));
    assert!(is_default(&os));
    assert!(holds_folders(&os));
    // The previous holder (to give the role back to) is kept.
    assert_eq!(prefs.prev_file_viewer.as_deref(), Some(FORKLIFT));
    // Second run: nothing left to do.
    assert!(adopt_old_bundle_id(&os, &mut prefs).is_empty());
}

#[test]
fn old_bundle_id_saved_as_previous_holder_is_dropped() {
    let os = bundled();
    os.st().file_viewer = Some(OLD_BUNDLE_ID.into());
    let mut prefs = ViewerPrefs {
        prev_file_viewer: Some(OLD_BUNDLE_ID.into()),
        ..Default::default()
    };
    adopt_old_bundle_id(&os, &mut prefs);
    assert_eq!(os.file_viewer().as_deref(), Some(ME));
    assert_eq!(prefs.prev_file_viewer, None);
}

#[test]
fn another_file_manager_holding_the_role_is_never_touched() {
    let os = bundled(); // NSFileViewer = ForkLift
    let mut prefs = ViewerPrefs {
        prev_file_viewer: Some(FINDER_ID.into()),
        prompt_state: PromptState::Answered,
        ..Default::default()
    };
    assert!(adopt_old_bundle_id(&os, &mut prefs).is_empty());
    assert_eq!(os.file_viewer().as_deref(), Some(FORKLIFT));
    assert!(os.st().calls.is_empty());
}

#[test]
fn old_bundle_id_without_saved_state_is_left_alone() {
    let os = bundled();
    os.st().file_viewer = Some(OLD_BUNDLE_ID.into());
    let mut prefs = ViewerPrefs::default();
    assert!(adopt_old_bundle_id(&os, &mut prefs).is_empty());
    assert_eq!(os.file_viewer().as_deref(), Some(OLD_BUNDLE_ID));
}

// ------------------------------------------------------------------ command (palette, menu bar)

/// The command asks first when off (Cancel records nothing), and gives the
/// role back right away when on.
#[test]
fn toggle_asks_to_switch_on_and_switches_off_directly() {
    let os = Arc::new(bundled());
    let mut v = DefaultViewer::new(os.clone(), None);
    assert!(v.can_toggle() && !v.status.is_default);
    v.toggle();
    assert!(v.prompt_open && v.prompt_manual, "asks, like at launch");
    assert!(!v.can_toggle(), "not twice while asking");
    v.answer(PromptChoice::NotNow);
    assert!(!v.prompt_open && !v.prompt_manual);
    assert_eq!(
        (v.prefs.prompt_state, v.prefs.not_now_count),
        (PromptState::default(), 0),
        "Cancel isn't a launch answer"
    );
    assert_eq!(os.file_viewer().as_deref(), Some(FORKLIFT));

    v.toggle();
    v.answer(PromptChoice::SetDefault);
    assert_eq!(v.wait(), Some(Ok(())));
    assert!(v.status.is_default);
    assert_eq!(os.file_viewer().as_deref(), Some(ME));

    v.toggle();
    assert!(!v.prompt_open, "switching off doesn't ask");
    assert_eq!(v.wait(), Some(Ok(())));
    assert!(!v.status.is_default);
    assert_eq!(
        os.file_viewer().as_deref(),
        Some(FORKLIFT),
        "back to ForkLift"
    );
}

#[test]
fn toggle_does_nothing_when_unavailable() {
    let mut v = DefaultViewer::new(Arc::new(FakeOs::new("/Volumes/Manta/Manta.app")), None);
    assert!(!v.can_toggle());
    v.toggle();
    assert!(!v.prompt_open);
}
