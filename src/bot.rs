pub struct Bot {
    db: PgPool,
    guild_id: GuildId,
}

impl Bot {
    pub const fn new(db: PgPool, guild_id: GuildId) -> Self {
        Self { db, guild_id }
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

    #[tracing::instrument(name = "Update session code", skip(self), err)]
    async fn update_session_code(
        &self,
        thread_id: ChannelId,
        code: UserCode,
    ) -> Result<(), anyhow::Error> {
        sqlx::query!(
            r#"
            UPDATE sessions
            SET
                source_code = $2
            WHERE
                thread_id = $1
            "#,
            thread_id.to_string(),
            code.as_ref()
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    #[tracing::instrument(name = "Update session users", skip(self), err)]
    async fn update_session_users(
        &self,
        thread_id: ChannelId,
        user_ids: Vec<UserId>,
    ) -> Result<(), anyhow::Error> {
        sqlx::query!(
            r#"
            UPDATE sessions
            SET
                user_ids = $2
            WHERE
                thread_id = $1
            "#,
            thread_id.to_string(),
            &user_ids
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<String>>(),
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    #[tracing::instrument(
        name = "Run an updating operation the a session",
        skip(self, transform, update),
        err
    )]
    async fn run_session_update<S, U, Fut>(
        &self,
        thread_id: ChannelId,
        caller: UserId,
        transform: S,
        update: U,
    ) -> Result<String, OpError>
    where
        S: FnOnce(&mut UserSession) -> Result<String, anyhow::Error> + Send,
        U: FnOnce(ChannelId, UserSession) -> Fut + Send,
        Fut: Future<Output = Result<(), anyhow::Error>> + Send,
    {
        if let Ok(mut session) = self.get_session(thread_id).await {
            if session.user_ids.contains(&caller) {
                match transform(&mut session) {
                    Ok(msg) => match update(thread_id, session).await {
                        Ok(()) => Ok(msg),
                        Err(e) => {
                            error!(
                                "Session update failed {}, callback \
                                     message {}",
                                e, msg
                            );
                            Err(OpError::Update(e))
                        },
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
                        .map(UserId::from)
                        .expect("Invalid data in db")
                })
                .collect::<Vec<UserId>>(),
            session.source_code,
        ))
    }

    async fn cmd_create_session_thread(
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
                rollback_thread_creation().await
            },
            Ok(()) => "Started a new session for you : D".to_owned(),
        }
    }

    async fn cmd_del_from_session(
        &self,
        thread_id: ChannelId,
        user_id: UserId,
        idx: i64,
    ) -> String {
        let run_op = self.run_session_update(
            thread_id,
            user_id,
            |session| match session.source_code.del(idx) {
                Some(deleted) => Ok(format!("Deleted `{deleted}`")),
                None => Ok("Nothing to delete".to_owned()),
            },
            |thread_id, new_session| {
                self.update_session_code(thread_id, new_session.source_code)
            },
        );

        match run_op.await {
            Ok(msg) => msg,
            Err(op_err) => match op_err {
                OpError::Callback(_) => INVALID_REQUEST_MSG.to_owned(),
                OpError::NotFound(_) => "You can't deleting \
                                         things outside of a \
                                         session thread."
                    .to_owned(),
                OpError::Update(_) => "Failed to execute deletion".to_owned(),
                OpError::NotAllowed => format!(
                    "Hey {}! You are not allowed \
                                        to delete stuff here.",
                    user_id.mention()
                ),
            },
        }
    }

    async fn cmd_invite_collaborator(
        &self,
        thread_id: ChannelId,
        user_id: UserId,
        invited_id: UserId,
    ) -> String {
        let run_op = self.run_session_update(
            thread_id,
            user_id,
            |session| {
                session.user_ids.push(invited_id);
                Ok(format!(
                    "{} is now part of this session",
                    invited_id.mention()
                ))
            },
            |thread_id, new_session| {
                self.update_session_users(thread_id, new_session.user_ids)
            },
        );

        match run_op.await {
            Ok(msg) => msg,
            Err(op_err) => match op_err {
                OpError::Callback(_) => INVALID_REQUEST_MSG.to_owned(),
                OpError::NotFound(_) => {
                    "You can't collaborate outside of a session.".to_owned()
                },
                OpError::Update(_) => "Failed to create invite".to_owned(),
                OpError::NotAllowed => format!(
                    "Hey {}! This is not your own \
                    session, you can't invite \
                    people.",
                    user_id.mention()
                ),
            },
        }
    }

    async fn eval_user_input(
        &self,
        orig_channel: ChannelId,
        options: &[CommandDataOption],
    ) -> anyhow::Result<String> {
        match self.get_session(orig_channel).await {
            Err(_) => {
                // There is no session for this thread, so `/eval`
                // will try to execute the  given code.
                let option = options
                    .iter()
                    .find(|opt| opt.name == CMD_EVAL_SEXPR)
                    .cloned()
                    .ok_or_else(|| anyhow!("Failed to find correct option"))?
                    .value
                    .ok_or_else(|| anyhow!("Missing option content"))?;
                let input = option
                    .as_str()
                    .ok_or_else(|| anyhow!("Failed to get inner string"))?;
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
                                        "Index of line to delete (last one starts at index 0)",
                                    )
                                    .kind(CommandOptionType::Integer)
                                    .required(false)
                            })
                    })
                    .create_application_command(|command| {
                        command
                            .name(CMD_COLLAB)
                            .description(
                                "Invite someone to collaborate on a session with you",
                            )
                            .create_option(|option| {
                                option
                                    .name(CMD_COLLAB_WHO)
                                    .description(
                                        "The person or role you want to invite",
                                    )
                                    .kind(CommandOptionType::Mentionable)
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
            let run_op = self.run_session_update(
                thread_id,
                msg.author.id,
                |session| {
                    session.source_code.append(&msg.content);
                    Ok(session.source_code.respond())
                },
                |thread_id, new_session| {
                    self.update_session_code(thread_id, new_session.source_code)
                },
            );

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

    #[allow(clippy::too_many_lines)]
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
                    self.cmd_create_session_thread(
                        &ctx,
                        command.channel_id,
                        command.user.id,
                    )
                    .await
                },
                CMD_DEL => {
                    let thread_id = command.channel_id;
                    let user_id = command.user.id;
                    let get_del_idx = || -> anyhow::Result<i64> {
                        let default = CommandDataOptionValue::Integer(0);
                        let idx = command
                            .data
                            .options
                            .iter()
                            .find(|opt| opt.name == CMD_DEL_IDX)
                            .map_or(&default, |opt| {
                                // Get the option's inner value.
                                opt.resolved.as_ref().unwrap_or(&default)
                            });
                        let CommandDataOptionValue::Integer(idx) = idx else {
                            return Err(anyhow!("Wrong command data option value type"));
                        };
                        Ok(*idx)
                    };

                    match get_del_idx() {
                        Err(err) => {
                            error!(
                                "Failed to get `/del` command argument: {err}"
                            );
                            "You must specify which line to delete".to_owned()
                        },
                        Ok(idx) => {
                            self.cmd_del_from_session(thread_id, user_id, idx)
                                .await
                        },
                    }
                },
                CMD_COLLAB => {
                    let thread_id = command.channel_id;
                    let user_id = command.user.id;
                    let get_invited_id = || -> anyhow::Result<UserId> {
                        let other = command
                            .data
                            .options
                            .iter()
                            .find(|opt| opt.name == CMD_COLLAB_WHO);
                        let other = other
                            .ok_or_else(|| {
                                anyhow!("Missing command data option")
                            })?
                            .resolved
                            .as_ref()
                            .ok_or_else(|| anyhow!("Missing resolved value"))?;
                        let CommandDataOptionValue::User(user, _) = other else {
                            return Err(anyhow!("Wrong command data option value type"));
                        };
                        Ok(user.id)
                    };

                    match get_invited_id() {
                        Err(err) => {
                            error!(
                                "Failed to get `/collab` command argument: {err}"
                            );
                            "You must specify who to add to this session"
                                .to_owned()
                        },
                        Ok(invited_id) => {
                            self.cmd_invite_collaborator(
                                thread_id, user_id, invited_id,
                            )
                            .await
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
const CMD_SESSION: &str = "lisp";
const CMD_DEL: &str = "del";
const CMD_DEL_IDX: &str = "index";
const CMD_COLLAB: &str = "collab";
const CMD_COLLAB_WHO: &str = "who";

const INVALID_REQUEST_MSG: &str =
    "I received an invalid request. Maybe try again.";

use std::future::Future;

use anyhow::anyhow;
use names::{Generator, Name};
use serenity::async_trait;
use serenity::client::{Context, EventHandler};
use serenity::model::application::command::CommandOptionType;
use serenity::model::application::interaction::application_command::{
    CommandDataOption, CommandDataOptionValue,
};
use serenity::model::application::interaction::{
    Interaction, InteractionResponseType,
};
use serenity::model::channel::{Message, MessageType};
use serenity::model::gateway::Ready;
use serenity::model::id::{ChannelId, GuildId, UserId};
use serenity::model::mention::Mentionable;
use sqlx::PgPool;
use tracing::{error, info};

use crate::eval::UserCode;
