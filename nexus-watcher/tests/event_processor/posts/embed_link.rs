//! pubky.app writes the link its author picked for the post's card as the post's `embed`. A
//! link embed is not a repost: the post view returns it as `relationships.embedded`, and an edit
//! that picks another link, or none, updates it.
use super::utils::{short_post, short_repost, test_user};
use crate::event_processor::utils::watcher::WatcherTest;
use anyhow::Result;
use nexus_common::models::post::{PostRelationships, PostView};
use pubky::Keypair;
use pubky_social_specs::legacy_v0::{
    post_uri_builder, PubkyAppPost, PubkyAppPostEmbed, PubkyAppPostKind,
};

fn link_post(content: &str, link: Option<&str>) -> PubkyAppPost {
    PubkyAppPost {
        embed: link.map(|uri| PubkyAppPostEmbed {
            kind: PubkyAppPostKind::Link,
            uri: uri.into(),
        }),
        ..short_post(content)
    }
}

/// The embedded link as the cached relationships and the post view return it.
async fn embedded(author_id: &str, post_id: &str) -> Result<(Option<String>, Option<String>)> {
    let index = PostRelationships::get_from_index(author_id, post_id)
        .await?
        .expect("the relationships are indexed")
        .embedded;
    let view = PostView::get_by_id(author_id, post_id, None, None, None)
        .await?
        .expect("the post is indexed")
        .relationships
        .embedded;
    Ok((index, view))
}

#[tokio_shared_rt::test(shared)]
async fn test_link_embed_is_returned_and_follows_edits() -> Result<()> {
    let mut test = WatcherTest::setup(None).await?;
    let author_kp = Keypair::random();
    let author_id = test
        .create_user(&author_kp, &test_user("Watcher:EmbedLink:Author", "embed"))
        .await?;

    let first = format!("https://example.com/first/{author_id}");
    let second = format!("https://example.com/second/{author_id}");
    let third = format!("https://example.com/third/{author_id}");
    let content = format!("three links: {first} {second} {third}");

    // The author picks the second link. Nexus keeps the Resource's normalized address: a
    // lowercase host and no fragment.
    let picked = second.replace("example.com", "EXAMPLE.com") + "#top";
    let (post_id, post_path) = test
        .create_post(&author_kp, &link_post(&content, Some(&picked)))
        .await?;
    assert_eq!(
        embedded(&author_id, &post_id).await?,
        (Some(second.clone()), Some(second.clone())),
        "the picked link is the post's embed"
    );
    let relationships = PostRelationships::get_from_graph(&author_id, &post_id)
        .await?
        .unwrap();
    assert_eq!(relationships.reposted, None, "a link embed is no repost");

    // An edit that picks the third link moves the embed.
    test.put(&author_kp, &post_path, link_post(&content, Some(&third)))
        .await?;
    assert_eq!(
        embedded(&author_id, &post_id).await?,
        (Some(third.clone()), Some(third.clone())),
        "an edit moves the embed"
    );

    // An edit with no embed drops it; the app then shows the first link again.
    test.put(&author_kp, &post_path, link_post(&content, None))
        .await?;
    assert_eq!(
        embedded(&author_id, &post_id).await?,
        (None, None),
        "an edit without an embed drops it"
    );

    // An embed of a post stays a repost, with no link embed.
    let (repost_id, repost_path) = test
        .create_post(
            &author_kp,
            &short_repost(
                "Watcher:EmbedLink:Repost",
                post_uri_builder(author_id.clone(), post_id.clone()),
            ),
        )
        .await?;
    let relationships = PostRelationships::get_from_graph(&author_id, &repost_id)
        .await?
        .unwrap();
    assert!(relationships.reposted.is_some(), "a post embed is a repost");
    assert_eq!(embedded(&author_id, &repost_id).await?, (None, None));

    test.cleanup_post(&author_kp, &repost_path).await?;
    test.cleanup_post(&author_kp, &post_path).await?;
    Ok(())
}
