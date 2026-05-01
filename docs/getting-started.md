# Peanut Getting Started

Peanut ships as a Rust single binary with an embedded admin console. The first
operator bootstraps a platform admin, creates beta invites, and lets invited
teams create organizations.

## Bootstrap

```bash
curl -s -X POST "$BASE_URL/api/bootstrap/admin" \
  -H "content-type: application/json" \
  --data '{"email":"owner@example.com","password":"password123"}'
```

Open the console at `/`, sign in, and use the language switcher to choose
English or Korean. The choice is stored in browser local storage.

## Invite a Beta Organization

```bash
curl -s -X POST "$BASE_URL/api/admin/beta-invites" \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data '{"label":"pilot","max_uses":1}'
```

Share the returned `invite_code` with the organization owner. They can create an
organization through `POST /api/beta/signup`.

## Create an App

Use the console or call:

```bash
curl -s -X POST "$BASE_URL/api/apps" \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data '{"organization_id":"default","name":"mobile-prod","display_name":"Mobile Prod"}'
```

App creation consumes the organization's `apps` quota.
