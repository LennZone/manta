//! Very large folders: a folder shown fresh appears part by part while it
//! is listed; what is derived from the selection follows every change.
//! (Timings: `bench_listing_tests`, docs/performance.md.)
//!
//! Child of `ux_tests`.

use std::io::{Read, Write};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

use manta_core::{Protocol, SharedVfs, Vfs, VfsResult};
use manta_ops::memfs::MemFs;

use super::*;
use crate::app::Location;

/// A server whose listing of `/big` comes in two parts; the second only
/// once the test says so.
struct TwoParts {
    mem: MemFs,
    release: Mutex<Receiver<()>>,
}

impl Vfs for TwoParts {
    fn label(&self) -> String {
        "two-parts@example.com (SFTP)".into()
    }
    fn protocol(&self) -> Protocol {
        Protocol::Sftp
    }
    fn home(&self) -> VfsResult<VfsPath> {
        Ok(VfsPath::new("/"))
    }
    fn list(&self, dir: &VfsPath) -> VfsResult<Vec<Entry>> {
        self.mem.list(dir)
    }
    fn list_parts(&self, dir: &VfsPath, part: &mut dyn FnMut(Vec<Entry>)) -> VfsResult<()> {
        let mut all = self.mem.list(dir)?;
        if dir.as_str() != "/big" {
            part(all);
            return Ok(());
        }
        let rest = all.split_off(all.len() / 2);
        part(all);
        let _ = self.release.lock().unwrap().recv();
        part(rest);
        Ok(())
    }
    fn stat(&self, path: &VfsPath) -> VfsResult<Entry> {
        self.mem.stat(path)
    }
    fn create_dir(&self, path: &VfsPath) -> VfsResult<()> {
        self.mem.create_dir(path)
    }
    fn remove_file(&self, path: &VfsPath) -> VfsResult<()> {
        self.mem.remove_file(path)
    }
    fn remove_dir(&self, path: &VfsPath) -> VfsResult<()> {
        self.mem.remove_dir(path)
    }
    fn rename(&self, from: &VfsPath, to: &VfsPath) -> VfsResult<()> {
        self.mem.rename(from, to)
    }
    fn open_read(&self, path: &VfsPath) -> VfsResult<Box<dyn Read + Send>> {
        self.mem.open_read(path)
    }
    fn open_write(&self, path: &VfsPath) -> VfsResult<Box<dyn Write + Send>> {
        self.mem.open_write(path)
    }
}

#[test]
fn a_large_folder_shows_its_first_part_while_it_is_listed() {
    let fx = fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    let mem = MemFs::new();
    mem.mkdir_p("/big");
    let now = chrono::Utc::now();
    for i in 0..12_000 {
        mem.put(&format!("/big/file {i}.txt"), b"x", now);
    }
    let (tx, rx) = channel();
    let fs: SharedVfs = Arc::new(TwoParts {
        mem,
        release: Mutex::new(rx),
    });
    app(&mut h).navigate(
        Side::Left,
        Location {
            fs,
            conn: None,
            path: VfsPath::new("/big"),
            archive: None,
        },
        false,
    );
    // The first half is shown (sorted) while the listing goes on.
    let start = Instant::now();
    while app(&mut h).pane(Side::Left).tab().entries.len() != 6_000 {
        assert!(start.elapsed() < Duration::from_secs(30), "no first part");
        h.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    h.run_steps(2);
    let tab = app(&mut h).pane(Side::Left).tab();
    assert!(tab.loading, "still listing");
    assert_eq!(tab.view.len(), 6_000);
    let names: Vec<&str> = (0..tab.view.len())
        .map(|vi| tab.entry_at(vi).unwrap().name.as_str())
        .collect();
    let mut sorted = names.clone();
    sorted.sort_by(|a, b| crate::util::natural_cmp(a, b));
    assert_eq!(names, sorted, "in order");
    // Something is selected meanwhile; it stays when the rest arrives.
    let pick = names[100].to_string();
    {
        let tab = app(&mut h).pane_mut(Side::Left).tab_mut();
        tab.select_only(100);
    }
    tx.send(()).unwrap();
    settle(&mut h);
    let tab = app(&mut h).pane(Side::Left).tab();
    assert!(!tab.loading);
    assert_eq!(tab.entries.len(), 12_000);
    assert_eq!(tab.view.len(), 12_000);
    assert_eq!(tab.cursor_entry().map(|e| e.name.as_str()), Some(&*pick));
    assert_eq!(tab.selected.iter().collect::<Vec<_>>(), vec![&pick]);
}

/// The selection summary (status bar size, menus, plugins) is cached, and
/// follows every kind of change: selection, filter, sort, new listing.
#[test]
fn selection_summary_follows_every_change() {
    let fx = fixture();
    let mut h = harness_with(&fx, WIDE, Theme::Light, false);
    let size_of = |a: &MantaApp, names: &[&str]| -> u64 {
        let t = a.pane(Side::Left).tab();
        t.entries
            .iter()
            .filter(|e| names.contains(&e.name.as_str()) && !e.is_dir())
            .map(|e| e.size)
            .sum()
    };
    select(&mut h, Side::Left, &["report-q3.pdf", "notes.txt"]);
    let a = app(&mut h);
    let want = size_of(a, &["report-q3.pdf", "notes.txt"]);
    let tab = a.pane_mut(Side::Left).tab_mut();
    assert_eq!(tab.selection_size(), want);
    assert_eq!(tab.selection_summary().view_idx.len(), 2);
    // Adding one.
    tab.selected.insert("data.json".into());
    let want = size_of(app(&mut h), &["report-q3.pdf", "notes.txt", "data.json"]);
    let tab = app(&mut h).pane_mut(Side::Left).tab_mut();
    assert_eq!(tab.selection_size(), want);
    // A filter hides two of them: they leave the selection.
    tab.filter = "data".into();
    tab.rebuild_view();
    assert_eq!(
        tab.selected_entries()
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        vec!["data.json"]
    );
    tab.filter.clear();
    tab.rebuild_view();
    // Select all, then sort the other way: same set, new view order.
    tab.select_all();
    let all = tab.selection_summary();
    assert_eq!(all.view_idx.len(), tab.view.len());
    let first = tab.selected_entries()[0].name.clone();
    tab.sort = crate::util::SortSpec {
        column: crate::util::SortColumn::Name,
        ascending: false,
    };
    tab.rebuild_view();
    assert_ne!(tab.selected_entries()[0].name, first, "view order");
    assert!(tab.selection_summary().any_archive, "backup-….zip");
    tab.select_only(0);
    assert!(!tab.selection_summary().any_archive, "a folder");
    // A new file appears (reload): the summary sees it.
    std::fs::write(fx.root.join("new.zip"), "zip").unwrap();
    let ctx = h.ctx.clone();
    app(&mut h).handle_action(&ctx, crate::app::Action::Refresh(Side::Left));
    settle(&mut h);
    let tab = app(&mut h).pane_mut(Side::Left).tab_mut();
    tab.select_all();
    assert!(tab.selection_summary().any_archive);
    assert!(tab.selection_summary().view_idx.len() > all.view_idx.len());
}
