# pubky-resource-id

The id function for every epoch; v1 canonical forms are inputs, never a different hash.
Indexers outside Nexus depend on this crate to share resource identity without depending
on the social specs, graph, cache, or Nexus models. Version 0.1.0 extracts Nexus's existing
behavior without a migration or backfill.

Normalize the URI first, then hash the normalized string. The ID is the first 16 bytes
of BLAKE3, encoded as 32 lowercase hexadecimal characters.

```rust
use pubky_resource_id::{normalize_uri, resource_id};

let (uri, scheme) = normalize_uri("https://Example.com").unwrap();
assert_eq!(scheme, "https");
assert_eq!(uri, "https://example.com/");
assert_eq!(resource_id(&uri), "19f876713c227a568d939946496a0d30");
```

Normalization uses `url` parsing: lowercase scheme/host, strip default HTTP(S) ports,
userinfo and fragments, and retain the parsed path and query order. Opaque schemes
retain their opaque path with the fragment removed. If URL parsing fails, a valid
RFC 3986 scheme is lowercased and the remainder is retained exactly. This fallback
and all URL-parser behavior are preserved from Nexus; there is no epoch-specific rule.
`resource_id` itself does not normalize, classify, or fetch its input.

## Pinned vectors

These values were computed by the original Nexus functions at `ca2312f7` before the
extraction. Tests check both raw v0 spellings and their v1 canonical forms against
these same IDs.

| Input | Normalized URI | ID |
| --- | --- | --- |
| `https://Example.com` | `https://example.com/` | `19f876713c227a568d939946496a0d30` |
| `https://example.com/` | `https://example.com/` | `19f876713c227a568d939946496a0d30` |
| `HTTPS://user:pass@Example.COM:443/path?q=1#frag` | `https://example.com/path?q=1` | `2397d03755c83364010ee03600730121` |
| `https://example.com/path?q=1` | `https://example.com/path?q=1` | `2397d03755c83364010ee03600730121` |
| `http://example.com:8080/path` | `http://example.com:8080/path` | `97641daa95b144d61e98627a5d206f43` |
| `nostr:note1abc123...` | `nostr:note1abc123...` | `3252f05dba228a84b22ce7c3136fc152` |
| `pubky://8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo/pub/eventky.app/events/E001` | `pubky://8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo/pub/eventky.app/events/E001` | `aecd76ae2b6b70783008c0adaf68db6f` |
| `ipfs://bafyexample/path` | `ipfs://bafyexample/path` | `4283fe0231133801e989c931705da90d` |
