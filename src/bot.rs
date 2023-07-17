pub struct Bot {
    guild_id: GuildId,
    db: PgPool,
}

impl Bot {
    pub fn new(db: PgPool, guild_id: GuildId) -> Result<Self, CustomError> {
        Ok(Bot { db, guild_id })
    }

    #[tracing::instrument(name = "Store new session", skip(self), err)]
    async fn store_session(
        &self,
        thread_id: ChannelId,
        user_id: UserId,
    ) -> Result<(), anyhow::Error> {
        sqlx::query!(
            r#"
            INSERT INTO sessions (thread_id, user_ids, source_code)
            VALUES ($1, $2, $3)
            "#,
            thread_id.to_string(),
            &vec![user_id.to_string()],
            String::new(),
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    #[tracing::instrument(name = "Update existing session", skip(self), err)]
    async fn update_session(
        &self,
        thread_id: ChannelId,
        session: UserSession,
    ) -> Result<(), anyhow::Error> {
        sqlx::query!(
            r#"
            UPDATE sessions
            SET
                source_code = $3,
                user_ids = $2
            WHERE
                thread_id = $1
            "#,
            thread_id.to_string(),
            &session
                .user_ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<String>>(),
            session.source_code.as_ref()
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    #[tracing::instrument(
        name = "Run an updating operation the a session",
        skip(self, update_callback),
        err
    )]
    async fn run_session_update<U>(
        &self,
        thread_id: ChannelId,
        caller: UserId,
        update_callback: U,
    ) -> Result<String, OpError>
    where
        U: FnOnce(&mut UserSession) -> Result<String, anyhow::Error>,
    {
        if let Ok(mut session) = self.get_session(thread_id).await {
            if session.user_ids.contains(&caller) {
                match update_callback(&mut session) {
                    Ok(msg) => {
                        match self.update_session(thread_id, session).await {
                            Ok(()) => Ok(msg),
                            Err(e) => {
                                error!(
                                    "Session update failed {}, callback \
                                     message {}",
                                    e, msg
                                );
                                Err(OpError::Update(e))
                            },
                        }
                    },
                    Err(e) => {
                        error!("Operation failed '{}'", e);
                        Err(e.into())
                    },
                }
            } else {
                Err(OpError::NotAllowed)
            }
        } else {
            Err(OpError::NotFound(thread_id))
        }
    }

    // NOTE: Thread ID and channel ID may be used
    // interchangeably.

    #[tracing::instrument(name = "Get session by thread ID", skip(self))]
    async fn get_session(
        &self,
        thread_id: ChannelId,
    ) -> Result<UserSession, anyhow::Error> {
        struct UserSessionStrings {
            user_ids: Vec<String>,
            source_code: String,
        }
        let session = sqlx::query_as!(
            UserSessionStrings,
            r#"
            SELECT user_ids, source_code
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
                .user_ids
                .iter()
                .map(|id| {
                    id.parse::<u64>()
                        .map(|id| UserId::from(id))
                        .expect("Invalid data in db")
                })
                .collect::<Vec<UserId>>(),
            session.source_code,
        ))
    }

    async fn create_session_thread(
        &self,
        ctx: &Context,
        orig_channel: ChannelId,
        user_id: UserId,
    ) -> String {
        // Generate a random two-word name.
        let name = Generator::with_naming(Name::Plain).next().unwrap();

        let create_thread = orig_channel
            .create_private_thread(&ctx.http, |thread| thread.name(&name));
        let thread = match create_thread.await {
            Err(err) => {
                error!("Failed to create thread: {}", err);
                return "Can't start new session :(".to_owned();
            },
            Ok(thread) => thread,
        };

        let rollback_thread_creation = || async move {
            let delete_thread = ctx.http.delete_channel(thread.id.0);
            if let Err(err) = delete_thread.await {
                error!(
                    "Failed to cleanup failed session creation {}: {}",
                    thread.id, err
                );
            }
            "Failed to create session :(".to_owned()
        };

        let send_message = thread.send_message(&ctx.http, |message| {
            message.content(format!(
                "Here you go {}, let's code!",
                user_id.mention()
            ))
        });
        if let Err(err) = send_message.await {
            error!("Failed to send initial message to thread: {}", err);
            return rollback_thread_creation().await;
        }

        let store = self.store_session(thread.id, user_id);
        match store.await {
            Err(err) => {
                error!("Failed to store session: {}", err);
                return rollback_thread_creation().await;
            },
            Ok(()) => "Started a new session for you : D".to_owned(),
        }
    }

    async fn eval_user_input(
        &self,
        orig_channel: ChannelId,
        options: &Vec<CommandDataOption>,
    ) -> anyhow::Result<String> {
        match self.get_session(orig_channel).await {
            Err(_) => {
                // There is no session for this thread, so `/eval`
                // will try to execute the  given code.
                let option = options
                    .iter()
                    .find(|opt| opt.name == CMD_EVAL_SEXPR)
                    .cloned()
                    .ok_or(anyhow!("Failed to find correct option"))?
                    .value
                    .ok_or(anyhow!("Missing option content"))?;
                let input = option
                    .as_str()
                    .ok_or(anyhow!("Failed to get inner string"))?;
                let code = UserCode::new(input);
                Ok(code.respond())
            },
            Ok(session) => Ok(session.source_code.respond()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum OpError {
    #[error("Operaton failed, '{0}'")]
    Callback(#[from] anyhow::Error),
    #[error("Failed to update session with result")]
    Update(#[source] anyhow::Error),
    #[error("No session with thread ID {0}")]
    NotFound(ChannelId),
    #[error("Caller was not allowed to work on the session")]
    NotAllowed,
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
                            .description("Evaluate Lisp code")
                            .create_option(|option| {
                                option
                                    .name(CMD_EVAL_SEXPR)
                                    .description("S-expression to evaluate")
                                    .kind(CommandOptionType::String)
                                    .required(false)
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
                    .create_application_command(|command| {
                        command
                            .name(CMD_INVITE)
                            .description(
                                "Invite someone to work on a session with you",
                            )
                            .create_option(|option| {
                                option
                                    .name(CMD_INVITE_WHO)
                                    .description(
                                        "The person you want to invite",
                                    )
                                    .kind(CommandOptionType::User)
                                    .required(true)
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
            let run_op =
                self.run_session_update(thread_id, msg.author.id, |session| {
                    session.source_code.append(&msg.content);
                    Ok(session.source_code.respond())
                });

            let response = match run_op.await {
                Ok(msg) => msg,
                Err(op_err) => match op_err {
                    OpError::Update(_) => "Sorry, I failed to update your \
                                           code. Maybe try again."
                        .to_owned(),
                    OpError::NotAllowed => {
                        format!(
                            "Hey {}! You are not allowed edit here.",
                            msg.author.id.mention()
                        )
                    },
                    // Don't react to messages in non-session channels.
                    OpError::NotFound(_) => return,
                    OpError::Callback(_) => {
                        unreachable!("Callback doesn't return any errors")
                    },
                },
            };

            if let Err(e) = thread_id.say(&ctx.http, response).await {
                error!("Failed to respond with new code, {}", e);
            }
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            let response_content = match command.data.name.as_str() {
                CMD_EVAL => {
                    let options = &command.data.options;
                    let eval_input =
                        self.eval_user_input(command.channel_id, options);
                    match eval_input.await {
                        Err(err) => {
                            error!("Failed to evaluate user input: {}", err);
                            format!("I need input outside of a session (looking at you {})",
                            command.user.id.mention())
                        },
                        Ok(response) => response,
                    }
                },
                CMD_SESSION => {
                    self.create_session_thread(
                        &ctx,
                        command.channel_id,
                        command.user.id,
                    )
                    .await
                },
                CMD_DEL => {
                    let thread_id = command.channel_id;
                    let run_op = self.run_session_update(
                        thread_id,
                        command.user.id,
                        |session| {
                            let default = CommandDataOptionValue::Integer(0);
                            let del_idx = command
                                .data
                                .options
                                .iter()
                                .find(|opt| opt.name == CMD_DEL_IDX)
                                .map_or(default.clone(), |opt| {
                                    // Get the option's inner value.
                                    opt.resolved.clone().unwrap_or(default)
                                });
                            if let CommandDataOptionValue::Integer(idx) =
                                del_idx
                            {
                                match session.source_code.del(idx) {
                                    Some(deleted) => {
                                        Ok(format!("Deleted `{}`", deleted))
                                    },
                                    None => Ok("Nothing to delete".to_owned()),
                                }
                            } else {
                                Ok("This commands requires and integer to \
                                    function."
                                    .to_owned())
                            }
                        },
                    );

                    match run_op.await {
                        Ok(msg) => msg,
                        Err(op_err) => match op_err {
                            OpError::Callback(_) => {
                                INVALID_REQUEST_MSG.to_owned()
                            },
                            OpError::NotFound(_) => "You can't deleting \
                                                     things outside of a \
                                                     session thread."
                                .to_owned(),
                            OpError::Update(_) => {
                                "Failed to execute deletion".to_owned()
                            },
                            OpError::NotAllowed => format!(
                                "Hey {}! You are not allowed \
                                                    to delete stuff here.",
                                command.user.id.mention()
                            )
                            .to_owned(),
                        },
                    }
                },
                CMD_INVITE => {
                    let thread_id = command.channel_id;
                    let run_op = self.run_session_update(
                        thread_id,
                        command.user.id,
                        |session| {
                            let other = command
                                .data
                                .options
                                .iter()
                                .find(|opt| opt.name == CMD_INVITE_WHO)
                                .cloned();
                            let other = other
                                .ok_or(anyhow!("No user other specified"))?
                                .resolved
                                .ok_or(anyhow!("No resolved user"))?;
                            if let CommandDataOptionValue::User(user, _) = other
                            {
                                session.user_ids.push(user.id);
                                Ok(format!(
                                    "{} is now part of this session",
                                    user.id.mention()
                                ))
                            } else {
                                Ok("This command requires a user to function."
                                    .to_owned())
                            }
                        },
                    );
                    match run_op.await {
                        Ok(msg) => msg,
                        Err(op_err) => match op_err {
                            OpError::Callback(_) => {
                                INVALID_REQUEST_MSG.to_owned()
                            },
                            OpError::NotFound(_) => "You can't invite someone \
                                                     if you aren't in a \
                                                     session."
                                .to_owned(),
                            OpError::Update(_) => {
                                "Failed to store invite".to_owned()
                            },
                            OpError::NotAllowed => format!(
                                "Hey {}! This is not your own \
                                                    thread, you can't invite \
                                                    people.",
                                command.user.id.mention()
                            )
                            .to_owned(),
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

#[derive(Debug)]
struct UserSession {
    user_ids: Vec<UserId>,
    source_code: UserCode,
}

impl UserSession {
    fn new(user_ids: Vec<UserId>, source_code: String) -> Self {
        Self {
            user_ids,
            source_code: UserCode::new(source_code),
        }
    }
}

const CMD_EVAL: &str = "eval";
const CMD_EVAL_SEXPR: &str = "sexpr";
const CMD_SESSION: &str = "session";
const CMD_DEL: &str = "del";
const CMD_DEL_IDX: &str = "index";
const CMD_INVITE: &str = "invite";
const CMD_INVITE_WHO: &str = "who";

const INVALID_REQUEST_MSG: &str =
    "I received an invalid request. Maybe try again.";

use anyhow::anyhow;
use names::{Generator, Name};
use serenity::async_trait;
use serenity::model::application::command::CommandOptionType;
#[rustfmt::skip]
use serenity::model::prelude::interaction::application_command::{
    CommandDataOption,
    CommandDataOptionValue,
};
use serenity::model::application::interaction::{
    Interaction, InteractionResponseType,
};
use serenity::model::channel::{Message, MessageType};
use serenity::model::gateway::Ready;
use serenity::model::id::{ChannelId, GuildId, UserId};
use serenity::prelude::*;
use shuttle_runtime::CustomError;
use sqlx::PgPool;
use tracing::{error, info};

use crate::eval::UserCode;
