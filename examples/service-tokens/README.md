# Peanut service token curl examples

This folder contains copy-pasteable curl examples for Peanut admin service tokens.

Recommended flow:
1. create an admin JWT first through the normal auth flow
2. create a service token with that admin JWT
3. save the returned plaintext `pst_...` token somewhere safe
4. use the service token as Bearer auth on protected admin routes
5. revoke the token when the automation is no longer needed

The examples default to direct local development on `http://127.0.0.1:3000`.
Use `http://127.0.0.1:3492` when talking to the default Docker Compose package.

Files included:
- `create-token.json`
- `create-token.sh`
- `create-token-jq.sh`
- `list-tokens.sh`
- `use-token-admin-users.sh`
- `revoke-token.sh`
- `revoke-latest-token-jq.sh`
- for a combined Data API + storage operator flow, see `../operations-e2e/`
- for runnable cron/operator scripts, see `../automation/`
- for cron/operator automation guidance, see `../../docs/automation-runbook.md`

Minimal flow:

```bash
export BASE_URL=http://127.0.0.1:3000
export ADMIN_JWT='<PASTE_ADMIN_JWT>'

./examples/service-tokens/create-token.sh
# copy `token` from the JSON response

export SERVICE_TOKEN='***'
./examples/service-tokens/use-token-admin-users.sh
```

If `jq` is installed, you can skip manual copying:

```bash
export BASE_URL=http://127.0.0.1:3000
export ADMIN_JWT='<PASTE_ADMIN_JWT>'

./examples/service-tokens/create-token-jq.sh
# copy/paste the printed export lines
```

For machine-local secret storage, copy `../automation/peanut.env.sample` and paste the plaintext service token there.
