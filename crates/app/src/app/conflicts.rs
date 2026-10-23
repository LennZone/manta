//! Conflict questions of transfers (`ConflictPolicy::Ask`): opens the
//! question dialog for the oldest job waiting for a decision whenever no
//! other dialog is open, and hands the answer to the transfer manager.
//! Several waiting jobs are asked one after the other.

use manta_ops::{ConflictChoice, ConflictQuestion, JobId};

use super::MantaApp;
use crate::conflict_dialog::ConflictForm;
use crate::dialogs::Dialog;

/// The last answer given, until the job has moved on (the next snapshot
/// may still show the answered question for a moment).
#[derive(Default)]
pub(crate) struct ConflictState {
    pub(crate) answered: Option<(JobId, ConflictQuestion)>,
}

impl MantaApp {
    /// Once per frame, after the job snapshot.
    pub(super) fn conflict_frame(&mut self) {
        // Forget the answered question once the job shows something else.
        if let Some((id, q)) = &self.conflicts.answered {
            let still = self
                .jobs
                .iter()
                .any(|j| j.id == *id && j.state.question() == Some(q));
            if !still {
                self.conflicts.answered = None;
            }
        }
        // The open question was settled elsewhere (stopped in the panel).
        if let Some(Dialog::Conflict(f)) = &self.dialog {
            let open = self
                .jobs
                .iter()
                .any(|j| j.id == f.job && j.state.question() == Some(&f.question));
            if !open {
                self.dialog = None;
            }
        }
        if self.dialog.is_some() {
            return;
        }
        let answered = self.conflicts.answered.as_ref();
        let next = self
            .jobs
            .iter()
            .filter(|j| j.state.question().is_some())
            .filter(|j| answered.is_none_or(|(id, q)| *id != j.id || j.state.question() != Some(q)))
            .min_by_key(|j| j.id);
        if let Some(j) = next {
            let question = j.state.question().cloned().expect("filtered");
            self.dialog = Some(Dialog::Conflict(Box::new(ConflictForm {
                job: j.id,
                kind: j.kind,
                title: j.title.clone(),
                question,
                all: false,
            })));
        }
    }

    /// The user answered the open question.
    pub(super) fn conflict_answered(&mut self, form: ConflictForm, choice: ConflictChoice) {
        log::info!(
            "transfer job {}: conflict at {} answered {choice:?} (all: {})",
            form.job,
            form.question.dst.path,
            form.all
        );
        self.transfers.resolve_conflict(form.job, choice, form.all);
        self.conflicts.answered = Some((form.job, form.question));
    }
}
