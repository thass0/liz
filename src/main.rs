#[shuttle_runtime::main]
async fn serenity(
    #[shuttle_secrets::Secrets] secret_store: SecretStore,
    #[shuttle_shared_db::Postgres] pool: PgPool,
) -> shuttle_serenity::ShuttleSerenity {
    // Get the discord token set in `Secrets.toml`
    let token = match secret_store.get("DISCORD_TOKEN") {
        Some(token) => token,
        None => {
            return Err(anyhow!("'DISCORD_TOKEN' was not found").into());
        },
    };
    let guild_id = match secret_store.get("DISCORD_GUILDID") {
        Some(guild_id_str) => match guild_id_str.parse::<u64>() {
            Ok(guild_id) => GuildId::from(guild_id),
            Err(e) => {
                return Err(
                    anyhow!("'DISCORD_GUILDID' was not valid: {}", e).into()
                );
            },
        },
        None => {
            return Err(anyhow!("'DISCORD_GUILDID' was not found").into());
        },
    };

    // Set gateway intents, which decides what events the bot
    // will be notified about
    let intents =
        GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

    let bot = match Bot::new(pool, guild_id).await {
        Ok(bot) => bot,
        Err(e) => {
            return Err(e.into());
        },
    };
    let client = Client::builder(&token, intents)
        .event_handler(bot)
        .await
        .expect("Err creating client");

    Ok(client.into())
}

use anyhow::anyhow;
use serenity::model::id::GuildId;
use serenity::prelude::*;
use shuttle_secrets::SecretStore;
use sqlx::PgPool;

mod bot;
mod eval;

use crate::bot::Bot;
