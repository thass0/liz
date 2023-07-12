use anyhow::anyhow;
use serenity::async_trait;
use serenity::model::application::interaction::{
    Interaction,
    InteractionResponseType,
};
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::prelude::command::CommandOptionType;
use serenity::model::prelude::GuildId;
use serenity::prelude::*;
use shuttle_secrets::SecretStore;
use tracing::{error, info};

use crate::eval::eval;

pub mod eval;

struct Bot;

const CMD_EVAL: &str = "eval";
const CMD_EVAL_SEXPR: &str = "sexpr";

#[async_trait]
impl EventHandler for Bot {
    // Every time our bot receives a message, it adds this
    // message to the eval.
    async fn message(&self, ctx: Context, msg: Message) {
        if msg.content == "!hello" {
            if let Err(e) = msg.channel_id.say(&ctx.http, "world!").await {
                error!("Error sending message: {:?}", e);
            }
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);

        let guild_id = GuildId(1128676502272229468);
        let commands = GuildId::set_application_commands(
            &guild_id,
            &ctx.http,
            |commands| {
                commands.create_application_command(|command| {
                    command
                        .name(CMD_EVAL)
                        .description("Evaluate an S-expression")
                        .create_option(|option| {
                            option
                                .name(CMD_EVAL_SEXPR)
                                .description("S-expression to evaluate")
                                .kind(CommandOptionType::String)
                                .required(true)
                        })
                })
            },
        )
        .await
        .unwrap();

        info!("{:#?}", commands);
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            let response_content = match command.data.name.as_str() {
                CMD_EVAL => {
                    let arg = command
                        .data
                        .options
                        .iter()
                        .find(|opt| opt.name == CMD_EVAL_SEXPR)
                        .cloned();
                    let value = arg.unwrap().value.unwrap();
                    let sexpr_str = value.as_str().unwrap();
                    eval(sexpr_str)
                },
                command => unreachable!("Unknown command: {}", command),
            };

            let create_response =
                command.create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message.content(response_content)
                        })
                });

            if let Err(why) = create_response.await {
                error!("Cannot respond to slash command: {}", why);
            }
        }
    }
}

#[shuttle_runtime::main]
async fn serenity(
    #[shuttle_secrets::Secrets] secret_store: SecretStore,
) -> shuttle_serenity::ShuttleSerenity {
    // Get the discord token set in `Secrets.toml`
    let token = if let Some(token) = secret_store.get("DISCORD_TOKEN") {
        token
    } else {
        return Err(anyhow!("'DISCORD_TOKEN' was not found").into());
    };

    // Set gateway intents, which decides what events the bot
    // will be notified about
    let intents =
        GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

    let client = Client::builder(&token, intents)
        .event_handler(Bot)
        .await
        .expect("Err creating client");

    Ok(client.into())
}
