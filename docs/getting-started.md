# Peanut Getting Started

Peanut ships as a Rust single binary with an embedded admin console. The first
operator bootstraps an instance admin, creates workspace setup invites, and lets
internal teams create isolated workspaces.

## Bootstrap

```bash
curl -s -X POST "$BASE_URL/api/bootstrap/admin" \
  -H "content-type: application/json" \
  --data '{"email":"owner@example.com","password":"password123"}'
```

Open the console at `/`, sign in, and use the language switcher to choose
English or Korean. The choice is stored in browser local storage.

## Invite a Workspace

```bash
curl -s -X POST "$BASE_URL/api/admin/workspace-invites" \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data '{"label":"mobile team setup","max_uses":1}'
```

Share the returned `invite_code` with the workspace owner. They can accept it
through `POST /api/workspace-invites/accept`.

## Create an App

Use the console or call:

```bash
curl -s -X POST "$BASE_URL/api/apps" \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data '{"workspace_id":"default","name":"mobile-prod","display_name":"Mobile Prod"}'
```

App creation consumes the workspace's `apps` resource limit.
