//! Contract tests requiring Neo4j and Redis Stack, but no homeserver/Postgres.
use anyhow::Result;
use nexus_common::{
    config::StackConfig,
    db::graph::Query,
    db::kv::SortOrder,
    db::{exec_single_row, fetch_row_from_graph, queries, RedisOps},
    models::{
        post::search::{create_post_content_index, PostsByContentSearch},
        post::{
            KindFilter, PostCounts, PostDetails, PostInput, PostRelationships, PostStream,
            StreamSource,
        },
        resource::{
            stream::{ResourceSorting, ResourceStream, ResourceStreamSource},
            ResourceDetails,
        },
        traits::Collection,
        user::{UserCounts, UserDetails, UserIngestor},
    },
    types::{Pagination, StreamSorting},
    universal_tag::normalize::{normalize_uri, resource_id},
    StackManager,
};
use nexus_watcher::events::handlers::{post, tag};
use pubky_app_specs::{traits::TimestampId, PubkyAppPost, PubkyAppTag, PubkyId};
use serde_json::json;

async fn put(
    author: &PubkyId,
    id: &str,
    kind: &str,
    content: &str,
    embed: Option<&str>,
) -> Result<()> {
    put_with_references(author, id, kind, content, None, embed).await
}

async fn put_with_references(
    author: &PubkyId,
    id: &str,
    kind: &str,
    content: &str,
    parent: Option<&str>,
    embed: Option<&str>,
) -> Result<()> {
    let bytes =
        serde_json::to_vec(&json!({"kind":kind,"content":content,"parent":parent,"embed":embed}))?;
    let input = PostInput::from_bytes(&bytes, id).map_err(anyhow::Error::msg)?;
    post::sync_put(input, author.clone(), id.into(), &UserIngestor::new([])).await?;
    Ok(())
}

async fn author() -> Result<PubkyId> {
    let id: PubkyId = pubky::Keypair::random().into();
    let user = UserDetails::from_pubky(id.clone());
    user.put_to_graph().await?;
    UserDetails::put_to_index(&[id.as_ref()], vec![Some(user)]).await?;
    UserCounts::default().put_to_index(&id).await?;
    Ok(id)
}

async fn keys(
    author: &PubkyId,
    filter: Option<KindFilter>,
    skip: usize,
    limit: usize,
) -> Result<Vec<String>> {
    Ok(PostStream::get_post_keys(
        StreamSource::Author {
            author_id: author.to_string(),
        },
        Pagination {
            skip: Some(skip),
            limit: Some(limit),
            ..Default::default()
        },
        SortOrder::Descending,
        StreamSorting::Timeline,
        None,
        filter,
    )
    .await?
    .map(|s| s.post_keys)
    .unwrap_or_default())
}

#[tokio::test]
async fn custom_posts_and_shared_embed_lifecycle() -> Result<()> {
    StackManager::setup(&StackConfig::default())
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    create_post_content_index().await?;
    let author = author().await?;
    let first = PubkyAppPost::default().create_id();
    let content = "  {\"title\":\"picnic\",\"lat\":12}  ";
    let uri = format!("https://Example.com/{author}#first");
    let normalized = normalize_uri(&uri).unwrap().0;
    let resource = resource_id(&normalized);
    put(&author, &first, "event", content, Some(&uri)).await?;

    let cached = PostDetails::get_by_id(&author, &first).await?.unwrap();
    assert_eq!(cached.kind.as_str(), "event");
    assert_eq!(cached.content, content);
    assert_eq!(cached.embed.as_deref(), Some(uri.as_str()));
    PostDetails::remove_from_index_multiple_json(&[&[&author, &first]]).await?;
    let rehydrated = PostDetails::get_by_id(&author, &first).await?.unwrap();
    assert_eq!(rehydrated.kind, cached.kind);
    assert_eq!(rehydrated.content, cached.content);
    assert_eq!(rehydrated.embed, cached.embed);
    assert_eq!(rehydrated.indexed_at, cached.indexed_at);
    let resource_details = ResourceDetails::get_by_id(&resource).await?.unwrap();
    assert_eq!(resource_details.uri, normalized);

    // Parent and embed can independently point to the same external Resource.
    // The parent makes this a reply; the embed makes it a shared/reposted item.
    let external_reply = PubkyAppPost::default().create_id();
    put_with_references(
        &author,
        &external_reply,
        "event",
        "reply and share",
        Some(&uri),
        Some(&uri),
    )
    .await?;
    let referenced = PostDetails::get_by_id(&author, &external_reply)
        .await?
        .unwrap();
    assert_eq!(referenced.parent.as_deref(), Some(uri.as_str()));
    assert_eq!(referenced.embed.as_deref(), Some(uri.as_str()));
    let relationship_types: Vec<String> = fetch_row_from_graph(
        Query::new(
            "external_post_references",
            "MATCH (:Post {id: $post_id})-[reference:REPLIED|EMBEDS]->(:Resource {id: $resource_id})
             RETURN collect(type(reference)) AS relationship_types",
        )
        .param("post_id", external_reply.clone())
        .param("resource_id", resource.clone()),
    )
    .await?
    .unwrap()
    .get("relationship_types")?;
    assert!(relationship_types.contains(&"REPLIED".to_string()));
    assert!(relationship_types.contains(&"EMBEDS".to_string()));
    assert!(!keys(&author, None, 0, 10)
        .await?
        .contains(&format!("{author}:{external_reply}")));
    post::del(author.clone(), external_reply, &UserIngestor::new([])).await?;
    assert!(ResourceDetails::get_by_id(&resource).await?.is_some());

    // Moving between a root post and an external reply updates feed membership
    // and removes the parent-only Resource after the final reference is gone.
    let movable = PubkyAppPost::default().create_id();
    let parent_only_uri = format!("geo:52.52,13.405?q={movable}");
    let parent_only_resource = resource_id(&normalize_uri(&parent_only_uri).unwrap().0);
    put(&author, &movable, "event", "moving", None).await?;
    assert!(keys(&author, None, 0, 10)
        .await?
        .contains(&format!("{author}:{movable}")));
    put_with_references(
        &author,
        &movable,
        "event",
        "moving",
        Some(&parent_only_uri),
        None,
    )
    .await?;
    assert!(!keys(&author, None, 0, 10)
        .await?
        .contains(&format!("{author}:{movable}")));
    assert!(ResourceDetails::get_by_id(&parent_only_resource)
        .await?
        .is_some());
    put(&author, &movable, "event", "moving", None).await?;
    assert!(keys(&author, None, 0, 10)
        .await?
        .contains(&format!("{author}:{movable}")));
    assert!(ResourceDetails::get_by_id(&parent_only_resource)
        .await?
        .is_none());
    post::del(author.clone(), movable, &UserIngestor::new([])).await?;

    // Replaying does not create a second Resource or move its timeline position.
    put(&author, &first, "event", content, Some(&uri)).await?;
    assert_eq!(
        ResourceDetails::get_by_id(&resource)
            .await?
            .unwrap()
            .indexed_at,
        resource_details.indexed_at
    );
    let resources = ResourceStream::get_resource_keys(
        &ResourceStreamSource::App {
            app: "pubky.app".into(),
        },
        Pagination {
            limit: Some(1000),
            ..Default::default()
        },
        SortOrder::Descending,
        &ResourceSorting::Timeline,
        None,
    )
    .await?;
    assert!(resources.resource_ids.contains(&resource));
    let zero_score = ResourceStream::get_resource_keys(
        &ResourceStreamSource::All,
        Pagination {
            start: Some(0.0),
            end: Some(0.0),
            limit: Some(1000),
            ..Default::default()
        },
        SortOrder::Descending,
        &ResourceSorting::TaggersCount,
        None,
    )
    .await?;
    assert!(zero_score.resource_ids.contains(&resource));

    // A tag with a different spelling shares the same node. Deleting the tag
    // must not delete the still-embedded resource.
    let tag_id = "00000000000000000000000000";
    let tag_uri = format!("pubky://{author}/pub/mapky/tags/{tag_id}");
    tag::sync_put_resource(
        PubkyAppTag {
            uri: format!("{normalized}#tag"),
            label: "picnic".into(),
            created_at: 1,
        },
        author.clone(),
        tag_id.into(),
        "mapky".into(),
        &UserIngestor::new([]),
    )
    .await?;
    tag::del(&tag_uri).await?;
    assert!(ResourceDetails::get_by_id(&resource).await?.is_some());

    // Exact kind strings survive graph/cache/search, including punctuation that
    // would be query syntax if it were not escaped.
    let upper = PubkyAppPost::default().create_id();
    put(&author, &upper, "Event", "picnic", None).await?;
    let punctuation = PubkyAppPost::default().create_id();
    let odd_kind = "event\"}|{short";
    put(&author, &punctuation, odd_kind, "picnic", None).await?;
    assert_eq!(
        PostDetails::get_from_graph(&author, &punctuation)
            .await?
            .unwrap()
            .0
            .kind
            .as_str(),
        odd_kind
    );
    assert_eq!(keys(&author, None, 0, 10).await?.len(), 3);
    assert_eq!(
        keys(
            &author,
            Some(KindFilter::Kind("event".parse().unwrap())),
            0,
            1
        )
        .await?,
        vec![format!("{author}:{first}")]
    );
    assert_eq!(
        keys(
            &author,
            Some(KindFilter::Exclude(vec![
                "Event".parse().unwrap(),
                odd_kind.parse().unwrap()
            ])),
            0,
            1
        )
        .await?,
        vec![format!("{author}:{first}")]
    );
    for (kind, id) in [
        ("event", &first),
        ("Event", &upper),
        (odd_kind, &punctuation),
    ] {
        let found =
            PostsByContentSearch::search("picnic", Some(&author), Some(kind), 0, 10).await?;
        assert_eq!(
            found
                .iter()
                .map(|p| p.post_key.as_str())
                .collect::<Vec<_>>(),
            vec![format!("{author}:{id}")]
        );
    }

    // A built-in -> custom edit clears both materialized and cached mentions.
    put(&author, &upper, "short", "picnic", None).await?;
    exec_single_row(queries::put::create_mention_relationship(
        &author, &upper, &author,
    ))
    .await?;
    PostRelationships::reindex(&author, &upper).await?;
    put(&author, &upper, "event", "picnic", None).await?;
    assert!(PostRelationships::get_by_id(&author, &upper)
        .await?
        .unwrap()
        .mentioned
        .is_empty());
    assert!(PostRelationships::get_from_graph(&author, &upper)
        .await?
        .unwrap()
        .mentioned
        .is_empty());

    // External -> repost -> external updates both edge types and parent counts.
    let parent_uri = format!("pubky://{author}/pub/pubky.app/posts/{upper}");
    put(&author, &first, "event", content, Some(&parent_uri)).await?;
    assert!(ResourceDetails::get_by_id(&resource).await?.is_none());
    assert_eq!(
        PostCounts::get_by_id(&author, &upper)
            .await?
            .unwrap()
            .reposts,
        1
    );
    put(&author, &first, "event", content, Some(&uri)).await?;
    assert_eq!(
        PostCounts::get_by_id(&author, &upper)
            .await?
            .unwrap()
            .reposts,
        0
    );
    assert!(PostRelationships::get_by_id(&author, &first)
        .await?
        .unwrap()
        .reposted
        .is_none());

    // Removing an embed retains a tagged resource; removing the final tag then
    // removes it. Re-adding and deleting the post also clears its last embed.
    tag::sync_put_resource(
        PubkyAppTag {
            uri: uri.clone(),
            label: "picnic".into(),
            created_at: 1,
        },
        author.clone(),
        tag_id.into(),
        "mapky".into(),
        &UserIngestor::new([]),
    )
    .await?;
    put(&author, &first, "event", content, None).await?;
    assert!(ResourceDetails::get_by_id(&resource).await?.is_some());
    tag::del(&tag_uri).await?;
    assert!(ResourceDetails::get_by_id(&resource).await?.is_none());
    put(&author, &first, "event", content, Some(&uri)).await?;
    post::del(author.clone(), first.clone(), &UserIngestor::new([])).await?;
    assert!(PostDetails::get_by_id(&author, &first).await?.is_none());
    assert!(ResourceDetails::get_by_id(&resource).await?.is_none());
    for id in [upper, punctuation] {
        post::del(author.clone(), id, &UserIngestor::new([])).await?;
    }
    Ok(())
}
