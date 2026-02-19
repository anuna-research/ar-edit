use chrono::Utc;
use thiserror::Error;

use crate::models::{EditDocument, EditOp, EditOpKind, EditSnapshot, Shot, ShotRange};

#[derive(Debug, Error)]
pub enum EditError {
    #[error("shot not found: {0}")]
    ShotNotFound(String),
    #[error("position out of bounds: {position} (max: {max})")]
    PositionOutOfBounds { position: usize, max: usize },
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
    /// and adds the shot to the snapshot.
    pub fn add_shot(&mut self, source: impl Into<String>, range: ShotRange) -> &Shot {
        let id = format!("shot-{:03}", self.next_shot_id);
        self.next_shot_id += 1;

        let shot = Shot {
            id,
            source: source.into(),
            range,
            notes: vec![],
        };

        self.push_op(EditOpKind::AddShot { shot: shot.clone() });
        self.snapshot.shots.push(shot);
        self.snapshot.shots.last().unwrap()
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
    pub fn trim_shot(&mut self, shot_id: &str, new_range: ShotRange) -> Result<(), EditError> {
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

    // -- add_shot -------------------------------------------------------------

    #[test]
    fn add_shot_appends_op_and_snapshot() {
        let mut doc = EditDocument::create("test");
        let shot = doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 });

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
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 });
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 });
        doc.add_shot("src-001", ShotRange::Time { from_ms: 5000, to_ms: 10000 });

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
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 });
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 });
        doc.add_shot("src-003", ShotRange::Words { from: 100, to: 200 });

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
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 10 });
        doc.add_shot("src-002", ShotRange::Words { from: 11, to: 20 });

        doc.move_shot("shot-001", 0).unwrap();

        assert_eq!(doc.snapshot.shots[0].id, "shot-001");
        assert_eq!(doc.snapshot.shots[1].id, "shot-002");
    }

    #[test]
    fn move_shot_not_found() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 });

        let err = doc.move_shot("shot-999", 0).unwrap_err();
        assert!(matches!(err, EditError::ShotNotFound(id) if id == "shot-999"));
    }

    #[test]
    fn move_shot_position_out_of_bounds() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 });
        doc.add_shot("src-002", ShotRange::Words { from: 53, to: 100 });

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
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 });
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 });

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
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 100 });

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

    // -- op log truncation (undo scenario) ------------------------------------

    #[test]
    fn new_edit_truncates_undone_ops() {
        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 });
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 });
        doc.add_shot("src-003", ShotRange::Words { from: 100, to: 200 });

        assert_eq!(doc.ops.len(), 3);
        assert_eq!(doc.head, 2);

        // simulate undo: move head back and fix snapshot to match
        doc.head = 0;
        doc.snapshot.shots.truncate(1);

        // new edit should truncate ops beyond head
        doc.add_shot("src-004", ShotRange::Time { from_ms: 0, to_ms: 5000 });

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
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 10 });
        doc.add_shot("src-002", ShotRange::Words { from: 11, to: 20 });
        doc.move_shot("shot-002", 0).unwrap();
        doc.trim_shot("shot-001", ShotRange::Words { from: 2, to: 8 }).unwrap();
        doc.remove_shot("shot-002").unwrap();

        let ids: Vec<u32> = doc.ops.iter().map(|op| op.id).collect();
        assert_eq!(ids, vec![0, 1, 2, 3, 4]);
    }

    // -- integration: spec-like flow ------------------------------------------

    #[test]
    fn spec_example_flow() {
        let mut doc = EditDocument::create("rough-cut");

        // Add three shots
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 });
        doc.add_shot("src-003", ShotRange::Words { from: 200, to: 280 });
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 });

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
}
