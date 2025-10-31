use crate::models::post::PostRelationships;
use crate::models::{
    alarm::AlarmDetails, attendee::AttendeeDetails, calendar::CalendarDetails,
    event::EventDetails, file::FileDetails, post::PostDetails, user::UserDetails,
};
use crate::types::DynError;
use neo4rs::{query, Query};
use pubky_app_specs::{ParsedUri, Resource};

/// Create a user node
pub fn create_user(user: &UserDetails) -> Result<Query, DynError> {
    let links = serde_json::to_string(&user.links)?;

    let query = query(
        "MERGE (u:User {id: $id})
         SET u.name = $name, u.bio = $bio, u.status = $status, u.links = $links, u.image = $image, u.indexed_at = $indexed_at;",
    )
    .param("id", user.id.to_string())
    .param("name", user.name.clone())
    .param("bio", user.bio.clone())
    .param("status", user.status.clone())
    .param("links", links)
    .param("image", user.image.clone())
    .param("indexed_at", user.indexed_at);

    Ok(query)
}

/// Creates a Cypher query to add or edit a post to the graph database and handles its relationships.
/// # Arguments
/// * `post` - A reference to a `PostDetails` struct containing information about the post to be created or edited
/// * `post_relationships` - A reference to a PostRelationships struct that define relationships
///   for the post (e.g., replies or reposts).
pub fn create_post(
    post: &PostDetails,
    post_relationships: &PostRelationships,
) -> Result<Query, DynError> {
    let mut cypher = String::new();
    let mut new_relationships = Vec::new();

    // Check if all the dependencies are consistent in the graph
    if post_relationships.replied.is_some() {
        cypher.push_str("
            MATCH (reply_parent_author:User {id: $reply_parent_author_id})-[:AUTHORED]->(reply_parent_post:Post {id: $reply_parent_post_id})
        ");
        new_relationships.push("MERGE (new_post)-[:REPLIED]->(reply_parent_post)");
    };
    if post_relationships.reposted.is_some() {
        cypher.push_str("
            MATCH (repost_parent_author:User {id: $repost_parent_author_id})-[:AUTHORED]->(repost_parent_post:Post {id: $repost_parent_post_id})
        ");
        new_relationships.push("MERGE (new_post)-[:REPOSTED]->(repost_parent_post)");
    }
    // Create the new post
    cypher.push_str(
        "
        MATCH (author:User {id: $author_id})
        OPTIONAL MATCH (u)-[:AUTHORED]->(existing_post:Post {id: $post_id})
        MERGE (author)-[:AUTHORED]->(new_post:Post {id: $post_id})
    ",
    );

    // Add relationships to the query
    cypher.push_str(&new_relationships.join("\n"));

    cypher.push_str(
        "
        SET new_post.content = $content,
            new_post.indexed_at = $indexed_at,
            new_post.kind = $kind,
            new_post.attachments = $attachments
        RETURN existing_post IS NOT NULL AS flag",
    );

    let kind = serde_json::to_string(&post.kind)?;

    let mut cypher_query = query(&cypher)
        .param("author_id", post.author.to_string())
        .param("post_id", post.id.to_string())
        .param("content", post.content.to_string())
        .param("indexed_at", post.indexed_at)
        .param("kind", kind.trim_matches('"'))
        .param(
            "attachments",
            post.attachments.clone().unwrap_or(vec![] as Vec<String>),
        );

    // Handle "replied" relationship
    cypher_query = add_relationship_params(
        cypher_query,
        &post_relationships.replied,
        "reply_parent_author_id",
        "reply_parent_post_id",
    )?;

    // Handle "reposted" relationship
    cypher_query = add_relationship_params(
        cypher_query,
        &post_relationships.reposted,
        "repost_parent_author_id",
        "repost_parent_post_id",
    )?;

    Ok(cypher_query)
}

fn add_relationship_params(
    cypher_query: Query,
    uri: &Option<String>,
    author_param: &str,
    post_param: &str,
) -> Result<Query, DynError> {
    if let Some(uri) = uri {
        let parsed_uri = ParsedUri::try_from(uri.as_str())?;
        let parent_author_id = parsed_uri.user_id;
        let parent_post_id = match parsed_uri.resource {
            Resource::Post(id) => id,
            _ => return Err("Reposted uri is not a Post resource".into()),
        };

        return Ok(cypher_query
            .param(author_param, parent_author_id.as_str())
            .param(post_param, parent_post_id.as_str()));
    }
    Ok(cypher_query)
}

/// Creates a `MENTIONED` relationship between a post and a user
/// # Arguments
/// * `author_id` - The unique identifier of the user who authored the post
/// * `post_id` - The unique identifier of the post where the mention occurs
/// * `mentioned_user_id` - The unique identifier of the user being mentioned in the post
pub fn create_mention_relationship(
    author_id: &str,
    post_id: &str,
    mentioned_user_id: &str,
) -> Query {
    query(
        "MATCH (author:User {id: $author_id})-[:AUTHORED]->(post:Post {id: $post_id}),
              (mentioned_user:User {id: $mentioned_user_id})
         MERGE (post)-[:MENTIONED]->(mentioned_user)",
    )
    .param("author_id", author_id)
    .param("post_id", post_id)
    .param("mentioned_user_id", mentioned_user_id)
}

/// Create a follows relationship between two users. Before creating the relationship,
/// it validates that both users exist in the database
/// Validates that both users exist before creating the relationship
/// # Arguments
/// * `follower_id` - The unique identifier of the user who will follow another user.
/// * `followee_id` - The unique identifier of the user to be followed.
/// * `indexed_at` - A timestamp representing when the relationship was indexed or updated.
pub fn create_follow(follower_id: &str, followee_id: &str, indexed_at: i64) -> Query {
    query(
        "MATCH (follower:User {id: $follower_id}), (followee:User {id: $followee_id})
         // Check if follow already existed
         OPTIONAL MATCH (follower)-[existing:FOLLOWS]->(followee)
         MERGE (follower)-[r:FOLLOWS]->(followee)
         SET r.indexed_at = $indexed_at
         // Returns true if the follow relationship already existed
         RETURN existing IS NOT NULL AS flag;",
    )
    .param("follower_id", follower_id.to_string())
    .param("followee_id", followee_id.to_string())
    .param("indexed_at", indexed_at)
}

/// Creates  a `MUTED` relationship between a user and another user they wish to mute
/// # Arguments
/// * `user_id` - The unique identifier of the user initiating the mute action.
/// * `muted_id` - The unique identifier of the user to be muted.
/// * `indexed_at` - A timestamp indicating when the relationship was created or last updated.
pub fn create_mute(user_id: &str, muted_id: &str, indexed_at: i64) -> Query {
    query(
        "MATCH (user:User {id: $user_id}), (muted:User {id: $muted_id})
        // Check if follow already existed
        OPTIONAL MATCH (user)-[existing:MUTED]->(muted) 
        MERGE (user)-[r:MUTED]->(muted)
        SET r.indexed_at = $indexed_at
         // Returns true if the mute relationship already existed
        RETURN existing IS NOT NULL AS flag;",
    )
    .param("user_id", user_id.to_string())
    .param("muted_id", muted_id.to_string())
    .param("indexed_at", indexed_at)
}

/// Creates a "BOOKMARKED" relationship between a user and a post authored by another user
/// # Arguments
/// * `user_id` - The unique identifier of the user bookmarking the post.
/// * `author_id` - The unique identifier of the user who authored the post.
/// * `post_id` - The unique identifier of the post being bookmarked.
/// * `bookmark_id` - A unique identifier for the bookmark relationship.
/// * `indexed_at` - A timestamp representing when the bookmark relationship was created or last updated.
pub fn create_post_bookmark(
    user_id: &str,
    author_id: &str,
    post_id: &str,
    bookmark_id: &str,
    indexed_at: i64,
) -> Query {
    query(
        "MATCH (u:User {id: $user_id})
        // We assume these nodes are already created. If not we would not be able to add a bookmark
        MATCH (author:User {id: $author_id})-[:AUTHORED]->(p:Post {id: $post_id})
        // Check if bookmark already existed
        OPTIONAL MATCH (u)-[existing:BOOKMARKED]->(p) 
        MERGE (u)-[b:BOOKMARKED]->(p)
        SET b.indexed_at = $indexed_at,
            b.id = $bookmark_id
        // Returns true if the bookmark relationship already existed
        RETURN existing IS NOT NULL AS flag;",
    )
    .param("user_id", user_id)
    .param("author_id", author_id)
    .param("post_id", post_id)
    .param("bookmark_id", bookmark_id)
    .param("indexed_at", indexed_at)
}

/// Creates a `TAGGED` relationship between a user and a post authored by another user. The tag is uniquely
/// identified by a `label` and is associated with the post
/// # Arguments
/// * `user_id` - The unique identifier of the user tagging the post.
/// * `author_id` - The unique identifier of the user who authored the post.
/// * `post_id` - The unique identifier of the post being tagged.
/// * `tag_id` - A unique identifier for the tagging relationship.
/// * `label` - A string representing the label of the tag.
/// * `indexed_at` - A timestamp representing when the tagging relationship was created or last updated.
///
pub fn create_post_tag(
    user_id: &str,
    author_id: &str,
    post_id: &str,
    tag_id: &str,
    label: &str,
    indexed_at: i64,
) -> Query {
    query(
        "MATCH (user:User {id: $user_id})
        // We assume these nodes are already created. If not we would not be able to add a tag
        MATCH (author:User {id: $author_id})-[:AUTHORED]->(post:Post {id: $post_id})
        // Check if tag already existed
        OPTIONAL MATCH (user)-[existing:TAGGED {label: $label}]->(post) 
        MERGE (user)-[t:TAGGED {label: $label}]->(post)
        SET t.indexed_at = $indexed_at,
            t.id = $tag_id
        // Returns true if the post tag relationship already existed
        RETURN existing IS NOT NULL AS flag;",
    )
    .param("user_id", user_id)
    .param("author_id", author_id)
    .param("post_id", post_id)
    .param("tag_id", tag_id)
    .param("label", label)
    .param("indexed_at", indexed_at)
}

/// Creates a `TAGGED` relationship between two users. The relationship is uniquely identified by a `label`
/// # Arguments
/// * `tagger_user_id` - The unique identifier of the user creating the tag.
/// * `tagged_user_id` - The unique identifier of the user being tagged.
/// * `tag_id` - A unique identifier for the tagging relationship.
/// * `label` - A string representing the label of the tag.
/// * `indexed_at` - A timestamp indicating when the tagging relationship was created or last updated.
pub fn create_user_tag(
    tagger_user_id: &str,
    tagged_user_id: &str,
    tag_id: &str,
    label: &str,
    indexed_at: i64,
) -> Query {
    query(
        "MATCH (tagged_used:User {id: $tagged_user_id})
        MATCH (tagger:User {id: $tagger_user_id})
        // Check if tag already existed
        OPTIONAL MATCH (tagger)-[existing:TAGGED {label: $label}]->(tagged_used) 
        MERGE (tagger)-[t:TAGGED {label: $label}]->(tagged_used)
        SET t.indexed_at = $indexed_at,
            t.id = $tag_id
        // Returns true if the user tag relationship already existed
        RETURN existing IS NOT NULL AS flag;",
    )
    .param("tagger_user_id", tagger_user_id)
    .param("tagged_user_id", tagged_user_id)
    .param("tag_id", tag_id)
    .param("label", label)
    .param("indexed_at", indexed_at)
}

/// Create a file node
pub fn create_file(file: &FileDetails) -> Result<Query, DynError> {
    let urls = serde_json::to_string(&file.urls)?;

    let query = query(
        "MERGE (f:File {id: $id, owner_id: $owner_id})
         SET f.uri = $uri, f.indexed_at = $indexed_at, f.created_at = $created_at, f.size = $size,
            f.src = $src, f.name = $name, f.content_type = $content_type, f.urls = $urls;",
    )
    .param("id", file.id.to_string())
    .param("owner_id", file.owner_id.to_string())
    .param("uri", file.uri.to_string())
    .param("indexed_at", file.indexed_at)
    .param("created_at", file.created_at)
    .param("size", file.size)
    .param("src", file.src.to_string())
    .param("name", file.name.to_string())
    .param("content_type", file.content_type.to_string())
    .param("urls", urls);

    Ok(query)
}

/// Create a homeserver
pub fn create_homeserver(homeserver_id: &str) -> Query {
    query(
        "MERGE (hs:Homeserver {
          id: $id
        })
        RETURN hs;",
    )
    .param("id", homeserver_id)
}

// Calendar creation functions

pub fn create_calendar(calendar: &CalendarDetails) -> Result<Query, DynError> {
    let x_pubky_admins = calendar.x_pubky_admins.as_ref()
        .and_then(|a| serde_json::to_string(a).ok());

    let query = query(
        "MATCH (u:User {id: $author})
         MERGE (u)-[:AUTHORED]->(c:Calendar {id: $id})
         SET c.indexed_at = $indexed_at,
             c.name = $name,
             c.timezone = $timezone,
             c.color = $color,
             c.description = $description,
             c.url = $url,
             c.image_uri = $image_uri,
             c.x_pubky_admins = $x_pubky_admins,
             c.created = $created",
    )
    .param("author", calendar.author.clone())
    .param("id", calendar.id.clone())
    .param("indexed_at", calendar.indexed_at)
    .param("name", calendar.name.clone())
    .param("timezone", calendar.timezone.clone())
    .param("color", calendar.color.clone())
    .param("description", calendar.description.clone())
    .param("url", calendar.url.clone())
    .param("image_uri", calendar.image_uri.clone())
    .param("x_pubky_admins", x_pubky_admins)
    .param("created", calendar.created);

    Ok(query)
}

pub fn create_event(event: &EventDetails) -> Result<Query, DynError> {
    let rdate = event.rdate.as_ref()
        .and_then(|r| serde_json::to_string(r).ok());
    let exdate = event.exdate.as_ref()
        .and_then(|e| serde_json::to_string(e).ok());
    let categories = event.categories.as_ref()
        .and_then(|c| serde_json::to_string(c).ok());
    let conference = event.conference.as_ref()
        .and_then(|c| serde_json::to_string(c).ok());
    let image = event.image.as_ref()
        .and_then(|i| serde_json::to_string(i).ok());
    let attach = event.attach.as_ref()
        .and_then(|a| serde_json::to_string(a).ok());
    let attendee = event.attendee.as_ref()
        .and_then(|a| serde_json::to_string(a).ok());
    let comment = event.comment.as_ref()
        .and_then(|c| serde_json::to_string(c).ok());
    let contact = event.contact.as_ref()
        .and_then(|c| serde_json::to_string(c).ok());
    let related_to = event.related_to.as_ref()
        .and_then(|r| serde_json::to_string(r).ok());
    let request_status = event.request_status.as_ref()
        .and_then(|r| serde_json::to_string(r).ok());
    let resources = event.resources.as_ref()
        .and_then(|r| serde_json::to_string(r).ok());
    let participant_type = event.participant_type.as_ref()
        .and_then(|p| serde_json::to_string(p).ok());
    let resource_type = event.resource_type.as_ref()
        .and_then(|r| serde_json::to_string(r).ok());

    let query = query(
        "MATCH (u:User {id: $author})
         MERGE (u)-[:AUTHORED]->(e:Event {id: $id})
         SET e.indexed_at = $indexed_at,
             e.uid = $uid,
             e.dtstamp = $dtstamp,
             e.dtstart = $dtstart,
             e.summary = $summary,
             e.dtend = $dtend,
             e.duration = $duration,
             e.rrule = $rrule,
             e.rdate = $rdate,
             e.exdate = $exdate,
             e.description = $description,
             e.status = $status,
             e.location = $location,
             e.geo = $geo,
             e.organizer = $organizer,
             e.url = $url,
             e.categories = $categories,
             e.sequence = $sequence,
             e.conference = $conference,
             e.image = $image,
             e.structured_locations = $structured_locations,
             e.styled_description = $styled_description,
             e.x_pubky_recurrence_id = $x_pubky_recurrence_id,
             e.x_pubky_calendar_uri = $x_pubky_calendar_uri,
             e.x_pubky_rsvp_access = $x_pubky_rsvp_access,
             e.class = $class,
             e.priority = $priority,
             e.transp = $transp,
             e.attach = $attach,
             e.attendee = $attendee,
             e.comment = $comment,
             e.contact = $contact,
             e.related_to = $related_to,
             e.request_status = $request_status,
             e.resources = $resources,
             e.color = $color,
             e.participant_type = $participant_type,
             e.resource_type = $resource_type,
             e.structured_data = $structured_data,
             e.styled_description_param = $styled_description_param",
    )
    .param("author", event.author.clone())
    .param("id", event.id.clone())
    .param("indexed_at", event.indexed_at)
    .param("uid", event.uid.clone())
    .param("dtstamp", event.dtstamp.clone())
    .param("dtstart", event.dtstart.clone())
    .param("summary", event.summary.clone())
    .param("dtend", event.dtend.clone())
    .param("duration", event.duration.clone())
    .param("rrule", event.rrule.clone())
    .param("rdate", rdate)
    .param("exdate", exdate)
    .param("description", event.description.clone())
    .param("status", event.status.clone())
    .param("location", event.location.clone())
    .param("geo", event.geo.clone())
    .param("organizer", event.organizer.clone())
    .param("url", event.url.clone())
    .param("categories", categories)
    .param("sequence", event.sequence)
    .param("conference", conference)
    .param("image", image)
    .param("structured_locations", event.structured_locations.clone())
    .param("styled_description", event.styled_description.clone())
    .param("x_pubky_recurrence_id", event.x_pubky_recurrence_id.clone())
    .param("x_pubky_calendar_uri", event.x_pubky_calendar_uri.clone())
    .param("x_pubky_rsvp_access", event.x_pubky_rsvp_access.clone())
    .param("class", event.class.clone())
    .param("priority", event.priority)
    .param("transp", event.transp.clone())
    .param("attach", attach)
    .param("attendee", attendee)
    .param("comment", comment)
    .param("contact", contact)
    .param("related_to", related_to)
    .param("request_status", request_status)
    .param("resources", resources)
    .param("color", event.color.clone())
    .param("participant_type", participant_type)
    .param("resource_type", resource_type)
    .param("structured_data", event.structured_data.clone())
    .param("styled_description_param", event.styled_description_param.clone());

    Ok(query)
}

pub fn create_attendee(attendee: &AttendeeDetails) -> Result<Query, DynError> {
    let query = query(
        "MATCH (u:User {id: $author})
         MERGE (u)-[:AUTHORED]->(a:Attendee {id: $id})
         SET a.indexed_at = $indexed_at,
             a.attendee_uri = $attendee_uri,
             a.partstat = $partstat,
             a.x_pubky_event_uri = $x_pubky_event_uri,
             a.role = $role,
             a.rsvp = $rsvp,
             a.delegated_to = $delegated_to,
             a.delegated_from = $delegated_from",
    )
    .param("author", attendee.author.clone())
    .param("id", attendee.id.clone())
    .param("indexed_at", attendee.indexed_at)
    .param("attendee_uri", attendee.attendee_uri.clone())
    .param("partstat", attendee.partstat.clone())
    .param("x_pubky_event_uri", attendee.x_pubky_event_uri.clone())
    .param("role", attendee.role.clone())
    .param("rsvp", attendee.rsvp)
    .param("delegated_to", attendee.delegated_to.clone())
    .param("delegated_from", attendee.delegated_from.clone());

    Ok(query)
}

pub fn create_alarm(alarm: &AlarmDetails) -> Result<Query, DynError> {
    let attach = alarm.attach.as_ref()
        .and_then(|a| serde_json::to_string(a).ok());
    let attendees = alarm.attendees.as_ref()
        .and_then(|a| serde_json::to_string(a).ok());

    let query = query(
        "MATCH (u:User {id: $author})
         MERGE (u)-[:AUTHORED]->(a:Alarm {id: $id})
         SET a.indexed_at = $indexed_at,
             a.action = $action,
             a.trigger = $trigger,
             a.x_pubky_target_uri = $x_pubky_target_uri,
             a.duration = $duration,
             a.repeat = $repeat,
             a.attach = $attach,
             a.description = $description,
             a.summary = $summary,
             a.attendees = $attendees",
    )
    .param("author", alarm.author.clone())
    .param("id", alarm.id.clone())
    .param("indexed_at", alarm.indexed_at)
    .param("action", alarm.action.clone())
    .param("trigger", alarm.trigger.clone())
    .param("x_pubky_target_uri", alarm.x_pubky_target_uri.clone())
    .param("duration", alarm.duration.clone())
    .param("repeat", alarm.repeat)
    .param("attach", attach)
    .param("description", alarm.description.clone())
    .param("summary", alarm.summary.clone())
    .param("attendees", attendees);

    Ok(query)
}

