//! Finder features on the app side: tags (reading the visible folders'
//! tags, the Tags submenu, the sidebar's tags), searching across folders
//! (scope bar, results as a virtual location) and the Get Info windows.
//! The pieces themselves are in `crate::tags`, `crate::search` and
//! `crate::info`.

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use manta_core::{SharedVfs, VfsPath};
use manta_plugin::FileActionKind;

use super::{live_folders, start_listing, MantaApp, Side};
use crate::info::{InfoRequest, Target};
use crate::pane::Location;
use crate::search::{Query, Scope, Search};
use crate::tags::{self, KnownTags, TagEdit};
use crate::worker::WorkResult;

impl MantaApp {
    // ------------------------------------------------------------ tags

    /// Once per frame: read the tags of the visible local folders in the
    /// background and hand them to their tabs; the menus get the known tags.
    pub(super) fn tags_frame(&mut self, ctx: &egui::Context) {
        let mut wanted: Vec<PathBuf> = Vec::new();
        for pane in &mut self.panes {
            let view = pane.view;
            let tab = pane.tab_mut();
            if tab.in_search() || !tags::taggable(&tab.loc) {
                continue;
            }
            let dirs = live_folders(tab, view);
            // A new listing (or a reload) reads the folder's tags again.
            if tab.tags_listing != tab.listing_gen {
                tab.tags_listing = tab.listing_gen;
                if let Some(d) = tab.loc.fs.local_path(&tab.loc.path) {
                    self.tags.invalidate(&d);
                }
            }
            wanted.extend(dirs);
        }
        wanted.sort();
        wanted.dedup();
        self.tags.frame(ctx, &wanted);
        for pane in &mut self.panes {
            let tab = pane.tab_mut();
            if tab.in_search() {
                continue;
            }
            let now = tags::taggable(&tab.loc)
                .then(|| tab.loc.fs.local_path(&tab.loc.path))
                .flatten()
                .and_then(|d| self.tags.get(&d));
            let changed = match (&tab.tags, &now) {
                (Some(a), Some(b)) => !std::sync::Arc::ptr_eq(a, b),
                (None, None) => false,
                _ => true,
            };
            if changed {
                tab.tags = now;
                if tab.filter.to_lowercase().contains("tag:") {
                    tab.rebuild_view();
                }
            }
        }
        if self.tags.version() != self.tags_known_version {
            self.tags_known_version = self.tags.version();
            tags::set_known(ctx, KnownTags::of(&self.tags));
        }
    }

    /// Context menu → Tags: add or remove a tag on the selection.
    pub(super) fn set_tag(&mut self, side: Side, edit: TagEdit) {
        let tab = self.pane(side).tab();
        let fs = tab.loc.fs.clone();
        let items: Vec<(String, PathBuf)> = tab
            .selected_entries()
            .into_iter()
            .filter_map(|e| fs.local_path(&e.path).map(|p| (e.name, p)))
            .collect();
        if items.is_empty() {
            return;
        }
        let mut errors = Vec::new();
        let mut done: Vec<(String, Vec<tags::Tag>)> = Vec::new();
        let mut dirs: Vec<PathBuf> = Vec::new();
        for (name, path) in &items {
            match tags::set_tag(path, &edit.tag, edit.on) {
                Ok(t) => done.push((name.clone(), t)),
                Err(e) => errors.push(format!(
                    "{}: {}",
                    crate::util::display_name(name),
                    crate::i18n::localize_error(&e.to_string())
                )),
            }
            if let Some(d) = path.parent() {
                if !dirs.iter().any(|x| x == d) {
                    dirs.push(d.to_path_buf());
                }
            }
        }
        self.tags.note(&edit.tag);
        for d in &dirs {
            self.tags.invalidate(d);
        }
        // Search results keep their own tags: update them right away.
        let tab = self.pane_mut(side).tab_mut();
        if let Some(s) = tab.search.as_mut() {
            let map = std::sync::Arc::make_mut(&mut s.tags);
            for (name, t) in done {
                if t.is_empty() {
                    map.remove(&name);
                } else {
                    map.insert(name, t);
                }
            }
            tab.tags = Some(s.tags.clone());
            tab.rebuild_view();
        }
        if !errors.is_empty() {
            self.set_error_detail(t!("tags.failed"), errors.join("\n"));
        }
    }

    /// Sidebar → Tags: everything with the tag, found by Spotlight, in the
    /// active pane.
    pub(super) fn show_tag(&mut self, name: String) {
        let side = self.active;
        let local = self.local.clone();
        let ctx = self.worker.ctx().clone();
        let spotlight = self.spotlight.clone();
        let tab = self.pane_mut(side).tab_mut();
        // The results are files on this Mac: leave a server first.
        if !tags::taggable(&tab.loc) {
            let home = local.home().unwrap_or_else(|_| VfsPath::root());
            tab.back.push(tab.loc.clone());
            tab.forward.clear();
            tab.loc = Location::new(local, None, home);
        }
        tab.filter = if name.contains(char::is_whitespace) {
            format!("tag:\"{name}\"")
        } else {
            format!("tag:{name}")
        };
        tab.scope = Scope::Folder;
        tab.search_due = None;
        let spec = crate::search::spec_for(Query::Tag(name), Scope::Mac, &tab.loc, tab.show_hidden);
        if let Some(spec) = spec {
            tab.begin_search(Search::start(spec, &ctx, spotlight));
        }
    }

    // ------------------------------------------------------------ search

    /// Once per frame: start searches whose typing pause is over, take the
    /// results that arrived.
    pub(super) fn search_frame(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let spotlight = self.spotlight.clone();
        for pane in &mut self.panes {
            for tab in &mut pane.tabs {
                if let Some(due) = tab.search_due {
                    if due <= now {
                        tab.search_due = None;
                        let spec = crate::search::spec_for(
                            Query::Text(tab.filter.clone()),
                            tab.scope,
                            &tab.loc,
                            tab.show_hidden,
                        );
                        if let Some(spec) = spec {
                            log::debug!("search: {:?} in {:?}", tab.scope, tab.loc.path);
                            tab.begin_search(Search::start(spec, ctx, spotlight.clone()));
                        }
                    } else {
                        ctx.request_repaint_after(due - now);
                    }
                }
                if let Some(s) = tab.search.as_mut() {
                    let (hits, _) = s.poll();
                    tab.add_results(hits);
                }
            }
        }
    }

    /// The scope bar: search this folder (filter), below it, or the Mac.
    pub(super) fn set_search_scope(&mut self, side: Side, scope: Scope) {
        let worker = &self.worker;
        let pi = self.pos(side);
        let tab = self.panes[pi].tab_mut();
        tab.scope = scope;
        if scope == Scope::Folder {
            if tab.in_search() {
                tab.end_search();
                let p = tab.loc.path.clone();
                start_listing(worker, tab, Some(p));
            } else {
                tab.rebuild_view();
            }
            return;
        }
        if tab.filter.trim().is_empty() {
            // Nothing to look for yet: type it.
            tab.filter_focus = true;
        } else {
            tab.search_due = Some(Instant::now());
        }
    }

    /// Leave the search results (the query goes too) and list the folder
    /// again.
    pub(super) fn end_search(&mut self, side: Side) {
        let worker = &self.worker;
        let pi = self.pos(side);
        let tab = self.panes[pi].tab_mut();
        if !tab.in_search() {
            return;
        }
        tab.end_search();
        tab.filter.clear();
        let p = tab.loc.path.clone();
        start_listing(worker, tab, Some(p));
    }

    /// Run the same search again (⌘R on search results).
    pub(super) fn restart_search(&mut self, side: Side) {
        let ctx = self.worker.ctx().clone();
        let spotlight = self.spotlight.clone();
        let tab = self.pane_mut(side).tab_mut();
        let Some(s) = &tab.search else {
            return;
        };
        let spec = crate::search::spec_for(s.query.clone(), s.scope, &tab.loc, tab.show_hidden);
        if let Some(spec) = spec {
            tab.begin_search(Search::start(spec, &ctx, spotlight));
        }
    }

    /// Search results: show the selected item in its folder, selected.
    pub(super) fn reveal_result(&mut self, side: Side) {
        let tab = self.pane(side).tab();
        let Some(e) = tab.selected_entries().into_iter().next() else {
            self.end_search(side);
            return;
        };
        let Some(parent) = e.path.parent() else {
            return;
        };
        let name = e.path.file_name().to_string();
        let loc = tab.loc.with_path(parent);
        self.activate(side);
        self.navigate(side, loc, false);
        self.pane_mut(side).tab_mut().pending_select = Some(name);
    }

    /// After a file operation in `dir`: search results whose item is gone
    /// (moved, renamed, trashed) leave the list.
    pub(super) fn prune_search_results(&mut self, fs: &SharedVfs) {
        for pane in &mut self.panes {
            for tab in &mut pane.tabs {
                if !tab.in_search() || !crate::util::same_fs(&tab.loc.fs, fs) {
                    continue;
                }
                let before = tab.entries.len();
                let tfs = tab.loc.fs.clone();
                tab.entries.retain(|e| match tfs.local_path(&e.path) {
                    Some(p) => std::fs::symlink_metadata(p).is_ok(),
                    None => true,
                });
                if tab.entries.len() != before {
                    tab.listing_gen += 1;
                    tab.rebuild_view();
                }
            }
        }
    }

    // ------------------------------------------------------------ Get Info

    /// ⌘I: the Info window of the selection (one window for several items).
    pub(super) fn get_info(&mut self, side: Side) {
        let tab = self.pane(side).tab();
        let items = tab.selected_entries();
        if items.is_empty() {
            self.set_info(t!("info.select"));
            return;
        }
        let target = Target {
            fs: tab.loc.fs.clone(),
            items,
            place: self.place_label(&tab.loc),
        };
        let ctx = self.worker.ctx().clone();
        self.info.open(&ctx, target);
    }

    /// Draw the Info windows and do what they ask.
    pub(super) fn info_frame(&mut self, ctx: &egui::Context) {
        if self.info.windows.is_empty() {
            return;
        }
        for req in self.info.ui(ctx) {
            match req {
                InfoRequest::Rename { fs, from, name } => self.rename_from_info(fs, from, name),
                InfoRequest::Changed { fs, dir } => {
                    if let Some(d) = fs.local_path(&dir) {
                        self.tags.invalidate(&d);
                    }
                    self.refresh_matching(&fs, &dir);
                }
                InfoRequest::OpenWith { app, file } => {
                    self.launch(vec![
                        "-a".into(),
                        app.into_os_string(),
                        file.into_os_string(),
                    ]);
                }
            }
        }
    }

    /// Rename an item from its Info window (like Rename… in the pane).
    pub(in crate::app) fn rename_from_info(&mut self, fs: SharedVfs, from: VfsPath, name: String) {
        let Some(dir) = from.parent() else {
            return;
        };
        let old = from.file_name().to_string();
        let target = dir.join(&name);
        let side = self.active;
        let mut a = self.new_action(side, FileActionKind::Rename, &fs, vec![from.clone()], None);
        a.destination = Some(target.clone());
        let action = Some(self.start_action(a));
        let refresh = vec![(fs.clone(), dir)];
        let undo = Some(crate::undo::Op::Rename {
            fs: fs.clone(),
            from: target.clone(),
            to: from.clone(),
        });
        self.worker.spawn(
            "rename",
            move || {
                // Never replace another item (a local rename would).
                if fs.stat(&target).is_ok() {
                    return Err(format!("already exists: {target}"));
                }
                fs.rename(&from, &target).map_err(|e| e.to_string())
            },
            move |result| WorkResult::FileOp {
                description: t!("status.renamed", old = old, name = name),
                failure: t!("error.rename", name = old),
                refresh,
                select: None,
                action,
                result,
                undo,
            },
        );
    }
}
