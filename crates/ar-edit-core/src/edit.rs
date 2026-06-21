use std::fs;
use std::path::Path;

use chrono::Utc;
use thiserror::Error;

use crate::models::{EditDocument, EditOp, EditOpKind, EditSnapshot, Shot, ShotNote, ShotRange};

#[derive(Debug, Error)]
pub enum EditError {
    #[error("shot not found: {0}")]
    ShotNotFound(String),
    #[error("position out of bounds: {position} (max: {max})")]
    PositionOutOfBounds { position: usize, max: usize },
    #[error("invalid range: {0}")]
    InvalidRange(String),
    #[error("nothing to undo")]
    NothingToUndo,
    #[error("nothing to redo")]
    NothingToRedo,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("failed to parse edit document: {0}")]
    Json(#[from] serde_json::Error),
}

/// Validate that a range has correct ordering (from <= to) and non-zero duration.
pub fn validate_range(range: &ShotRange) -> Result<(), EditError> {
    match range {
        ShotRange::Words { from, to } => {
            if from > to {
                return Err(EditError::InvalidRange(format!(
                    "from ({from}) must be <= to ({to})"
                )));
            }
            if from == to {
                return Err(EditError::InvalidRange("range has zero duration".into()));
            }
        }
        ShotRange::Scenes { from, to } => {
            if from > to {
                return Err(EditError::InvalidRange(format!(
                    "from ({from}) must be <= to ({to})"
                )));
            }
            if from == to {
                return Err(EditError::InvalidRange("range has zero duration".into()));
            }
        }
        ShotRange::Time { from_ms, to_ms } => {
            if from_ms > to_ms {
                return Err(EditError::InvalidRange(format!(
                    "from ({from_ms}ms) must be <= to ({to_ms}ms)"
                )));
            }
            if from_ms == to_ms {
                return Err(EditError::InvalidRange("range has zero duration".into()));
            }
        }
    }
    Ok(())
}

impl EditDocument {
    /// Create a new empty edit document with no ops and head at -1.
    pub fn create(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            created: Utc::now(),
            next_shot_id: 1,
            head: -1,
            ops: vec![],
            snapshot: EditSnapshot { shots: vec![] },
        }
    }

    /// Append a new shot to the end of the timeline.
    ///
    /// Generates a new shot ID from `next_shot_id`, appends an `AddShot` op,
    /// and adds the shot to the snapshot. Returns an error if the range is
    /// inverted or has zero duration.
    pub fn add_shot(
        &mut self,
        source: impl Into<String>,
        range: ShotRange,
    ) -> Result<&Shot, EditError> {
        validate_range(&range)?;

        let id = format!("shot-{:03}", self.next_shot_id);
        self.next_shot_id += 1;

        let shot = Shot {
            id,
            source: source.into(),
            range,
            notes: vec![],
            author: String::new(),
        };

        self.push_op(EditOpKind::AddShot { shot: shot.clone() });
        self.snapshot.shots.push(shot);
        Ok(self.snapshot.shots.last().unwrap())
    }

    /// Add a shot using a caller-supplied `id` rather than one derived from
    /// `next_shot_id`. Used when a live collaboration daemon has already minted
    /// the canonical actor-scoped id (SPEC-003 REQ-090): the CLI must persist and
    /// report that id so later move/trim/note/remove target the live shot.
    pub fn add_shot_with_id(
        &mut self,
        id: impl Into<String>,
        source: impl Into<String>,
        range: ShotRange,
    ) -> Result<&Shot, EditError> {
        validate_range(&range)?;

        let shot = Shot {
            id: id.into(),
            source: source.into(),
            range,
            notes: vec![],
            author: String::new(),
        };

        self.push_op(EditOpKind::AddShot { shot: shot.clone() });
        self.snapshot.shots.push(shot);
        Ok(self.snapshot.shots.last().unwrap())
    }

    /// Move a shot from its current position to `to_position` in the timeline.
    ///
    /// `to_position` is the desired index in the resulting shot list.
    pub fn move_shot(&mut self, shot_id: &str, to_position: usize) -> Result<(), EditError> {
        let from = self.find_shot_position(shot_id)?;
        let max = self.snapshot.shots.len() - 1;
        if to_position > max {
            return Err(EditError::PositionOutOfBounds {
                position: to_position,
                max,
            });
        }

        self.push_op(EditOpKind::MoveShot {
            shot_id: shot_id.into(),
            from_position: from as u32,
            to_position: to_position as u32,
        });

        let shot = self.snapshot.shots.remove(from);
        self.snapshot.shots.insert(to_position, shot);
        Ok(())
    }

    /// Remove a shot by ID. The full shot data is preserved in the op for undo.
    pub fn remove_shot(&mut self, shot_id: &str) -> Result<Shot, EditError> {
        let idx = self.find_shot_position(shot_id)?;
        let shot = self.snapshot.shots[idx].clone();

        self.push_op(EditOpKind::RemoveShot {
            shot_id: shot_id.into(),
            shot: shot.clone(),
        });

        self.snapshot.shots.remove(idx);
        Ok(shot)
    }

    /// Trim a shot's range, storing both old and new range in the op.
    ///
    /// Returns an error if the new range is inverted or has zero duration.
    pub fn trim_shot(&mut self, shot_id: &str, new_range: ShotRange) -> Result<(), EditError> {
        validate_range(&new_range)?;
        let idx = self.find_shot_position(shot_id)?;
        let old_range = self.snapshot.shots[idx].range.clone();

        self.push_op(EditOpKind::TrimShot {
            shot_id: shot_id.into(),
            old_range,
            new_range: new_range.clone(),
        });

        self.snapshot.shots[idx].range = new_range;
        Ok(())
    }

    /// Append a note to a shot's notes array.
    ///
    /// Notes are append-only and never deleted.
    pub fn add_note(
        &mut self,
        shot_id: &str,
        text: impl Into<String>,
    ) -> Result<&ShotNote, EditError> {
        let idx = self.find_shot_position(shot_id)?;

        let note = ShotNote {
            text: text.into(),
            author: String::new(),
            created: Utc::now(),
        };

        self.push_op(EditOpKind::AddNote {
            shot_id: shot_id.into(),
            note: note.clone(),
        });

        self.snapshot.shots[idx].notes.push(note);
        Ok(self.snapshot.shots[idx].notes.last().unwrap())
    }

    // -- undo / redo ----------------------------------------------------------

    /// Undo the last operation: decrement head and recompute the snapshot.
    ///
    /// Returns the undone op for reporting. Errors if head is already at -1.
    pub fn undo(&mut self) -> Result<&EditOp, EditError> {
        if self.head < 0 {
            return Err(EditError::NothingToUndo);
        }
        self.head -= 1;
        self.snapshot = Self::recompute_snapshot(&self.ops, self.head);
        Ok(&self.ops[(self.head + 1) as usize])
    }

    /// Redo the next operation: increment head and recompute the snapshot.
    ///
    /// Returns the redone op for reporting. Errors if head is already at the end.
    pub fn redo(&mut self) -> Result<&EditOp, EditError> {
        let max = self.ops.len() as i32 - 1;
        if self.head >= max {
            return Err(EditError::NothingToRedo);
        }
        self.head += 1;
        self.snapshot = Self::recompute_snapshot(&self.ops, self.head);
        Ok(&self.ops[self.head as usize])
    }

    // -- snapshot recomputation -----------------------------------------------

    /// Replay `ops[0..=head]` from an empty state to produce an `EditSnapshot`.
    ///
    /// This is a pure function: given the same ops and head, it always produces
    /// the same snapshot. If head is negative, returns an empty snapshot.
    pub fn recompute_snapshot(ops: &[EditOp], head: i32) -> EditSnapshot {
        let mut snapshot = EditSnapshot { shots: vec![] };
        if head < 0 {
            return snapshot;
        }

        let end = (head as usize) + 1;
        for op in &ops[..end] {
            match &op.op {
                EditOpKind::AddShot { shot } => {
                    snapshot.shots.push(shot.clone());
                }
                EditOpKind::RemoveShot { shot_id, .. } => {
                    if let Some(idx) = snapshot.shots.iter().position(|s| s.id == *shot_id) {
                        snapshot.shots.remove(idx);
                    }
                }
                EditOpKind::MoveShot {
                    shot_id,
                    to_position,
                    ..
                } => {
                    if let Some(idx) = snapshot.shots.iter().position(|s| s.id == *shot_id) {
                        let shot = snapshot.shots.remove(idx);
                        snapshot.shots.insert(*to_position as usize, shot);
                    }
                }
                EditOpKind::TrimShot {
                    shot_id, new_range, ..
                } => {
                    if let Some(shot) = snapshot.shots.iter_mut().find(|s| s.id == *shot_id) {
                        shot.range = new_range.clone();
                    }
                }
                EditOpKind::ReplaceRangeType {
                    shot_id, new_range, ..
                } => {
                    if let Some(shot) = snapshot.shots.iter_mut().find(|s| s.id == *shot_id) {
                        shot.range = new_range.clone();
                    }
                }
                EditOpKind::AddNote { shot_id, note } => {
                    if let Some(shot) = snapshot.shots.iter_mut().find(|s| s.id == *shot_id) {
                        shot.notes.push(note.clone());
                    }
                }
            }
        }

        snapshot
    }

    // -- persistence ----------------------------------------------------------

    /// Save the edit document to disk as pretty-printed JSON.
    pub fn save(&self, path: &Path) -> Result<(), EditError> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Load an edit document from disk.
    pub fn load(path: &Path) -> Result<Self, EditError> {
        let content = fs::read_to_string(path)?;
        let doc: EditDocument = serde_json::from_str(&content)?;
        Ok(doc)
    }

    // -- helpers --------------------------------------------------------------

    /// Truncate any undone ops beyond head, append a new op, and advance head.
    fn push_op(&mut self, kind: EditOpKind) {
        let keep = if self.head < 0 {
            0
        } else {
            (self.head as usize) + 1
        };
        self.ops.truncate(keep);

        self.ops.push(EditOp {
            id: self.ops.len() as u32,
            ts: Utc::now(),
            op: kind,
        });
        self.head = (self.ops.len() - 1) as i32;
    }

    fn find_shot_position(&self, shot_id: &str) -> Result<usize, EditError> {
        self.snapshot
            .shots
            .iter()
            .position(|s| s.id == shot_id)
            .ok_or_else(|| EditError::ShotNotFound(shot_id.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ShotRange;

    // -- create ---------------------------------------------------------------

    #[test]
    fn create_empty_edit() {
        let doc = EditDocument::create("rough-cut");
        assert_eq!(doc.name, "rough-cut");
        assert_eq!(doc.head, -1);
        assert!(doc.ops.is_empty());
        assert!(doc.snapshot.shots.is_empty());
        assert_eq!(doc.next_shot_id, 1);
    }

    // -- validate_range -------------------------------------------------------

    #[test]
    fn validate_range_rejects_inverted_with_ordering_message() {
        for range in [
            ShotRange::Words { from: 10, to: 3 },
            ShotRange::Scenes { from: 5, to: 2 },
            ShotRange::Time {
                from_ms: 9000,
                to_ms: 1000,
            },
        ] {
            let err = validate_range(&range).unwrap_err().to_string();
            assert!(
                err.contains("must be <= to"),
                "inverted range message: {err}"
            );
        }
    }

    #[test]
    fn validate_range_rejects_zero_width_with_zero_duration_message() {
        // A zero-width range (from == to) is rejected as "zero duration", NOT as
        // an ordering error — distinguishes `from > to` from `from >= to`.
        for range in [
            ShotRange::Words { from: 7, to: 7 },
            ShotRange::Scenes { from: 0, to: 0 },
            ShotRange::Time {
                from_ms: 500,
                to_ms: 500,
            },
        ] {
            let err = validate_range(&range).unwrap_err().to_string();
            assert!(
                err.contains("zero duration"),
                "zero-width range message: {err}"
            );
        }
    }

    #[test]
    fn validate_range_accepts_proper_ranges() {
        for range in [
            ShotRange::Words { from: 0, to: 1 },
            ShotRange::Scenes { from: 0, to: 3 },
            ShotRange::Time {
                from_ms: 0,
                to_ms: 1,
            },
        ] {
            assert!(
                validate_range(&range).is_ok(),
                "proper range is valid: {range:?}"
            );
        }
    }

    // -- add_shot -------------------------------------------------------------

    #[test]
    fn add_shot_appends_op_and_snapshot() {
        let mut doc = EditDocument::create("test");
        let shot = doc
            .add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();

        assert_eq!(shot.id, "shot-001");
        assert_eq!(shot.source, "src-001");
        assert_eq!(shot.range, ShotRange::Words { from: 0, to: 52 });
        assert!(shot.notes.is_empty());

        assert_eq!(doc.ops.len(), 1);
        assert_eq!(doc.head, 0);
        assert_eq!(doc.ops[0].id, 0);
        assert!(matches!(
            &doc.ops[0].op,
            EditOpKind::AddShot { shot } if shot.id == "shot-001"
        ));
        assert_eq!(doc.snapshot.shots.len(), 1);
        assert_eq!(doc.next_shot_id, 2);
    }

    #[test]
    fn add_multiple_shots_increments_ids() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
            .unwrap();
        doc.add_shot(
            "src-001",
            ShotRange::Time {
                from_ms: 5000,
                to_ms: 10000,
            },
        )
        .unwrap();

        assert_eq!(doc.ops.len(), 3);
        assert_eq!(doc.head, 2);
        assert_eq!(doc.snapshot.shots.len(), 3);
        assert_eq!(doc.snapshot.shots[0].id, "shot-001");
        assert_eq!(doc.snapshot.shots[1].id, "shot-002");
        assert_eq!(doc.snapshot.shots[2].id, "shot-003");
        assert_eq!(doc.next_shot_id, 4);
    }

    // -- move_shot ------------------------------------------------------------

    #[test]
    fn move_shot_reorders_snapshot() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
            .unwrap();
        doc.add_shot("src-003", ShotRange::Words { from: 100, to: 200 })
            .unwrap();

        doc.move_shot("shot-003", 0).unwrap();

        assert_eq!(doc.snapshot.shots[0].id, "shot-003");
        assert_eq!(doc.snapshot.shots[1].id, "shot-001");
        assert_eq!(doc.snapshot.shots[2].id, "shot-002");

        assert_eq!(doc.ops.len(), 4);
        assert_eq!(doc.head, 3);
        match &doc.ops[3].op {
            EditOpKind::MoveShot {
                shot_id,
                from_position,
                to_position,
            } => {
                assert_eq!(shot_id, "shot-003");
                assert_eq!(*from_position, 2);
                assert_eq!(*to_position, 0);
            }
            other => panic!("expected MoveShot, got {other:?}"),
        }
    }

    #[test]
    fn move_shot_to_same_position_is_noop() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 10 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Words { from: 11, to: 20 })
            .unwrap();

        doc.move_shot("shot-001", 0).unwrap();

        assert_eq!(doc.snapshot.shots[0].id, "shot-001");
        assert_eq!(doc.snapshot.shots[1].id, "shot-002");
    }

    #[test]
    fn move_shot_not_found() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();

        let err = doc.move_shot("shot-999", 0).unwrap_err();
        assert!(matches!(err, EditError::ShotNotFound(id) if id == "shot-999"));
    }

    #[test]
    fn move_shot_position_out_of_bounds() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Words { from: 53, to: 100 })
            .unwrap();

        let err = doc.move_shot("shot-001", 5).unwrap_err();
        assert!(matches!(
            err,
            EditError::PositionOutOfBounds {
                position: 5,
                max: 1
            }
        ));
    }

    // -- remove_shot ----------------------------------------------------------

    #[test]
    fn remove_shot_preserves_data_in_op() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
            .unwrap();

        let removed = doc.remove_shot("shot-001").unwrap();
        assert_eq!(removed.id, "shot-001");
        assert_eq!(removed.source, "src-001");
        assert_eq!(removed.range, ShotRange::Words { from: 0, to: 52 });

        // snapshot has only shot-002
        assert_eq!(doc.snapshot.shots.len(), 1);
        assert_eq!(doc.snapshot.shots[0].id, "shot-002");

        // op preserves full shot data for undo
        match &doc.ops[2].op {
            EditOpKind::RemoveShot { shot_id, shot } => {
                assert_eq!(shot_id, "shot-001");
                assert_eq!(shot.source, "src-001");
                assert_eq!(shot.range, ShotRange::Words { from: 0, to: 52 });
            }
            other => panic!("expected RemoveShot, got {other:?}"),
        }
    }

    #[test]
    fn remove_shot_not_found() {
        let mut doc = EditDocument::create("test");

        let err = doc.remove_shot("shot-999").unwrap_err();
        assert!(matches!(err, EditError::ShotNotFound(_)));
    }

    // -- trim_shot ------------------------------------------------------------

    #[test]
    fn trim_shot_updates_range() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 100 })
            .unwrap();

        doc.trim_shot("shot-001", ShotRange::Words { from: 10, to: 90 })
            .unwrap();

        assert_eq!(
            doc.snapshot.shots[0].range,
            ShotRange::Words { from: 10, to: 90 }
        );

        match &doc.ops[1].op {
            EditOpKind::TrimShot {
                shot_id,
                old_range,
                new_range,
            } => {
                assert_eq!(shot_id, "shot-001");
                assert_eq!(*old_range, ShotRange::Words { from: 0, to: 100 });
                assert_eq!(*new_range, ShotRange::Words { from: 10, to: 90 });
            }
            other => panic!("expected TrimShot, got {other:?}"),
        }
    }

    #[test]
    fn trim_shot_not_found() {
        let mut doc = EditDocument::create("test");

        let err = doc
            .trim_shot("shot-999", ShotRange::Words { from: 0, to: 10 })
            .unwrap_err();
        assert!(matches!(err, EditError::ShotNotFound(_)));
    }

    // -- range validation (eager rejection) -----------------------------------

    #[test]
    fn add_shot_rejects_inverted_word_range() {
        let mut doc = EditDocument::create("test");
        let err = doc
            .add_shot("src-001", ShotRange::Words { from: 52, to: 10 })
            .unwrap_err();
        assert!(matches!(err, EditError::InvalidRange(_)));
        assert!(doc.snapshot.shots.is_empty());
    }

    #[test]
    fn add_shot_rejects_zero_duration_word_range() {
        let mut doc = EditDocument::create("test");
        let err = doc
            .add_shot("src-001", ShotRange::Words { from: 5, to: 5 })
            .unwrap_err();
        assert!(matches!(err, EditError::InvalidRange(_)));
        assert!(doc.snapshot.shots.is_empty());
    }

    #[test]
    fn add_shot_rejects_inverted_scene_range() {
        let mut doc = EditDocument::create("test");
        let err = doc
            .add_shot("src-001", ShotRange::Scenes { from: 3, to: 1 })
            .unwrap_err();
        assert!(matches!(err, EditError::InvalidRange(_)));
    }

    #[test]
    fn add_shot_rejects_zero_duration_scene_range() {
        let mut doc = EditDocument::create("test");
        let err = doc
            .add_shot("src-001", ShotRange::Scenes { from: 2, to: 2 })
            .unwrap_err();
        assert!(matches!(err, EditError::InvalidRange(_)));
    }

    #[test]
    fn add_shot_rejects_inverted_time_range() {
        let mut doc = EditDocument::create("test");
        let err = doc
            .add_shot(
                "src-001",
                ShotRange::Time {
                    from_ms: 22000,
                    to_ms: 15000,
                },
            )
            .unwrap_err();
        assert!(matches!(err, EditError::InvalidRange(_)));
    }

    #[test]
    fn add_shot_rejects_zero_duration_time_range() {
        let mut doc = EditDocument::create("test");
        let err = doc
            .add_shot(
                "src-001",
                ShotRange::Time {
                    from_ms: 5000,
                    to_ms: 5000,
                },
            )
            .unwrap_err();
        assert!(matches!(err, EditError::InvalidRange(_)));
    }

    #[test]
    fn add_shot_does_not_increment_id_on_invalid_range() {
        let mut doc = EditDocument::create("test");
        let _ = doc.add_shot("src-001", ShotRange::Words { from: 10, to: 5 });
        assert_eq!(doc.next_shot_id, 1); // Should not have incremented
        assert!(doc.ops.is_empty());

        // Valid add should still get shot-001
        let shot = doc
            .add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        assert_eq!(shot.id, "shot-001");
    }

    #[test]
    fn trim_shot_rejects_inverted_range() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 100 })
            .unwrap();

        let err = doc
            .trim_shot("shot-001", ShotRange::Words { from: 90, to: 10 })
            .unwrap_err();
        assert!(matches!(err, EditError::InvalidRange(_)));
        // Range should be unchanged
        assert_eq!(
            doc.snapshot.shots[0].range,
            ShotRange::Words { from: 0, to: 100 }
        );
    }

    #[test]
    fn trim_shot_rejects_zero_duration() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 100 })
            .unwrap();

        let err = doc
            .trim_shot("shot-001", ShotRange::Words { from: 50, to: 50 })
            .unwrap_err();
        assert!(matches!(err, EditError::InvalidRange(_)));
        assert_eq!(
            doc.snapshot.shots[0].range,
            ShotRange::Words { from: 0, to: 100 }
        );
    }

    // -- op log truncation (undo scenario) ------------------------------------

    #[test]
    fn new_edit_truncates_undone_ops() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
            .unwrap();
        doc.add_shot("src-003", ShotRange::Words { from: 100, to: 200 })
            .unwrap();

        assert_eq!(doc.ops.len(), 3);
        assert_eq!(doc.head, 2);

        // simulate undo: move head back and fix snapshot to match
        doc.head = 0;
        doc.snapshot.shots.truncate(1);

        // new edit should truncate ops beyond head
        doc.add_shot(
            "src-004",
            ShotRange::Time {
                from_ms: 0,
                to_ms: 5000,
            },
        )
        .unwrap();

        assert_eq!(doc.ops.len(), 2); // op[0] kept + new op
        assert_eq!(doc.ops[0].id, 0);
        assert_eq!(doc.ops[1].id, 1);
        assert_eq!(doc.head, 1);
        assert_eq!(doc.snapshot.shots.len(), 2);
        assert_eq!(doc.snapshot.shots[0].id, "shot-001");
        assert_eq!(doc.snapshot.shots[1].id, "shot-004");
    }

    // -- op id assignment -----------------------------------------------------

    #[test]
    fn op_ids_are_sequential() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 10 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Words { from: 11, to: 20 })
            .unwrap();
        doc.move_shot("shot-002", 0).unwrap();
        doc.trim_shot("shot-001", ShotRange::Words { from: 2, to: 8 })
            .unwrap();
        doc.remove_shot("shot-002").unwrap();

        let ids: Vec<u32> = doc.ops.iter().map(|op| op.id).collect();
        assert_eq!(ids, vec![0, 1, 2, 3, 4]);
    }

    // -- integration: spec-like flow ------------------------------------------

    #[test]
    fn spec_example_flow() {
        let mut doc = EditDocument::create("rough-cut");

        // Add three shots
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_shot("src-003", ShotRange::Words { from: 200, to: 280 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
            .unwrap();

        assert_eq!(doc.snapshot.shots.len(), 3);
        assert_eq!(doc.snapshot.shots[0].id, "shot-001");
        assert_eq!(doc.snapshot.shots[1].id, "shot-002");
        assert_eq!(doc.snapshot.shots[2].id, "shot-003");

        // Move shot-003 to position 1
        doc.move_shot("shot-003", 1).unwrap();
        assert_eq!(doc.snapshot.shots[0].id, "shot-001");
        assert_eq!(doc.snapshot.shots[1].id, "shot-003");
        assert_eq!(doc.snapshot.shots[2].id, "shot-002");

        // Trim shot-002
        doc.trim_shot("shot-002", ShotRange::Words { from: 210, to: 265 })
            .unwrap();
        assert_eq!(
            doc.snapshot.shots[2].range,
            ShotRange::Words { from: 210, to: 265 }
        );

        // Verify final state
        assert_eq!(doc.ops.len(), 5);
        assert_eq!(doc.head, 4);
        assert_eq!(doc.snapshot.shots.len(), 3);
    }

    // -- recompute_snapshot ---------------------------------------------------

    #[test]
    fn recompute_empty_ops() {
        let snapshot = EditDocument::recompute_snapshot(&[], -1);
        assert!(snapshot.shots.is_empty());
    }

    #[test]
    fn recompute_negative_head_returns_empty() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 10 })
            .unwrap();

        let snapshot = EditDocument::recompute_snapshot(&doc.ops, -1);
        assert!(snapshot.shots.is_empty());
    }

    #[test]
    fn recompute_matches_inline_snapshot() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
            .unwrap();
        doc.add_shot("src-003", ShotRange::Words { from: 100, to: 200 })
            .unwrap();
        doc.move_shot("shot-003", 0).unwrap();
        doc.trim_shot("shot-001", ShotRange::Words { from: 5, to: 45 })
            .unwrap();

        let recomputed = EditDocument::recompute_snapshot(&doc.ops, doc.head);
        assert_eq!(recomputed, doc.snapshot);
    }

    #[test]
    fn recompute_partial_head() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
            .unwrap();
        doc.add_shot("src-003", ShotRange::Words { from: 100, to: 200 })
            .unwrap();

        // Recompute at head=1 should only have first two shots
        let snapshot = EditDocument::recompute_snapshot(&doc.ops, 1);
        assert_eq!(snapshot.shots.len(), 2);
        assert_eq!(snapshot.shots[0].id, "shot-001");
        assert_eq!(snapshot.shots[1].id, "shot-002");
    }

    #[test]
    fn recompute_handles_add_shot() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();

        let snapshot = EditDocument::recompute_snapshot(&doc.ops, 0);
        assert_eq!(snapshot.shots.len(), 1);
        assert_eq!(snapshot.shots[0].id, "shot-001");
        assert_eq!(snapshot.shots[0].source, "src-001");
        assert_eq!(
            snapshot.shots[0].range,
            ShotRange::Words { from: 0, to: 52 }
        );
    }

    #[test]
    fn recompute_handles_remove_shot() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
            .unwrap();
        doc.remove_shot("shot-001").unwrap();

        let snapshot = EditDocument::recompute_snapshot(&doc.ops, doc.head);
        assert_eq!(snapshot.shots.len(), 1);
        assert_eq!(snapshot.shots[0].id, "shot-002");
    }

    #[test]
    fn recompute_handles_move_shot() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
            .unwrap();
        doc.add_shot("src-003", ShotRange::Words { from: 100, to: 200 })
            .unwrap();
        doc.move_shot("shot-003", 0).unwrap();

        let snapshot = EditDocument::recompute_snapshot(&doc.ops, doc.head);
        assert_eq!(snapshot.shots[0].id, "shot-003");
        assert_eq!(snapshot.shots[1].id, "shot-001");
        assert_eq!(snapshot.shots[2].id, "shot-002");
    }

    #[test]
    fn recompute_handles_trim_shot() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 100 })
            .unwrap();
        doc.trim_shot("shot-001", ShotRange::Words { from: 10, to: 90 })
            .unwrap();

        let snapshot = EditDocument::recompute_snapshot(&doc.ops, doc.head);
        assert_eq!(
            snapshot.shots[0].range,
            ShotRange::Words { from: 10, to: 90 }
        );
    }

    #[test]
    fn recompute_handles_replace_range_type() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();

        // Manually push a ReplaceRangeType op
        let new_range = ShotRange::Time {
            from_ms: 0,
            to_ms: 12400,
        };
        doc.push_op(EditOpKind::ReplaceRangeType {
            shot_id: "shot-001".into(),
            old_range: ShotRange::Words { from: 0, to: 52 },
            new_range: new_range.clone(),
        });
        doc.snapshot.shots[0].range = new_range.clone();

        let snapshot = EditDocument::recompute_snapshot(&doc.ops, doc.head);
        assert_eq!(
            snapshot.shots[0].range,
            ShotRange::Time {
                from_ms: 0,
                to_ms: 12400
            }
        );
    }

    #[test]
    fn recompute_all_op_types_combined() {
        let mut doc = EditDocument::create("test");
        // Add 3 shots
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
            .unwrap();
        doc.add_shot(
            "src-003",
            ShotRange::Time {
                from_ms: 5000,
                to_ms: 10000,
            },
        )
        .unwrap();
        // Move shot-003 to front
        doc.move_shot("shot-003", 0).unwrap();
        // Trim shot-001
        doc.trim_shot("shot-001", ShotRange::Words { from: 5, to: 40 })
            .unwrap();
        // Remove shot-002
        doc.remove_shot("shot-002").unwrap();

        let recomputed = EditDocument::recompute_snapshot(&doc.ops, doc.head);
        assert_eq!(recomputed, doc.snapshot);
        assert_eq!(recomputed.shots.len(), 2);
        assert_eq!(recomputed.shots[0].id, "shot-003");
        assert_eq!(recomputed.shots[1].id, "shot-001");
        assert_eq!(
            recomputed.shots[1].range,
            ShotRange::Words { from: 5, to: 40 }
        );
    }

    // -- undo / redo ----------------------------------------------------------

    #[test]
    fn undo_single_op() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();

        let undone = doc.undo().unwrap();
        assert!(matches!(&undone.op, EditOpKind::AddShot { .. }));
        assert_eq!(doc.head, -1);
        assert!(doc.snapshot.shots.is_empty());
        // ops remain in log for redo
        assert_eq!(doc.ops.len(), 1);
    }

    #[test]
    fn undo_multiple_ops() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
            .unwrap();
        doc.add_shot("src-003", ShotRange::Words { from: 100, to: 200 })
            .unwrap();

        doc.undo().unwrap();
        assert_eq!(doc.head, 1);
        assert_eq!(doc.snapshot.shots.len(), 2);
        assert_eq!(doc.snapshot.shots[0].id, "shot-001");
        assert_eq!(doc.snapshot.shots[1].id, "shot-002");

        doc.undo().unwrap();
        assert_eq!(doc.head, 0);
        assert_eq!(doc.snapshot.shots.len(), 1);
        assert_eq!(doc.snapshot.shots[0].id, "shot-001");

        doc.undo().unwrap();
        assert_eq!(doc.head, -1);
        assert!(doc.snapshot.shots.is_empty());
    }

    #[test]
    fn undo_nothing_to_undo() {
        let mut doc = EditDocument::create("test");
        let err = doc.undo().unwrap_err();
        assert!(matches!(err, EditError::NothingToUndo));
    }

    #[test]
    fn redo_single_op() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.undo().unwrap();

        let redone = doc.redo().unwrap();
        assert!(matches!(&redone.op, EditOpKind::AddShot { .. }));
        assert_eq!(doc.head, 0);
        assert_eq!(doc.snapshot.shots.len(), 1);
        assert_eq!(doc.snapshot.shots[0].id, "shot-001");
    }

    #[test]
    fn redo_nothing_to_redo() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();

        let err = doc.redo().unwrap_err();
        assert!(matches!(err, EditError::NothingToRedo));
    }

    #[test]
    fn redo_nothing_to_redo_empty() {
        let mut doc = EditDocument::create("test");
        let err = doc.redo().unwrap_err();
        assert!(matches!(err, EditError::NothingToRedo));
    }

    #[test]
    fn undo_redo_roundtrip() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
            .unwrap();

        let original_snapshot = doc.snapshot.clone();

        doc.undo().unwrap();
        assert_eq!(doc.snapshot.shots.len(), 1);

        doc.redo().unwrap();
        assert_eq!(doc.snapshot, original_snapshot);
    }

    #[test]
    fn undo_then_new_op_truncates_redo() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
            .unwrap();
        doc.add_shot("src-003", ShotRange::Words { from: 100, to: 200 })
            .unwrap();

        // Undo two ops
        doc.undo().unwrap();
        doc.undo().unwrap();
        assert_eq!(doc.head, 0);
        assert_eq!(doc.snapshot.shots.len(), 1);

        // New op should discard the undone ops
        doc.add_shot(
            "src-004",
            ShotRange::Time {
                from_ms: 0,
                to_ms: 5000,
            },
        )
        .unwrap();

        assert_eq!(doc.ops.len(), 2);
        assert_eq!(doc.head, 1);
        assert_eq!(doc.snapshot.shots.len(), 2);
        assert_eq!(doc.snapshot.shots[0].id, "shot-001");
        assert_eq!(doc.snapshot.shots[1].id, "shot-004");

        // Redo should fail since the redo history was discarded
        let err = doc.redo().unwrap_err();
        assert!(matches!(err, EditError::NothingToRedo));
    }

    #[test]
    fn undo_complex_operations() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
            .unwrap();
        doc.add_shot("src-003", ShotRange::Words { from: 100, to: 200 })
            .unwrap();
        doc.move_shot("shot-003", 0).unwrap();
        doc.trim_shot("shot-001", ShotRange::Words { from: 5, to: 45 })
            .unwrap();

        // Undo trim — shot-001 should go back to original range
        doc.undo().unwrap();
        assert_eq!(doc.snapshot.shots.len(), 3);
        let shot_001 = doc
            .snapshot
            .shots
            .iter()
            .find(|s| s.id == "shot-001")
            .unwrap();
        assert_eq!(shot_001.range, ShotRange::Words { from: 0, to: 52 });

        // Undo move — shot-003 should be back at the end
        doc.undo().unwrap();
        assert_eq!(doc.snapshot.shots[0].id, "shot-001");
        assert_eq!(doc.snapshot.shots[1].id, "shot-002");
        assert_eq!(doc.snapshot.shots[2].id, "shot-003");
    }

    #[test]
    fn undo_redo_all_ops() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
            .unwrap();
        doc.move_shot("shot-002", 0).unwrap();
        doc.trim_shot("shot-001", ShotRange::Words { from: 5, to: 45 })
            .unwrap();
        doc.remove_shot("shot-002").unwrap();

        let final_snapshot = doc.snapshot.clone();

        // Undo all
        for _ in 0..5 {
            doc.undo().unwrap();
        }
        assert_eq!(doc.head, -1);
        assert!(doc.snapshot.shots.is_empty());

        // Redo all
        for _ in 0..5 {
            doc.redo().unwrap();
        }
        assert_eq!(doc.head, 4);
        assert_eq!(doc.snapshot, final_snapshot);
    }

    // -- persistence ----------------------------------------------------------

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("test.edit.json");

        let mut doc = EditDocument::create("rough-cut");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
            .unwrap();
        doc.move_shot("shot-002", 0).unwrap();

        doc.save(&path).unwrap();
        let loaded = EditDocument::load(&path).unwrap();

        assert_eq!(loaded.name, doc.name);
        assert_eq!(loaded.head, doc.head);
        assert_eq!(loaded.ops.len(), doc.ops.len());
        assert_eq!(loaded.snapshot, doc.snapshot);
        assert_eq!(loaded.next_shot_id, doc.next_shot_id);
    }

    #[test]
    fn save_includes_cached_snapshot() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("test.edit.json");

        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.save(&path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();

        // Verify snapshot is present in the JSON
        assert!(json.get("snapshot").is_some());
        let shots = json["snapshot"]["shots"].as_array().unwrap();
        assert_eq!(shots.len(), 1);
        assert_eq!(shots[0]["id"], "shot-001");
    }

    #[test]
    fn load_and_recompute_matches() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("test.edit.json");

        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
            .unwrap();
        doc.move_shot("shot-002", 0).unwrap();
        doc.trim_shot("shot-001", ShotRange::Words { from: 5, to: 45 })
            .unwrap();

        doc.save(&path).unwrap();
        let loaded = EditDocument::load(&path).unwrap();

        // Recomputed snapshot should match the cached one
        let recomputed = EditDocument::recompute_snapshot(&loaded.ops, loaded.head);
        assert_eq!(recomputed, loaded.snapshot);
    }

    #[test]
    fn save_after_undo_preserves_full_ops() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("test.edit.json");

        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
            .unwrap();
        doc.undo().unwrap();

        doc.save(&path).unwrap();
        let loaded = EditDocument::load(&path).unwrap();

        assert_eq!(loaded.head, 0);
        assert_eq!(loaded.ops.len(), 2);
        assert_eq!(loaded.snapshot.shots.len(), 1);
    }

    #[test]
    fn load_nonexistent_file_errors() {
        let err = EditDocument::load(Path::new("/nonexistent/file.json")).unwrap_err();
        assert!(matches!(err, EditError::Io(_)));
    }

    // -- add_note -------------------------------------------------------------

    #[test]
    fn add_note_appends_to_shot() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();

        let note = doc
            .add_note("shot-001", "Too long, trim the first half")
            .unwrap();
        assert_eq!(note.text, "Too long, trim the first half");

        assert_eq!(doc.snapshot.shots[0].notes.len(), 1);
        assert_eq!(
            doc.snapshot.shots[0].notes[0].text,
            "Too long, trim the first half"
        );

        assert_eq!(doc.ops.len(), 2);
        match &doc.ops[1].op {
            EditOpKind::AddNote { shot_id, note } => {
                assert_eq!(shot_id, "shot-001");
                assert_eq!(note.text, "Too long, trim the first half");
            }
            other => panic!("expected AddNote, got {other:?}"),
        }
    }

    #[test]
    fn add_multiple_notes_to_same_shot() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();

        doc.add_note("shot-001", "First note").unwrap();
        doc.add_note("shot-001", "Second note").unwrap();

        assert_eq!(doc.snapshot.shots[0].notes.len(), 2);
        assert_eq!(doc.snapshot.shots[0].notes[0].text, "First note");
        assert_eq!(doc.snapshot.shots[0].notes[1].text, "Second note");
        assert_eq!(doc.ops.len(), 3);
    }

    #[test]
    fn add_note_shot_not_found() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();

        let err = doc.add_note("shot-999", "note text").unwrap_err();
        assert!(matches!(err, EditError::ShotNotFound(id) if id == "shot-999"));
    }

    #[test]
    fn add_note_undo_removes_note() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_note("shot-001", "A note").unwrap();

        assert_eq!(doc.snapshot.shots[0].notes.len(), 1);

        doc.undo().unwrap();
        assert!(doc.snapshot.shots[0].notes.is_empty());

        doc.redo().unwrap();
        assert_eq!(doc.snapshot.shots[0].notes.len(), 1);
        assert_eq!(doc.snapshot.shots[0].notes[0].text, "A note");
    }

    #[test]
    fn recompute_handles_add_note() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_note("shot-001", "First note").unwrap();
        doc.add_note("shot-001", "Second note").unwrap();

        let recomputed = EditDocument::recompute_snapshot(&doc.ops, doc.head);
        assert_eq!(recomputed, doc.snapshot);
        assert_eq!(recomputed.shots[0].notes.len(), 2);
    }

    #[test]
    fn save_load_roundtrip_with_notes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("test.edit.json");

        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
            .unwrap();
        doc.add_note("shot-001", "A note on this shot").unwrap();

        doc.save(&path).unwrap();
        let loaded = EditDocument::load(&path).unwrap();

        assert_eq!(loaded.snapshot.shots[0].notes.len(), 1);
        assert_eq!(
            loaded.snapshot.shots[0].notes[0].text,
            "A note on this shot"
        );

        let recomputed = EditDocument::recompute_snapshot(&loaded.ops, loaded.head);
        assert_eq!(recomputed, loaded.snapshot);
    }
}
