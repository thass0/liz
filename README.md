# Liz: collaborative Lisp coding on Discord

![Computing Fibonacci numbers using Liz](.assets/liz-fib.png)

Liz is a Discord bot that provides you with a basic Lisp REPL with collaborative editing capabilities. It's run using [Serenity](https://github.com/serenity-rs/serenity) and [Shuttle](https://github.com/shuttle-hq/shuttle), which means you can easily get your own instance up and running in a minute.

You can try Liz right now, on the [Liz Playground](https://discord.gg/bdM34npb) Discord server or by [adding Liz to a server](https://discord.com/api/oauth2/authorize?client_id=1128674616483778661&permissions=534723950656&scope=bot) of your own.

## 🦾Commands

* `/eval` takes a single S-expression as input and evaluates it in a fresh environment. Alternatively, when used inside an active session, this command evaluates all the code in that session.

* `/lisp` creates a new Lisp session in a private thread that's only visible to the user who evoked the command. In a session, any message you send resembles a piece of Lisp code. Each message is appended to the end of the code. The code is evaluated automatically once all parentheses are balanced. In the output, comments indicate which expression yielded which values. Text that was `print`ed during the evaluation is displayed without a leading comment.

* `/collab` invites the given user or all uses with the given role to join you in your coding session. Now they can see what you are writing, and they are allowed to make edits and evaluate the code themselves. By inviting people to a session, you allow them to invite others, too.

* `/del` without an additional argument, deletes the last line of code in the session. You can also specify the index of the line to delete. Lines are indexed in reverse, starting at 0. That is, the last line you entered has the index 0, the one before that has the index 1, and so on.


In a Lisp session, any message you write is interpreted as code. This means that if you want to write a 'normal' message, you need to make it a comment by starting it with `;;`. If you want, you can use single back-tics so that your text is rendered using a mono space font. You can also enclose the code you write in triple back-tics, and you're allowed to specify `lisp` as the language that's used.

## 🚀 Deployment

> The following deployment strategy will work if you didn't make any changes to the code in this repository. If you did, please refer to the development section to make sure that your deployment compiles.

Liz is built using Shuttle. To deploy it, you need to have [`cargo-shuttle` set up correctly](https://docs.shuttle.rs/getting-started/installation).

Deploying on Shuttle is super easy. First, you have to set up a new bot application on Discord and generate a token for it (remember to set the right permissions). My instance uses the following permission integer, which allows Liz to use most text chat capabilities: `534723946560`.

Once you have your token, store it in a file in this repository's root called `Secrets.toml`:

``` toml
DISCORD_TOKEN = 'Your Discord token'
```

Be careful to not leak your application token. For example, don't check `Secrets.toml` into source control (this repository ignores it by default, don't worry).

Now, run the following two commands in this repository's root. You need to replace `my-liz` with some project name of your own.

``` sh
cargo shuttle project start --name my-liz
cargo shuttle deploy --name my-liz
```

Project names are unique on Shuttle. That's why you need to choose your own. Instead of using the `--name` flag with each command, you can also change the name of the project in `Cargo.toml`:

``` toml
[package]
name = "my-liz"
# ...
```

For a Discord bot like this, you should also consider setting `idle-minutes` to `0`. You can read about what this does for your [here](https://docs.shuttle.rs/getting-started/idle-projects).

## 🛠️ Development

For development purposes, you need to provide Liz with a Postgres database connection of your own. If you have [Docker](https://docs.docker.com/desktop/), [psql](https://www.postgresql.org/docs/current/app-psql.html) and [SQLx](https://crates.io/crates/sqlx-cli) installed, the script `scripts/init_postgres.sh` can do this for you automatically.

Assuming you didn't change the default password used in the script (which is `password`), the following commands should allow you to run Liz locally.

``` sh
echo "POSTGRES_PASSWORD = 'password'" >> Secrets.toml
echo "DATABASE_URL=\"postgres://postgres:password@localhost:5432/sessions\"" >> .env
chmod +x scripts/init_postgres.sh
./scripts/init_postgres.sh
```

Liz has two different modes of operations: one for release build where all application commands are set globally for your bot and one for development purposes which is limited in scope to a single server.

When running your bot locally with `cargo shuttle run`, the development (debug) build is used. To use the global build, you can pass the `--release` flag: `cargo shuttle run --release`. `cargo shuttle deploy` always uses the latter option.


 Additionally, you need the guild (server) ID of the server that you intend to use Liz on.

It is recommended that you use the single-server development mode, until you deploy your final changes. This mode requires you do add two more entries to your `Secrets.toml` file:

1. The guild (server) ID of the server than you want to test Liz on (`DISCORD_DEVEL_TOKEN`).
2. Another Discord application token (`DISCORD_GUILDID`).

Again, you can get this token by create a new bot application on Discord. You will only be able to use this development bot application on the serve whose guild ID you put in your `Secrets.toml`. Lastly, compile and run Liz using `cargo shuttle run`. The purpose of this division is that you can safely develop your bot, while other people use your running instance.

A full `Secrets.toml` file for development should look like this:

``` toml
DISCORD_TOKEN = 'The token for your main application. This is quite a long string'
POSTGRES_PASSWORD = 'password'
DISCORD_GUILDID = 'The guild ID of the server you want to test your bot on.'
DISCORD_DEVEL_TOKEN = 'The token of the bot used for development. This bot can only be used on the server with the guild ID above.'
```

If you have any issues, feel free to [reach out](mailto:thassilo.schulze@proton.me) or [open an issue](https://github.com/thass0/liz/issues/new).

### 🏗️ Building without a database

SQLx checks the validity of the SQL queries our code makes at compile time. This is why we need an active database connection to compile the code. However, we cannot provide such a connection when we deploy Liz in the cloud in Shuttle. To mitigate this issue, SQLx has the ability to store the information it needs from the active database in a file (called `sqlx-data.json`).

You can generate or update this file while you are connected to a database. The following script will do so. It might be clever to add this script to your local clone as a pre-push git hook. This way, all code you push to a remote will compile without an active database connection.

``` sh
set -eo pipefail
set -x

# Ensure that `sqlx-data.json` is not out of date.
cargo sqlx prepare --check
if [ $? -ne 0 ]; then
    echo "sqlx-data.json is out of date. Regenerating ..."
    cargo sqlx prepare
else
    echo "sqlx-data.json is up to date"
fi

exit 0
```

By default, SQLx will ignore the `sqlx-data.json` file if the `DATABASE_URL` environment variable is present. This will be the case if you added the `DATABASE_URL` to the `.env` file. To force SQLx to use the information in `sqlx-data.json` instead of trying to connect to a database, you can set the `SQLX_OFFLINE` environment variable to `true`.

For more information, please refer to the [SQLx documentation](https://crates.io/crates/sqlx-cli).
