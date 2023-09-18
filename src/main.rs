#[shuttle_runtime::main]
async fn serenity(
    #[shuttle_secrets::Secrets] secret_store: SecretStore,
    #[shuttle_shared_db::Postgres(local_uri = "postgres://postgres:{secrets.\
                                               POSTGRES_PASSWORD}@localhost:\
                                               5432/sessions")]
    pool: PgPool,
) -> shuttle_serenity::ShuttleSerenity {
    #[cfg(debug_assertions)]
    info!(
        "Building in debug mode. Using DISCORD_GUILDID and DISCORD_DEVEL_TOKEN"
    );
    #[cfg(not(debug_assertions))]
    info!("Building in release mode. Using DISCORD_TOKEN");

    #[cfg(debug_assertions)]
    let (api_token, guild_id) = get_secrets(&secret_store)?;
    #[cfg(not(debug_assertions))]
    let api_token = get_secrets(&secret_store)?;

    let intents =
        GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("Failed to migrate database".to_owned())?;

    #[cfg(debug_assertions)]
    let bot = Bot::new(pool, guild_id);
    #[cfg(not(debug_assertions))]
    let bot = Bot::new(pool);

    let client = Client::builder(&api_token, intents)
        .event_handler(bot)
        .await
        .context("Failed to construct client")?;

    Ok(client.into())
}

#[cfg(debug_assertions)]
fn get_secrets(
    secret_store: &SecretStore,
) -> anyhow::Result<(String, GuildId)> {
    let Some(token) = secret_store.get("DISCORD_DEVEL_TOKEN") else {
        return Err(anyhow!("'DISCORD_DEVEL_TOKEN' was not found"));
    };

    let guild_id = match secret_store.get("DISCORD_GUILDID") {
        Some(guild_id_str) => match guild_id_str.parse::<u64>() {
            Ok(guild_id) => GuildId::from(guild_id),
            Err(e) => {
                return Err(anyhow!("'DISCORD_GUILDID' was not valid: {}", e));
            },
        },
        None => {
            return Err(anyhow!("'DISCORD_GUILDID' was not found"));
        },
    };

    Ok((token, guild_id))
}

#[cfg(not(debug_assertions))]
fn get_secrets(secret_store: &SecretStore) -> anyhow::Result<String> {
    let Some(token) = secret_store.get("DISCORD_TOKEN") else {
        return Err(anyhow!("'DISCORD_TOKEN' was not found"));
    };

    Ok(token)
}

use anyhow::{anyhow, Context};
use serenity::client::Client;
use serenity::model::gateway::GatewayIntents;
#[cfg(debug_assertions)]
use serenity::model::id::GuildId;
use shuttle_secrets::SecretStore;
use sqlx::PgPool;
use tracing::info;

mod bot;
mod eval;

use crate::bot::Bot;
