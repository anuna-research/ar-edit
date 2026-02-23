use schemars::schema_for;

use crate::models::EditDocument;

/// Return the JSON Schema for [`EditDocument`] as a pretty-printed string.
pub fn edit_document_schema() -> String {
    let schema = schema_for!(EditDocument);
    serde_json::to_string_pretty(&schema).expect("schema serialisation cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_valid_json() {
        let schema_str = edit_document_schema();
        let value: serde_json::Value = serde_json::from_str(&schema_str).unwrap();
        assert_eq!(value["$schema"], "http://json-schema.org/draft-07/schema#");
        assert_eq!(value["title"], "EditDocument");
    }

    /// Validate the DATA-MODEL.md edit document example against the generated schema.
    #[test]
    fn schema_validates_spec_example() {
        let schema_str = edit_document_schema();
        let schema_value: serde_json::Value = serde_json::from_str(&schema_str).unwrap();
        let compiled = jsonschema::draft7::new(&schema_value).expect("schema compiles");

        let example = serde_json::json!({
            "name": "rough-cut",
            "created": "2026-02-19T13:00:00Z",
            "next_shot_id": 6,
            "head": 4,
            "ops": [
                {
                    "id": 0,
                    "ts": "2026-02-19T13:00:01Z",
                    "op": "add_shot",
                    "shot": {
                        "id": "shot-001",
                        "source": "src-001",
                        "range": { "words": { "from": 0, "to": 52 } }
                    }
                },
                {
                    "id": 1,
                    "ts": "2026-02-19T13:00:15Z",
                    "op": "add_shot",
                    "shot": {
                        "id": "shot-002",
                        "source": "src-003",
                        "range": { "words": { "from": 200, "to": 280 } }
                    }
                },
                {
                    "id": 2,
                    "ts": "2026-02-19T13:01:02Z",
                    "op": "add_shot",
                    "shot": {
                        "id": "shot-003",
                        "source": "src-002",
                        "range": { "scenes": { "from": 0, "to": 2 } }
                    }
                },
                {
                    "id": 3,
                    "ts": "2026-02-19T13:02:30Z",
                    "op": "move_shot",
                    "shot_id": "shot-003",
                    "from_position": 2,
                    "to_position": 1
                },
                {
                    "id": 4,
                    "ts": "2026-02-19T13:03:45Z",
                    "op": "trim_shot",
                    "shot_id": "shot-002",
                    "old_range": { "words": { "from": 200, "to": 280 } },
                    "new_range": { "words": { "from": 210, "to": 265 } }
                }
            ],
            "snapshot": {
                "shots": [
                    {
                        "id": "shot-001",
                        "source": "src-001",
                        "range": { "words": { "from": 0, "to": 52 } }
                    },
                    {
                        "id": "shot-003",
                        "source": "src-002",
                        "range": { "scenes": { "from": 0, "to": 2 } }
                    },
                    {
                        "id": "shot-002",
                        "source": "src-003",
                        "range": { "words": { "from": 210, "to": 265 } }
                    }
                ]
            }
        });

        let result = compiled.validate(&example);
        assert!(
            result.is_ok(),
            "spec example failed validation: {:?}",
            result.err()
        );
    }

    /// Validate a minimal edit document (empty ops, head -1).
    #[test]
    fn schema_validates_minimal_document() {
        let schema_str = edit_document_schema();
        let schema_value: serde_json::Value = serde_json::from_str(&schema_str).unwrap();
        let compiled = jsonschema::draft7::new(&schema_value).expect("schema compiles");

        let minimal = serde_json::json!({
            "name": "empty",
            "created": "2026-01-01T00:00:00Z",
            "next_shot_id": 1,
            "head": -1,
            "ops": [],
            "snapshot": { "shots": [] }
        });

        let result = compiled.validate(&minimal);
        assert!(
            result.is_ok(),
            "minimal doc failed validation: {:?}",
            result.err()
        );
    }

    /// Validate that an edit document with notes passes schema validation.
    #[test]
    fn schema_validates_document_with_notes() {
        let schema_str = edit_document_schema();
        let schema_value: serde_json::Value = serde_json::from_str(&schema_str).unwrap();
        let compiled = jsonschema::draft7::new(&schema_value).expect("schema compiles");

        let doc = serde_json::json!({
            "name": "annotated",
            "created": "2026-02-19T13:00:00Z",
            "next_shot_id": 2,
            "head": 1,
            "ops": [
                {
                    "id": 0,
                    "ts": "2026-02-19T13:00:01Z",
                    "op": "add_shot",
                    "shot": {
                        "id": "shot-001",
                        "source": "src-001",
                        "range": { "words": { "from": 0, "to": 52 } }
                    }
                },
                {
                    "id": 1,
                    "ts": "2026-02-19T15:00:00Z",
                    "op": "add_note",
                    "shot_id": "shot-001",
                    "note": {
                        "text": "Too long, trim the first half",
                        "created": "2026-02-19T15:00:00Z"
                    }
                }
            ],
            "snapshot": {
                "shots": [
                    {
                        "id": "shot-001",
                        "source": "src-001",
                        "range": { "words": { "from": 0, "to": 52 } },
                        "notes": [
                            {
                                "text": "Too long, trim the first half",
                                "created": "2026-02-19T15:00:00Z"
                            }
                        ]
                    }
                ]
            }
        });

        let result = compiled.validate(&doc);
        assert!(
            result.is_ok(),
            "annotated doc failed validation: {:?}",
            result.err()
        );
    }

    /// Validate a document with all range types (words, scenes, time).
    #[test]
    fn schema_validates_all_range_types() {
        let schema_str = edit_document_schema();
        let schema_value: serde_json::Value = serde_json::from_str(&schema_str).unwrap();
        let compiled = jsonschema::draft7::new(&schema_value).expect("schema compiles");

        let doc = serde_json::json!({
            "name": "mixed-ranges",
            "created": "2026-02-19T13:00:00Z",
            "next_shot_id": 4,
            "head": 2,
            "ops": [
                {
                    "id": 0, "ts": "2026-02-19T13:00:01Z",
                    "op": "add_shot",
                    "shot": { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } }
                },
                {
                    "id": 1, "ts": "2026-02-19T13:00:02Z",
                    "op": "add_shot",
                    "shot": { "id": "shot-002", "source": "src-002", "range": { "scenes": { "from": 0, "to": 2 } } }
                },
                {
                    "id": 2, "ts": "2026-02-19T13:00:03Z",
                    "op": "add_shot",
                    "shot": { "id": "shot-003", "source": "src-002", "range": { "time": { "from_ms": 15000, "to_ms": 22000 } } }
                }
            ],
            "snapshot": {
                "shots": [
                    { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } },
                    { "id": "shot-002", "source": "src-002", "range": { "scenes": { "from": 0, "to": 2 } } },
                    { "id": "shot-003", "source": "src-002", "range": { "time": { "from_ms": 15000, "to_ms": 22000 } } }
                ]
            }
        });

        let result = compiled.validate(&doc);
        assert!(
            result.is_ok(),
            "mixed-ranges doc failed validation: {:?}",
            result.err()
        );
    }

    /// Validate that all operation types are accepted by the schema.
    #[test]
    fn schema_validates_all_op_types() {
        let schema_str = edit_document_schema();
        let schema_value: serde_json::Value = serde_json::from_str(&schema_str).unwrap();
        let compiled = jsonschema::draft7::new(&schema_value).expect("schema compiles");

        let doc = serde_json::json!({
            "name": "all-ops",
            "created": "2026-02-19T13:00:00Z",
            "next_shot_id": 3,
            "head": 5,
            "ops": [
                {
                    "id": 0, "ts": "2026-02-19T13:00:01Z",
                    "op": "add_shot",
                    "shot": { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } }
                },
                {
                    "id": 1, "ts": "2026-02-19T13:00:02Z",
                    "op": "remove_shot",
                    "shot_id": "shot-001",
                    "shot": { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } }
                },
                {
                    "id": 2, "ts": "2026-02-19T13:00:03Z",
                    "op": "add_shot",
                    "shot": { "id": "shot-002", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } }
                },
                {
                    "id": 3, "ts": "2026-02-19T13:00:04Z",
                    "op": "move_shot",
                    "shot_id": "shot-002",
                    "from_position": 0,
                    "to_position": 1
                },
                {
                    "id": 4, "ts": "2026-02-19T13:00:05Z",
                    "op": "trim_shot",
                    "shot_id": "shot-002",
                    "old_range": { "words": { "from": 0, "to": 52 } },
                    "new_range": { "words": { "from": 10, "to": 40 } }
                },
                {
                    "id": 5, "ts": "2026-02-19T13:00:06Z",
                    "op": "replace_range_type",
                    "shot_id": "shot-002",
                    "old_range": { "words": { "from": 10, "to": 40 } },
                    "new_range": { "time": { "from_ms": 5000, "to_ms": 12000 } }
                }
            ],
            "snapshot": {
                "shots": [
                    { "id": "shot-002", "source": "src-001", "range": { "time": { "from_ms": 5000, "to_ms": 12000 } } }
                ]
            }
        });

        let result = compiled.validate(&doc);
        assert!(
            result.is_ok(),
            "all-ops doc failed validation: {:?}",
            result.err()
        );
    }
}
