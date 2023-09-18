# Liz: collaborative Lisp coding on Discord

![Computing Fibonacci numbers using Liz](.assets/liz-fib.png)

Liz is a Discord bot that provides you with a basic Lisp REPL with collaborative editing capabilities. It's run using [Serenity](https://github.com/serenity-rs/serenity) and [Shuttle](https://github.com/shuttle-hq/shuttle), which means you can easily get your own instance up and running in a minute.

## 🦾Commands

* `/eval` takes a single S-expression as input and evaluates it in a fresh environment. Alternatively, when used inside an active session, this command evaluates all the code in that session.

* `/lisp` creates a new Lisp session in a private thread that's only visible to the user who evoked the command. In a session, any message you send resembles a piece of Lisp code. Each message is appended to the end of the code. The code is evaluated automatically once all parentheses are balanced. In the output, comments indicate which expression yielded which values. Text that was `print`ed during the evaluation is displayed without a leading comment.

* `/collab` invites the given user or all uses with the given role to join you in your coding session. Now they can see what you are writing, and they are allowed to make edits and evaluate the code themselves. By inviting people to a session, you allow them to invite others, too.

* `/del` without an additional argument, deletes the last line of code in the session. You can also specify the index of the line to delete. Lines are indexed in reverse, starting at 0. That is, the last line you entered has the index 0, the one before that has the index 1, and so on.


In a Lisp session, any message you write is interpreted as code. This means that if you want to write a 'normal' message, you need to make it a comment by starting it with `;;`. If you want, you can use single back-tics so that your text is rendered using a mono space font. You can also enclose the code you write in triple back-tics, and you're allowed to specify `lisp` as the language that's used.

## 🚀 Deployment

> The following deployment strategy will work if you didn't make any changes to the code in this repository. If you did, please refer to the development section to make sure that your deployment compiles.

Liz is built using Shuttle. To deploy it, you need to have [`cargo-shuttle` set up correctly](https://docs.shuttle.rs/getting-started/installation).

Deploying on Shuttle is super easy. First, you have to set up a new bot application on Discord and generate a token for it. My instance uses the following permission integer, which allows Liz to use most text chat capabilities: `534723946560`. Additionally, you need the guild (server) ID of the server that you intend to use Liz on.

Once you have both of them, store them in a file in this repository's root called `Secrets.toml`:

``` toml
DISCORD_TOKEN = 'Your Discord token'
DISCORD_GUILDID = 'Your guild ID'
```

Be careful to not leak your application token. For example, don't check `Secrets.toml` into source control.

Now, run the following two commands in this repository's root.

``` sh
cargo shuttle project start
cargo shuttle deploy
```

## 🛠️ Development

For development purposes, you need to provide Liz with a Postgres database connection of your own. If you have [Docker](https://docs.docker.com/desktop/), [psql](https://www.postgresql.org/docs/current/app-psql.html) and [SQLx](https://crates.io/crates/sqlx-cli) installed, the script `scripts/init_postgres.sh` can do this for you automatically.

Assuming you didn't change the default password used in the script (which is `password`), the following commands should allow you to run Liz locally.

``` sh
echo "POSTGRES_PASSWORD = 'password'" >> Secrets.toml
echo "DATABASE_URL=\"postgres://postgres:password@localhost:5432/sessions\"" >> .env
chmod +x scripts/init_postgres.sh
./scripts/init_postgres.sh
```

Lastly, compile and run Liz using `cargo shuttle run`.

If you have any issues, feel free to [reach out](mailto:d4kd@proton.me) or [open an issue](https://github.com/d4ckard/liz/issues/new).

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
