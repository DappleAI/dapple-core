use dapple_data::model::playbooks::Playbook;
use dapple_data::{ContentHash, DateTime};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

#[test]
fn datetime_serializes_as_rfc3339_string() {
    // build a minimal Playbook consumer object
    let pb = Playbook {
        id: uuid::Uuid::now_v7(),
        path_id: uuid::Uuid::now_v7(),
        name: "test".to_string(),
        slug: "test".to_string(),
        description: "desc".to_string(),
        content_hash: ContentHash::default(),
        deprecation_level: None,
        created_at: DateTime(OffsetDateTime::from_unix_timestamp(0).unwrap()),
        stages: vec![],
        categories: vec![],
    };

    let s = serde_json::to_string(&pb).expect("serialize");
    let v: Value = serde_json::from_str(&s).expect("parse json");

    // created_at should be a string and parse as RFC3339
    let v_created = &v["created_at"];
    assert!(
        v_created.is_string(),
        "created_at is not a string: {}",
        v_created
    );

    let sdt = v_created.as_str().unwrap();
    let _dt = OffsetDateTime::parse(sdt, &Rfc3339).expect("created_at not RFC3339");
}
