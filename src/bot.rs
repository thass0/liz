pub struct Bot {
    guild_id: GuildId,
    _db:      PgPool,
}

impl Bot {
    pub async fn new(
        db: PgPool,
        guild_id: GuildId,
    ) -> Result<Self, CustomError> {
        db.execute(include_str!("../schema.sql"))
            .await
            .map_err(CustomError::new)?;

        Ok(Bot { _db: db, guild_id })
    }
}

#[async_trait]
impl EventHandler for Bot {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);

        let commands = GuildId::set_application_commands(
            &self.guild_id,
            &ctx.http,
            |commands| {
                commands
                    .create_application_command(|command| {
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
                    .create_application_command(|command| {
                        command
                            .name(CMD_SESSION)
                            .description("Enter a Lisp session")
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
                CMD_SESSION => {
                    let name =
                        Generator::with_naming(Name::Plain).next().unwrap();
                    let send_followup =
                        command.channel_id.say(&ctx.http, "Yay, let's code");
                    match send_followup.await {
                        Err(why) => {
                            error!(
                                "Failed to send thread follow-up message: {}",
                                why
                            );
                            "Can't create thread : (".to_owned()
                        },
                        Ok(follow_up) => {
                            let create_thread =
                                command.channel_id.create_public_thread(
                                    &ctx.http,
                                    follow_up.id,
                                    |thread| thread.name(&name),
                                );
                            match create_thread.await {
                                Err(why) => {
                                    error!("Failed to create thread:  {}", why);
                                    "Can't create thread : (".to_owned()
                                },
                                Ok(_channel) => {
                                    "Creating a thread for you :D".to_owned()
                                },
                            }
                        },
                    }
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

const CMD_EVAL: &str = "eval";
const CMD_EVAL_SEXPR: &str = "sexpr";

const CMD_SESSION: &str = "session";

use names::{Generator, Name};
use serenity::async_trait;
use serenity::model::application::interaction::{
    Interaction,
    InteractionResponseType,
};
use serenity::model::gateway::Ready;
use serenity::model::prelude::command::CommandOptionType;
use serenity::model::prelude::GuildId;
use serenity::prelude::*;
use shuttle_runtime::CustomError;
use sqlx::{Executor, PgPool};
use tracing::{error, info};

use crate::eval::eval;
