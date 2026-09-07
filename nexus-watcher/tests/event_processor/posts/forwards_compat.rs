use nexus_common::models::post::PostInput;
use pubky_app_specs::{traits::TimestampId, PubkyAppPost, PubkyAppPostKind};

#[test]
fn nexus_preserves_future_kinds_before_specs_deserialization() {
    let blob = br#"{"content":"future-post-content","kind":"hyperverse_post"}"#;
    let specs_post: PubkyAppPost = serde_json::from_slice(blob).unwrap();
    assert_eq!(specs_post.kind, PubkyAppPostKind::Unknown);

    // Run before the specs enum loses the original string. Other resource
    // types still use the specs importer.
    let post = PostInput::from_bytes(blob, &specs_post.create_id()).unwrap();
    assert_eq!(post.kind.as_str(), "hyperverse_post");
    assert_eq!(post.content, "future-post-content");
}
