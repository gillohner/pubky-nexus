//! App-owned v1 post envelopes and content-free graph metadata.

use crate::db::graph::{GraphError, GraphResult, Query};
use crate::db::{fetch_row_from_graph, get_neo4j_graph};
use crate::types::Pagination;
use crate::universal_tag::normalize::{normalize_uri, resource_id};
use futures::TryStreamExt;
use pubky_social_specs::traits::{TimestampId, Validatable};
use pubky_social_specs::{
    legacy_v0, ParsedUri, PostEnvelope, PostKind, PubkyId, Resource, Root, ValidationCtx,
    Visibility,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub fn valid_kind(value: &str) -> bool {
    (1..=32).contains(&value.len())
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

pub fn valid_namespace(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-_".contains(&b))
}

/// The app owns the vocabulary and content semantics. Nexus accepts bounded lowercase
/// kind tokens, leaving content and extensions entirely to the app.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct AppKind(pub String);

impl PostKind for AppKind {
    fn is_known(&self) -> bool {
        valid_kind(&self.0)
    }
    fn validate_content(
        &self,
        _post: &PostEnvelope<Self>,
        _ctx: &ValidationCtx,
    ) -> Result<(), String> {
        Ok(())
    }
}

pub type AppPostInput = PostEnvelope<AppKind>;

/// One logical post and, for stored events, its version leaf. The social reader
/// regards app paths as foreign; its post path parser still owns the shared layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPostUri {
    pub author: PubkyId,
    pub namespace: String,
    pub id: String,
    pub version: Option<String>,
    pub label: Option<String>,
}

impl AppPostUri {
    /// Social post addresses use the same version layout, but retain social routing.
    pub fn parse_social(uri: &str) -> Option<Self> {
        let parsed = ParsedUri::try_from(uri).ok()?;
        if parsed.visibility != Visibility::Public || parsed.try_to_uri_str().ok()?.as_str() != uri
        {
            return None;
        }
        let Resource::Post { id, version, label } = parsed.resource else {
            return None;
        };
        Some(Self {
            author: parsed.user_id,
            namespace: "social".into(),
            id,
            version,
            label,
        })
    }

    pub fn parse(uri: &str) -> Option<Self> {
        let parsed = ParsedUri::try_from(uri).ok()?;
        if parsed.visibility != Visibility::Public || parsed.try_to_uri_str().ok()?.as_str() != uri
        {
            return None;
        }
        let Resource::Foreign {
            namespace,
            version: Some(epoch),
            rest,
        } = parsed.resource
        else {
            return None;
        };
        if namespace == "pubky.app" || !valid_namespace(&namespace) || epoch != "v1" {
            return None;
        }
        // Reuse the pinned reader's timestamp, leaf, slug and segment rules rather
        // than maintaining a second version-path grammar for app posts.
        let mirror = format!(
            "pubky://{}/pub/social/v1/{}",
            parsed.user_id,
            rest.join("/")
        );
        let social = ParsedUri::try_from(mirror.as_str()).ok()?;
        if social.try_to_uri_str().ok()?.as_str() != mirror {
            return None;
        }
        let Resource::Post { id, version, label } = social.resource else {
            return None;
        };
        Some(Self {
            author: parsed.user_id,
            namespace,
            id,
            version,
            label,
        })
    }

    pub fn path(&self) -> String {
        format!("/pub/{}/v1/posts/{}", self.namespace, self.id)
    }
    pub fn uri(&self) -> String {
        format!("pubky://{}{}", self.author, self.path())
    }
    pub fn version_uri(&self) -> String {
        format!("pubky://{}{}", self.author, self.version_path())
    }
    pub fn version_path(&self) -> String {
        match &self.version {
            Some(version) => AppPostInput::version_path(
                &self.namespace,
                Root::Pub,
                &self.id,
                version,
                self.label.as_deref(),
            ),
            None => self.path(),
        }
    }

    pub fn read(&self, bytes: &[u8]) -> Result<AppPostInput, String> {
        let version = self
            .version
            .as_deref()
            .ok_or("Validation Error: a versionless post reference is never a stored object")?;
        let ctx = ValidationCtx { root: Root::Pub };
        let post = <AppPostInput as Validatable>::try_from(bytes, &self.id, &ctx)?;
        // Same version rules as PubkySocialObject::from_resource at the pinned revision.
        post.validate_id(version)?;
        if version.as_bytes() < self.id.as_bytes() {
            return Err("Validation Error: version is older than the post id".into());
        }
        post.check_references(&ctx, Some(&self.author))?;
        Ok(post)
    }
}

/// Metadata returned only for an explicit namespace stream. The URI is the
/// logical address; version_uri names the latest accepted file for hydration.
/// Content, ext, attachments and lock are not stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AppPost {
    pub uri: String,
    pub version_uri: String,
    pub id: String,
    pub author: String,
    pub path: String,
    pub namespace: String,
    pub kind: String,
    pub indexed_at: i64,
    pub parent: Option<String>,
    pub embed: Option<String>,
}

impl AppPost {
    pub fn new(path: &AppPostUri, input: AppPostInput) -> Self {
        Self {
            uri: path.uri(),
            version_uri: path.version_uri(),
            id: path.id.clone(),
            author: path.author.to_string(),
            path: path.path(),
            namespace: path.namespace.clone(),
            kind: input.kind.0,
            indexed_at: chrono::Utc::now().timestamp_millis(),
            parent: input.parent.map(canonical_reference),
            embed: input.embed.map(canonical_reference),
        }
    }

    pub async fn get(uri: &str) -> GraphResult<Option<Self>> {
        fetch_row_from_graph(
            Query::new(
                "app_post_get",
                "MATCH (p:AppPost {uri: $uri}) RETURN p AS post",
            )
            .param("uri", uri),
        )
        .await?
        .map(|row| row.get("post").map_err(Into::into))
        .transpose()
    }

    /// One transaction replaces the metadata and outgoing edges. Missing Pubky
    /// post dependencies leave no partial node; the watcher retries those paths.
    pub async fn put(&self) -> GraphResult<bool> {
        let mut cypher = String::from("MATCH (author:User {id: $author})\n");
        let mut edges = String::new();
        let mut params: Vec<(String, String)> = Vec::new();
        for (name, uri, edge) in [
            ("parent", &self.parent, "APP_REPLIED"),
            ("embed", &self.embed, "APP_EMBEDDED"),
        ] {
            let Some(uri) = uri else { continue };
            let target = reference_target(uri);
            match target {
                ReferenceTarget::App => {
                    // Self-references use the node created below.
                    if uri == &self.uri {
                        edges.push_str(&format!("MERGE (p)-[:{edge}]->(p)\n"));
                        continue;
                    }
                    cypher.push_str(&format!("MATCH ({name}:AppPost {{uri: ${name}_uri}})\n"));
                    params.push((format!("{name}_uri"), uri.clone()));
                }
                ReferenceTarget::Social { author, id } => {
                    cypher.push_str(&format!("MATCH (:User {{id: ${name}_author}})-[:AUTHORED]->({name}:Post {{id: ${name}_id}})\n"));
                    params.extend([
                        (format!("{name}_author"), author),
                        (format!("{name}_id"), id),
                    ]);
                }
                ReferenceTarget::Resource => {
                    let (normalized, scheme) =
                        normalize_uri(uri).map_err(GraphError::QueryBuildError)?;
                    // Resource MERGEs must follow every dependency MATCH, so a missing
                    // dependency never leaves a Resource behind either.
                    edges.push_str(&format!("MERGE ({name}:Resource {{id: ${name}_id}}) ON CREATE SET {name}.uri = ${name}_uri, {name}.scheme = ${name}_scheme, {name}.indexed_at = $at\n"));
                    params.extend([
                        (format!("{name}_id"), resource_id(&normalized)),
                        (format!("{name}_uri"), normalized),
                        (format!("{name}_scheme"), scheme),
                    ]);
                }
            }
            edges.push_str(&format!("MERGE (p)-[:{edge}]->({name})\n"));
        }
        cypher.push_str("MERGE (p:AppPost {uri: $uri}) ON CREATE SET p.indexed_at = $at\nSET p.version_uri = $version_uri, p.id = $id, p.author = $author, p.path = $path, p.namespace = $namespace, p.kind = $kind, p.parent = $parent, p.embed = $embed\nMERGE (author)-[:AUTHORED]->(p)\nWITH *\nCALL (p) { MATCH (p)-[old:APP_REPLIED|APP_EMBEDDED]->() DELETE old }\n");
        cypher.push_str(&edges);
        cypher.push_str("RETURN DISTINCT p.uri AS uri");
        let query = Query::new("app_post_put", cypher)
            .params(params)
            .param("uri", self.uri.clone())
            .param("version_uri", self.version_uri.clone())
            .param("id", self.id.clone())
            .param("author", self.author.clone())
            .param("path", self.path.clone())
            .param("namespace", self.namespace.clone())
            .param("kind", self.kind.clone())
            .param("at", self.indexed_at)
            .param("parent", self.parent.clone())
            .param("embed", self.embed.clone());
        Ok(fetch_row_from_graph(query).await?.is_some())
    }

    pub async fn delete(uri: &str) -> GraphResult<()> {
        get_neo4j_graph()?
            .run(
                Query::new(
                    "app_post_delete",
                    "MATCH (p:AppPost {uri: $uri}) DETACH DELETE p",
                )
                .param("uri", uri),
            )
            .await?;
        Ok(())
    }

    pub async fn stream(
        namespace: &str,
        kind: Option<&str>,
        exclude: &[String],
        author: Option<&str>,
        pagination: Pagination,
        ascending: bool,
    ) -> GraphResult<Vec<Self>> {
        let order = if ascending { "ASC" } else { "DESC" };
        let (start_op, end_op) = if ascending {
            (">=", "<=")
        } else {
            ("<=", ">=")
        };
        let query = Query::new("app_post_stream", format!("MATCH (p:AppPost {{namespace: $namespace}}) WHERE ($kind IS NULL OR p.kind = $kind) AND NOT p.kind IN $exclude AND ($author IS NULL OR p.author = $author) AND ($start IS NULL OR p.indexed_at {start_op} $start) AND ($end IS NULL OR p.indexed_at {end_op} $end) RETURN p AS post ORDER BY p.indexed_at {order}, p.uri {order} SKIP $skip LIMIT $limit"))
            .param("namespace", namespace).param("kind", kind.map(str::to_owned)).param("exclude", exclude.to_vec())
            .param("author", author.map(str::to_owned)).param("start", pagination.start).param("end", pagination.end)
            .param("skip", pagination.skip.unwrap_or(0) as i64).param("limit", pagination.limit.unwrap_or(10) as i64);
        let mut rows = get_neo4j_graph()?.execute(query).await?;
        let mut posts = Vec::new();
        while let Some(row) = rows.try_next().await? {
            posts.push(row.get("post")?);
        }
        Ok(posts)
    }
}

enum ReferenceTarget {
    App,
    Social { author: String, id: String },
    Resource,
}

fn canonical_reference(uri: String) -> String {
    AppPostUri::parse(&uri).map_or(uri, |post| post.uri())
}

fn reference_target(uri: &str) -> ReferenceTarget {
    if AppPostUri::parse(uri).is_some() {
        return ReferenceTarget::App;
    }
    if let Ok(parsed) = legacy_v0::ParsedUri::try_from(uri) {
        if parsed.try_to_uri_str().ok().as_deref() == Some(uri) {
            if let legacy_v0::Resource::Post(id) = parsed.resource {
                return ReferenceTarget::Social {
                    author: parsed.user_id.to_string(),
                    id,
                };
            }
        }
    }
    if let Ok(parsed) = ParsedUri::try_from(uri) {
        if parsed.visibility == Visibility::Public
            && parsed.try_to_uri_str().ok().as_deref() == Some(uri)
        {
            if let Resource::Post { id, .. } = parsed.resource {
                return ReferenceTarget::Social {
                    author: parsed.user_id.to_string(),
                    id,
                };
            }
        }
    }
    ReferenceTarget::Resource
}

pub fn is_post_reference(uri: &str) -> bool {
    !matches!(reference_target(uri), ReferenceTarget::Resource)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const AUTHOR: &str = "operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo";
    const ID: &str = "0032SSN7Q4EVG";
    const EDIT: &str = "0032SSN7Q4EVJ";

    fn stored() -> AppPostUri {
        AppPostUri::parse(&format!(
            "pubky://{AUTHOR}/pub/music/v1/posts/{ID}/{EDIT}.json"
        ))
        .unwrap()
    }

    #[test]
    fn canonical_versions_share_a_logical_identity() {
        let plain = stored();
        let labelled =
            AppPostUri::parse(&plain.version_uri().replace(".json", "-my-session.json")).unwrap();
        assert_eq!(plain.uri(), labelled.uri());
        assert_ne!(plain.version_uri(), labelled.version_uri());
        assert_eq!(AppPostUri::parse(&plain.uri()).unwrap().version, None);
        for uri in [
            plain.version_uri().replace("pubky://", "pubky"),
            plain.version_uri().replace("/pub/", "/priv/"),
            plain.version_uri().replace(".json", ".txt"),
            plain.version_uri().replace("/music/", "/pubky.app/"),
            plain.version_uri().replace("/music/", "/social/"),
            format!("{}/extra", plain.version_uri()),
        ] {
            assert!(AppPostUri::parse(&uri).is_none(), "{uri}");
        }
    }

    #[test]
    fn shared_envelope_validates_fields_but_leaves_app_content_and_extensions_opaque() {
        let path = stored();
        let body = json!({"kind":"my_kind-2", "content":"  arbitrary, not JSON  ", "ext":{"future": [null, 7]}});
        let post = path.read(&serde_json::to_vec(&body).unwrap()).unwrap();
        assert_eq!(post.content, "  arbitrary, not JSON  ");
        assert_eq!(post.extra["ext"], body["ext"]);
        // V1 ids belong to the path; no body id or private copy of the envelope is required.
        for (field, value) in [
            ("kind", json!("Upper")),
            ("kind", json!("x".repeat(33))),
            ("content", json!({})),
            ("attachments", json!(null)),
            ("parent", json!("relative/path")),
            (
                "attachments",
                json!([{"uri":"https://example.com/x", "name":" "}]),
            ),
            ("lock", json!("https://example.com/lock")),
            ("ext", json!("x".repeat(512 * 1024))),
        ] {
            let mut invalid = body.clone();
            invalid[field] = value;
            assert!(
                path.read(&serde_json::to_vec(&invalid).unwrap()).is_err(),
                "{field}"
            );
        }
        let mut old_version = path.clone();
        old_version.id = EDIT.into();
        old_version.version = Some(ID.into());
        assert!(old_version
            .read(&serde_json::to_vec(&body).unwrap())
            .is_err());
        assert!(AppPostUri::parse(&path.uri())
            .unwrap()
            .read(&serde_json::to_vec(&body).unwrap())
            .is_err());
    }
}
