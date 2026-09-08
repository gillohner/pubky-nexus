//! Public API contract for custom kinds and embed-only resources.
#[allow(dead_code)]
mod utils;

use anyhow::Result;
use axum::http::StatusCode;
use nexus_common::{
    db::{exec_single_row, queries},
    models::{
        post::{PostDetails, PostInput, PostRelationships},
        traits::Collection,
        user::UserDetails,
    },
    universal_tag::normalize::{normalize_uri, resource_id},
};
use pubky_app_specs::{traits::TimestampId, PubkyAppPost, PubkyId};
use serde_json::json;

#[tokio::test]
async fn existing_endpoints_return_and_filter_custom_posts() -> Result<()> {
    utils::host_url().await;
    let author: PubkyId = pubky::Keypair::random().into();
    let user = UserDetails::from_pubky(author.clone());
    user.put_to_graph().await?;
    UserDetails::put_to_index(&[author.as_ref()], vec![Some(user)]).await?;
    let uri = format!("https://example.com/{author}#map");
    let resource_id = resource_id(&normalize_uri(&uri).unwrap().0);
    let mut ids = Vec::new();
    for kind in ["event", "Event", "short"] {
        let id = PubkyAppPost::default().create_id();
        let blob = serde_json::to_vec(&json!({"kind":kind,"content":"picnic","embed":uri}))?;
        let input = PostInput::from_bytes(&blob, &id).map_err(anyhow::Error::msg)?;
        let relationships = PostRelationships::from_homeserver(&input);
        let details = PostDetails::from_homeserver(input, &author, &id);
        details.put_to_graph(&relationships).await?;
        details.put_to_index(&author, None, false).await?;
        ids.push(id);
    }

    let path = format!("/v0/post/{author}/{}", ids[0]);
    let details = utils::get_request(&format!("{path}/details")).await?;
    assert_eq!(details["kind"], "event");
    assert_eq!(details["content"], "picnic");
    assert_eq!(details["embed"], uri);
    assert_eq!(utils::get_request(&path).await?["details"], details);

    let root = format!("/v0/stream/posts?source=author&author_id={author}");
    let all = utils::get_request(&root).await?;
    assert_eq!(all.as_array().unwrap().len(), 3);
    let filtered = utils::get_request(&format!("{root}&kind=event&limit=1")).await?;
    assert_eq!(filtered[0]["details"], details);
    let keys = utils::get_request(&format!(
        "/v0/stream/posts/keys?source=author&author_id={author}&exclude_kinds=Event,short&limit=1"
    ))
    .await?;
    assert_eq!(keys["post_keys"], json!([format!("{author}:{}", ids[0])]));
    let batch = utils::post_request(
        "/v0/stream/posts/by_ids",
        json!({"post_ids":[format!("{author}:{}", ids[0])]}),
    )
    .await?;
    assert_eq!(batch[0]["details"], details);
    let search = utils::get_request(&format!(
        "/v0/search/posts/by_content?q=picnic&author={author}&kind=event"
    ))
    .await?;
    assert_eq!(search.as_array().unwrap().len(), 1);

    let tags = utils::get_request(&format!("/v0/resource/{resource_id}/tags")).await?;
    assert_eq!(tags["resource"]["id"], resource_id);
    assert_eq!(tags["tags"], json!([]));
    let lookup = url::Url::parse_with_params(
        "http://localhost/v0/resource/by-uri",
        [("uri", uri.as_str())],
    )?;
    let by_uri =
        utils::get_request(&format!("/v0/resource/by-uri?{}", lookup.query().unwrap())).await?;
    assert_eq!(by_uri, tags);
    let resources = utils::get_request("/v0/stream/resources/ids?app=pubky.app&limit=100").await?;
    assert!(resources["resource_ids"]
        .as_array()
        .unwrap()
        .contains(&json!(resource_id)));

    for suffix in [
        "kind=event&exclude_kinds=short",
        "kind=bad%20kind",
        "exclude_kinds=bad%20kind",
    ] {
        utils::invalid_get_request(&format!("{root}&{suffix}"), StatusCode::BAD_REQUEST).await?;
    }
    for source in ["collection", "post_replies", "author_replies"] {
        utils::invalid_get_request(
            &format!(
                "/v0/stream/posts?source={source}&author_id={author}&post_id={}&kind=event",
                ids[0]
            ),
            StatusCode::BAD_REQUEST,
        )
        .await?;
    }
    for id in ids {
        exec_single_row(queries::del::delete_post(&author, &id)).await?;
    }
    // Global ranking fixtures share this graph. Leaving the random author behind
    // changes the top-100 cutoff for tied scores in timeframe queries.
    UserDetails::delete(author.as_ref()).await?;
    Ok(())
}
