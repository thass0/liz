pub struct Bot {
    guild_id: GuildId,
    db:       PgPool,
}

impl Bot {
    pub fn new(db: PgPool, guild_id: GuildId) -> Result<Self, CustomError> {
        Ok(Bot { db, guild_id })
    }

    #[tracing::instrument(name = "Store new session", skip(self))]
    async fn store_session(
        &self,
        thread_id: ChannelId,
        user_id: UserId,
    ) -> Result<(), anyhow::Error> {
        sqlx::query!(
            r#"
            INSERT INTO sessions (thread_id, user_id, source_code)
            VALUES ($1, $2, $3)
            "#,
            thread_id.to_string(),
            user_id.to_string(),
            String::new(),
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    #[tracing::instrument(name = "Update existing session", skip(self))]
    async fn update_session(
        &self,
        thread_id: ChannelId,
        session: UserSession,
    ) -> Result<(), anyhow::Error> {
        sqlx::query!(
            r#"
            UPDATE sessions SET source_code = $3
            WHERE
                thread_id = $1 AND
                user_id = $2
            "#,
            thread_id.to_string(),
            session.user_id.to_string(),
            session.source_code.as_ref()
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    // NOTE: Thread ID and channel ID may be used
    // interchangeably.

    #[tracing::instrument(name = "Get session by thread ID", skip(self))]
    async fn get_session(
        &self,
        thread_id: ChannelId,
    ) -> Result<UserSession, anyhow::Error> {
        struct UserSessionStrings {
            user_id:     String,
            source_code: String,
        }
        let session = sqlx::query_as!(
            UserSessionStrings,
            r#"
            SELECT user_id, source_code
            FROM sessions
            WHERE
                thread_id = $1
            "#,
            thread_id.to_string()
        )
        .fetch_one(&self.db)
        .await?;

        Ok(UserSession::new(
            session
                .user_id
                .parse::<u64>()
                .map(|id| UserId::from(id))
                .expect("Invalid data in db"),
            session.source_code,
        ))
    }
}

#[async_trait]
impl EventHandler for Bot {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);

        GuildId::set_application_commands(
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
                            .description("Start a Lisp coding session")
                    })
                    .create_application_command(|command| {
                        command
                            .name(CMD_DEL)
                            .description(
                                "Delete an S-expression in this session",
                            )
                            .create_option(|option| {
                                option
                                    .name(CMD_DEL_IDX)
                                    .description(
                                        "Index of expression to delete (last \
                                         one starts at index 0)",
                                    )
                                    .kind(CommandOptionType::Integer)
                                    .required(false)
                            })
                    })
            },
        )
        .await
        .unwrap();
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.kind == MessageType::Regular && !msg.author.bot {
            let thread_id = msg.channel_id;
            if let Ok(mut session) = self.get_session(thread_id).await {
                session.source_code.append(msg.content);
                let mut response = session.source_code.to_string();

                // Evaluate once the code is valid.
                if let Balanced::Yes = session.source_code.balance() {
                    response.push_str("```");
                    response.push_str(&session.source_code.eval());
                    response.push_str("\n```");
                }

                thread_id.say(&ctx.http, response).await.unwrap();

                self.update_session(thread_id, session).await.unwrap();
            }
        }
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
                    format!("`{}`\n{}", sexpr_str, eval_single(sexpr_str))
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
                                Ok(channel) => {
                                    let store = self.store_session(
                                        channel.id,
                                        command.user.id,
                                    );
                                    match store.await {
                                        Ok(()) => "Creating a thread for you \
                                                   :D"
                                        .to_owned(),
                                        Err(_) => {
                                            let delete_channel = ctx
                                                .http
                                                .delete_channel(channel.id.0);
                                            if delete_channel.await.is_err() {
                                                "Failed to create valid \
                                                 session : (. You can remove \
                                                 the thread."
                                                    .to_owned()
                                            } else {
                                                "Failed to create thread : ("
                                                    .to_owned()
                                            }
                                        },
                                    }
                                },
                            }
                        },
                    }
                },
                CMD_DEL => {
                    let thread_id = command.channel_id;
                    if let Ok(mut session) = self.get_session(thread_id).await {
                        if session.user_id == command.user.id {
                            let default = CommandDataOptionValue::Integer(0);
                            let del_idx = command
                                .data
                                .options
                                .iter()
                                .find(|opt| opt.name == CMD_DEL_IDX)
                                .map_or(default.clone(), |opt| {
                                    // Get the option's inner value.
                                    opt.resolved.clone().unwrap_or(
                                        default,
                                    )
                                });
                            if let CommandDataOptionValue::Integer(idx) = del_idx {
                                match session.source_code.del(idx) {
                                    Some(deleted) =>
                                        match self.update_session(thread_id, session).await {
                                            Ok(()) => format!("Deleted `{}`", deleted),
                                            Err(_) => format!("Failed to deleted `{}`", deleted),
                                        },
                                    None => "Nothing to delete".to_owned(),
                                }
                            } else {
                                "This commands requires and integer to function.".to_owned()
                            }
                        } else {
                            "Hey! You are not allowed to delete stuff here."
                                .to_owned()
                        }
                    } else {
                        "You can't deleting things outside of a session thread."
                            .to_owned()
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

#[derive(Debug)]
struct UserSession {
    user_id:     UserId,
    source_code: UserCode,
}

impl UserSession {
    fn new(user_id: UserId, source_code: String) -> Self {
        Self {
            user_id,
            source_code: UserCode::new(source_code),
        }
    }
}

const CMD_EVAL: &str = "eval";
const CMD_EVAL_SEXPR: &str = "sexpr";
const CMD_SESSION: &str = "session";
const CMD_DEL: &str = "del";
const CMD_DEL_IDX: &str = "index";

use names::{Generator, Name};
use serenity::async_trait;
use serenity::model::application::command::CommandOptionType;
use serenity::model::application::interaction::{
    Interaction,
    InteractionResponseType,
};
use serenity::model::channel::{Message, MessageType};
use serenity::model::gateway::Ready;
use serenity::model::id::{ChannelId, GuildId, UserId};
use serenity::prelude::*;
use serenity::model::prelude::prelude::interaction::application_command::CommandDataOptionValue;
use shuttle_runtime::CustomError;
use sqlx::PgPool;
use tracing::{error, info};

use crate::eval::{eval_single, Balanced, UserCode};
