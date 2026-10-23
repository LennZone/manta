//! The queue of file-action lines waiting to be printed in a shell (pure).
//!
//! Lines are printed through the shell (see [`crate::shim::ECHO_KEY`]) only
//! while it sits idle at its prompt, so they scroll with the history and
//! never reach a running program. Until then they wait here, in order, at
//! most [`MAX_LINES`] of them.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use manta_plugin::{FileAction, FileActionPhase};

use crate::actions::{self, ShellCtx};

/// A `Started` line waits this long, so that a quick action (rename,
/// new folder, a small copy) prints its command and result together.
pub const GRACE: Duration = Duration::from_millis(150);
/// At most this many lines are kept while the shell is busy; older ones
/// are dropped (and a note says how many).
pub const MAX_LINES: usize = 50;

#[derive(Debug, Clone)]
pub struct EchoItem {
    /// Position among everything typed into / printed in this shell (cd
    /// follows share the counter), so the order is kept.
    pub seq: u64,
    pub ready_at: Instant,
    pub action: FileAction,
}

#[derive(Debug, Default)]
pub struct EchoQueue {
    items: VecDeque<EchoItem>,
    /// Items dropped since the last print.
    dropped: usize,
    /// The action printed last and the shell's prompt count at that time
    /// (a result right below its command needs no label).
    last: Option<(u64, u64)>,
}

impl EchoQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, seq: u64, action: FileAction, now: Instant) {
        let ready_at = if action.is_finished() {
            now
        } else {
            now + GRACE
        };
        self.items.push_back(EchoItem {
            seq,
            ready_at,
            action,
        });
        while self.items.len() > MAX_LINES {
            self.items.pop_front();
            self.dropped += 1;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Distinct actions waiting (a start and its result count once).
    pub fn actions_waiting(&self) -> usize {
        let mut ids: Vec<u64> = self.items.iter().map(|i| i.action.id).collect();
        ids.sort_unstable();
        ids.dedup();
        ids.len()
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// Sequence number of the oldest waiting item.
    pub fn front_seq(&self) -> Option<u64> {
        self.items.front().map(|i| i.seq)
    }

    fn is_ready(&self, item: &EchoItem, now: Instant) -> bool {
        item.ready_at <= now
            || self
                .items
                .iter()
                .any(|o| o.action.id == item.action.id && o.action.is_finished())
    }

    /// When the oldest item becomes ready (for a wake-up), if it is not yet.
    pub fn next_ready_at(&self, now: Instant) -> Option<Instant> {
        let first = self.items.front()?;
        (!self.is_ready(first, now)).then_some(first.ready_at)
    }

    /// Take the items that can be printed now: from the front, in order,
    /// while they are ready and older than `before` (a pending cd).
    pub fn take_ready(&mut self, before: Option<u64>, now: Instant) -> Vec<EchoItem> {
        let mut n = 0;
        for item in &self.items {
            if before.is_some_and(|b| item.seq >= b) || !self.is_ready(item, now) {
                break;
            }
            n += 1;
        }
        self.items.drain(..n).collect()
    }

    /// Everything still waiting (e.g. the shell turned out to have no
    /// integration).
    pub fn drain_all(&mut self) -> Vec<FileAction> {
        self.dropped = 0;
        self.items.drain(..).map(|i| i.action).collect()
    }

    /// The text to print for `items` (one line each, `\n`-terminated).
    /// `prompt_count` changes whenever the shell drew a new prompt; a result
    /// directly below its own command (same prompt) is printed without a
    /// label.
    pub fn render(&mut self, items: &[EchoItem], ctx: ShellCtx, prompt_count: u64) -> String {
        let mut out = String::new();
        if self.dropped > 0 {
            let n = std::mem::take(&mut self.dropped);
            out.push_str(&actions::styled_note(&crate::i18n::tr!(
                "term.dropped",
                count = n
            )));
            out.push('\n');
            self.last = None;
        }
        for item in items {
            let a = &item.action;
            match &a.phase {
                FileActionPhase::Started => {
                    out.push_str(&actions::styled_command(&actions::command(a, ctx)));
                }
                FileActionPhase::Finished { .. } => {
                    let adjacent = self.last == Some((a.id, prompt_count));
                    let label = (!adjacent).then(|| actions::short_label(a));
                    let Some((outcome, text)) = actions::result_text(a, label.as_deref()) else {
                        continue;
                    };
                    out.push_str(&actions::styled_result(outcome, &text));
                }
            }
            out.push('\n');
            self.last = Some((a.id, prompt_count));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::escape::ShellKind;
    use crate::follow::Dir;
    use manta_core::VfsPath;
    use manta_plugin::{FileActionKind, PaneId};

    fn started(id: u64) -> FileAction {
        let mut a = FileAction::new(
            id,
            PaneId::Left,
            FileActionKind::Delete,
            vec![VfsPath::new(&format!("/w/f{id}"))],
        );
        a.destination = None;
        a
    }

    fn finished(id: u64) -> FileAction {
        let mut a = started(id);
        a.phase = FileActionPhase::Finished {
            ok: true,
            files: 1,
            bytes: 0,
            errors: vec![],
            cancelled: false,
            elapsed_ms: 5,
        };
        a
    }

    fn plain(s: &str) -> String {
        // Strip SGR sequences.
        let mut out = String::new();
        let mut it = s.chars().peekable();
        while let Some(c) = it.next() {
            if c == '\x1b' {
                for d in it.by_ref() {
                    if d == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn grace_lets_quick_actions_print_together() {
        let t0 = Instant::now();
        let mut q = EchoQueue::new();
        q.push(1, started(1), t0);
        assert!(
            q.take_ready(None, t0).is_empty(),
            "started waits for the grace period"
        );
        assert_eq!(q.next_ready_at(t0), Some(t0 + GRACE));
        q.push(2, finished(1), t0 + Duration::from_millis(20));
        let items = q.take_ready(None, t0 + Duration::from_millis(20));
        assert_eq!(items.len(), 2, "the result makes the command ready");
        let d = Dir::same("/w");
        let text = plain(&q.render(&items, ShellCtx::new(Some(&d), ShellKind::Zsh), 7));
        assert_eq!(text, "→ rm -r 'f1'\n  ✓ 1 item · 5 ms\n");
        assert!(q.is_empty());
    }

    #[test]
    fn slow_actions_print_command_first_then_result() {
        let t0 = Instant::now();
        let mut q = EchoQueue::new();
        let d = Dir::same("/w");
        let ctx = ShellCtx::new(Some(&d), ShellKind::Zsh);
        q.push(1, started(1), t0);
        let items = q.take_ready(None, t0 + GRACE);
        assert_eq!(plain(&q.render(&items, ctx, 3)), "→ rm -r 'f1'\n");
        // Same prompt: no label.
        q.push(2, finished(1), t0 + Duration::from_secs(1));
        let items = q.take_ready(None, t0 + Duration::from_secs(1));
        assert_eq!(plain(&q.render(&items, ctx, 3)), "  ✓ 1 item · 5 ms\n");
        // The user ran a command in between: the result is labelled.
        q.push(3, started(2), t0);
        let items = q.take_ready(None, t0 + GRACE);
        q.render(&items, ctx, 4);
        q.push(4, finished(2), t0);
        let items = q.take_ready(None, t0);
        assert_eq!(
            plain(&q.render(&items, ctx, 5)),
            "  ✓ delete “f2” · 1 item · 5 ms\n"
        );
    }

    #[test]
    fn order_with_pending_cd_is_kept() {
        let t0 = Instant::now();
        let mut q = EchoQueue::new();
        q.push(1, finished(1), t0);
        q.push(3, finished(2), t0);
        // A cd with seq 2 is pending: only the item before it may go.
        let items = q.take_ready(Some(2), t0);
        assert_eq!(items.iter().map(|i| i.seq).collect::<Vec<_>>(), vec![1]);
        assert_eq!(q.front_seq(), Some(3));
        assert_eq!(q.take_ready(None, t0).len(), 1);
    }

    #[test]
    fn a_waiting_item_blocks_later_ones() {
        let t0 = Instant::now();
        let mut q = EchoQueue::new();
        q.push(1, started(1), t0);
        q.push(2, finished(9), t0);
        assert!(q.take_ready(None, t0).is_empty());
        assert_eq!(q.take_ready(None, t0 + GRACE).len(), 2);
    }

    #[test]
    fn caps_lines_and_notes_the_drop() {
        let t0 = Instant::now();
        let mut q = EchoQueue::new();
        for i in 0..(MAX_LINES as u64 + 7) {
            q.push(i, finished(i), t0);
        }
        assert_eq!(q.len(), MAX_LINES);
        assert_eq!(q.dropped(), 7);
        let items = q.take_ready(None, t0);
        let d = Dir::same("/w");
        let text = plain(&q.render(&items, ShellCtx::new(Some(&d), ShellKind::Zsh), 0));
        assert!(
            text.starts_with("  … 7 earlier file action lines not shown\n"),
            "{text}"
        );
        assert_eq!(text.lines().count(), MAX_LINES + 1);
        assert_eq!(q.dropped(), 0);
    }

    #[test]
    fn drain_all_returns_actions() {
        let t0 = Instant::now();
        let mut q = EchoQueue::new();
        q.push(1, started(1), t0);
        q.push(2, finished(1), t0);
        assert_eq!(q.actions_waiting(), 1);
        assert_eq!(q.drain_all().len(), 2);
        assert!(q.is_empty());
    }
}
