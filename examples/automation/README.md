# Peanut automation examples

This folder turns the service-token and operations examples into reusable cron/systemd/CI-style scripts.

Recommended rollout:
1. create an admin JWT through the normal auth flow
2. mint a dedicated service token with `../service-tokens/create-token.sh` or `../service-tokens/create-token-jq.sh`
3. copy `peanut.env.sample` to a machine-local env file such as `/opt/peanut/peanut.env`
4. paste the plaintext `pst_...` token into that env file
5. run `../operations-e2e/` once to validate Data API + storage access end to end
6. schedule one of the scripts below from cron/systemd/CI

Files included:
- `peanut.env.sample`
- `export-ops-todos.sh`
- `check-storage-head.sh`

Example:

```bash
cp examples/automation/peanut.env.sample /opt/peanut/peanut.env
$EDITOR /opt/peanut/peanut.env

PEANUT_ENV_FILE=/opt/peanut/peanut.env \
  ./examples/automation/export-ops-todos.sh
```

Notes:
- the env file is intentionally not committed with real secrets
- `peanut.env.sample` targets the default Docker Compose host port, `3492`; use `3000` for direct local `cargo run`
- automation API calls are app-scoped; set `APP_ID` when you are not using the default app
- prefer one service token per automation purpose
- rotate tokens by minting a new one, updating the env file, verifying one run, then revoking the old token
- for the broader guidance, see `../../docs/automation-runbook.md`
