#!/usr/bin/env bash

set -x
set -eo pipefail

if ! [ -x "$(command -v psql)" ]; then
	echo >&2 "Error: psql is not installed."
	exit 1
fi

if ! [ -x "$(command -v sqlx)" ]; then
	echo >&2 "Error: sqlx is not installed."
	echo >&2 "Use:"
	echo >&2 " cargo install --version='~0.6' sqlx-cli \
--no-default-features --features rustls,postgres"
	echo >&2 "to install it."
	exit 1
fi

DB_USER="${POSTGRES_USER:=postgres}"  # Custom user of default to 'postgres'
DB_PASSWORD="${POSTGRES_PASSWORD:=password}"  # Custom password or default to 'password'
DB_NAME="${POSTGRES_DB:=sessions}"  # Custom name of default to 'sessions'
DB_PORT="${POSTGRES_PORT:=5432}"  # Custom port or default to '5432'

# Skip docker if dockerized Postgres DB is already running
if [[ -z "${SKIP_DOCKER}" ]]
then
	docker run \
		-e POSTGRES_USER=${DB_USER} \
		-e POSTGRES_PASSWORD=${DB_PASSWORD} \
		-e POSTGRES_DB=${DB_NAME} \
		-p "${DB_PORT}":5432 \
		-d postgres \
		postgres -N 1000
	#   ^ increase max number of connections
fi

export PGPASSWORD="${DB_PASSWORD}"
until psql -h "localhost" -U "${DB_USER}" -p "${DB_PORT}" -d "postgres" -c '\q'; do
	>&2 echo "Postgres is still unavailable - sleeping"
	sleep 1
done

>&2 echo "Postgres is up and running on port ${DB_PORT} - running migrations now!"

# To migrate without tearing down and re-creating an existing Postgres instance run:
# `SKIP_DOCKER=true ./scripts/init_postgres.sh` 

# DATABASE_URL doesn't need to be exported because it's found in .env
# DATABASE_URL=postgres://${DB_USER}:${DB_PASSWORD}@localhost:${DB_PORT}/${DB_NAME}
# export DATABASE_URL
sqlx database create
sqlx migrate run

>&2 echo "Postgres has been migrated, ready to go!"
