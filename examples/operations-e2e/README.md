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
- upload an object into Peanut storage

Files included:
- `create-service-token.sh`
- `bootstrap-service-token-jq.sh`
- `create-todos-table.sh`
- `create-todo-row.sh`
- `upload-storage-object.sh`
- `head-storage-object.sh`
- `todos-table.json`
- `todo-row.json`
- `sample-object.txt`

Environment variables used:
- `BASE_URL` default: `http://127.0.0.1:3000`
- `ADMIN_JWT` required for service-token creation
- `SERVICE_TOKEN` required for Data API + storage steps
- `STORAGE_BUCKET` optional, default `assets`
- `STORAGE_KEY` optional, default `ops/hello.txt`

Minimal flow:

```bash
export BASE_URL=http://127.0.0.1:3000
export ADMIN_JWT='<PASTE_ADMIN_JWT>'

./examples/operations-e2e/create-service-token.sh
# copy `token` from the JSON response

export SERVICE_TOKEN='pst_...'
./examples/operations-e2e/create-todos-table.sh
./examples/operations-e2e/create-todo-row.sh
./examples/operations-e2e/upload-storage-object.sh
./examples/operations-e2e/head-storage-object.sh
```

If `jq` is installed, you can bootstrap the service token and print the next commands automatically:

```bash
export BASE_URL=http://127.0.0.1:3000
export ADMIN_JWT='<PASTE_ADMIN_JWT>'

./examples/operations-e2e/bootstrap-service-token-jq.sh
```

Notes:
- these examples intentionally use explicit placeholders instead of shell JSON parsing tricks
- the service token acts as the backing admin user on protected APIs
- the storage object still lands in the current authenticated user's scoped storage namespace
