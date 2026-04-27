# Peanut end-to-end operations example

This folder ties together three operator flows:
- admin auth bootstrap
- service token creation and reuse
- Data API + storage operations with the resulting service token

Goal:
- start from an admin JWT
- mint one admin service token
- use that service token to create a Data API table
- insert a row into the table
- list rows back through the Data API
- export the table snapshot
- read the latest row-event checkpoint
- replay only newer row events from a durable event id
- import a known-good table snapshot back into the same table
- upload an object into Peanut storage

Files included:
- `create-service-token.sh`
- `bootstrap-service-token-jq.sh`
- `create-todos-table.sh`
- `create-todo-row.sh`
- `list-todo-rows.sh`
- `export-todos-table.sh`
- `get-events-checkpoint.sh`
- `replay-todo-events.sh`
- `import-todos-table.sh`
- `upload-storage-object.sh`
- `head-storage-object.sh`
- `todos-table.json`
- `todo-row.json`
- `todos-import-replace.json`
- `sample-object.txt`

Environment variables used:
- `BASE_URL` default: `http://127.0.0.1:3000`
- `ADMIN_JWT` required for service-token creation
- `SERVICE_TOKEN` required for Data API + storage steps
- `TABLE_NAME` optional, default `ops_todos`
- `LAST_EVENT_ID` required only for replay, use the latest known checkpoint/event id
- `EVENT_LIMIT` optional for replay, default `50`
- `IMPORT_FILE` optional for import, default `examples/operations-e2e/todos-import-replace.json`
- `STORAGE_BUCKET` optional, default `assets`
- `STORAGE_KEY` optional, default `ops/hello.txt`

Minimal flow:

```bash
export BASE_URL=http://127.0.0.1:3000
export ADMIN_JWT='<PASTE_ADMIN_JWT>'

./examples/operations-e2e/create-service-token.sh
# copy `token` from the JSON response

export SERVICE_TOKEN='***'
./examples/operations-e2e/create-todos-table.sh
./examples/operations-e2e/create-todo-row.sh
./examples/operations-e2e/list-todo-rows.sh
./examples/operations-e2e/export-todos-table.sh
./examples/operations-e2e/get-events-checkpoint.sh
export LAST_EVENT_ID=1
./examples/operations-e2e/replay-todo-events.sh
./examples/operations-e2e/import-todos-table.sh
./examples/operations-e2e/upload-storage-object.sh
./examples/operations-e2e/head-storage-object.sh
```

If `jq` is installed, you can bootstrap the service token and print the next commands automatically:

```bash
export BASE_URL=http://127.0.0.1:3000
export ADMIN_JWT='<PASTE_ADMIN_JWT>'

./examples/operations-e2e/bootstrap-service-token-jq.sh
```

Practical event-sync pattern:
1. run `get-events-checkpoint.sh`
2. persist that `latest_event_id` in your worker/job state
3. after more writes happen, export `LAST_EVENT_ID=<saved id>`
4. run `replay-todo-events.sh` to fetch only newer mutations
5. update your saved event id

Notes:
- if you want to turn this into cron/systemd/CI automation, see `../automation/` and `../../docs/automation-runbook.md`
- for machine-local secret storage, start from `../automation/peanut.env.sample`
- these examples intentionally use explicit placeholders instead of shell JSON parsing tricks
- the service token acts as the backing admin user on protected APIs
- the storage object still lands in the current authenticated user's scoped storage namespace
