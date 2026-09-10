use anyhow::{Error, Result};
use nexus_common::{StackConfig, StackManager};

use crate::service::utils::MockEventProcessor;

pub const HS_IDS: [&str; 5] = [
    "1hb71xx9km3f4pw5izsy1gn19ff1uuuqonw4mcygzobwkryujoiy",
    "8rsrmfrn1anbrzuxiffwy1174o58emf4qgbfk5h7s8a33r3bd8dy",
    "984orjzbusofbqhsqz9axpez3uuwd3hbpqztd6rtx3pr78y9s1my",
    "mamtihagiptrngan9y6cdj1xu7yb8yc7us9uerytaewc13ejqy9y",
    "8x93apuue6kjyqosu1wp9xye45j9noq8y3pmuwmhfo3o95eimgoo",
];

pub async fn setup() -> Result<Vec<MockEventProcessor>> {
    // Initialize the test stack
    if let Err(e) = StackManager::setup(&StackConfig::default()).await {
        return Err(Error::msg(format!("could not initialise the stack, {e:?}")));
    }

    Ok(Vec::new())
}

/// Refuse default/shared stores for opt-in primary ownership integration tests.
pub async fn setup_primary_disposable() -> Result<()> {
    let neo4j = std::env::var("NEXUS_TEST_NEO4J_URI")?;
    let redis = std::env::var("NEXUS_TEST_REDIS_URI")?;
    anyhow::ensure!(
        neo4j == "bolt://127.0.0.1:17687",
        "requires disposable Neo4j tunnel"
    );
    anyhow::ensure!(
        redis == "redis://127.0.0.1:16379",
        "requires disposable Redis tunnel"
    );
    let mut config = StackConfig::default();
    config.db.neo4j.uri = neo4j;
    config.db.neo4j.password = "unused".into();
    config.db.redis = redis;
    StackManager::setup(&config)
        .await
        .map_err(|error| Error::msg(format!("could not initialise disposable stack: {error}")))?;
    Ok(())
}
