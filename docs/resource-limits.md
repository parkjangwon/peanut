# Peanut Resource Limits

Self-hosted workspaces are assigned the `self_hosted_default` limit profile by
default. These limits are guardrails for one Peanut instance, not billing plans.

## Default Limits

- Apps per workspace: 3
- App users per workspace: 10,000
- Data rows per workspace: 250,000
- Storage per workspace: 2GB
- Function invocations per month: 50,000
- Push sends per month: 50,000
- API requests per month: 1,000,000

## Enforcement

Write-style operations and SDK app-key requests are the enforcement points.
Peanut currently guards app creation, app-user registration, data row creation,
storage writes, Function invocations, Push sends, and monthly SDK API requests.
If a workspace exceeds a resource limit, Peanut returns:

```json
{
  "code": "resource_limit_exceeded",
  "resource_key": "apps",
  "used": 3,
  "limit": 3,
  "period_start": "all",
  "reset_at": null,
  "source": "count"
}
```

Monthly resources use a calendar-month `period_start` and expose `reset_at`.
Read operations remain available unless the API request quota itself is
exhausted, so operators can inspect and recover through admin routes.

## Overrides

Instance admins can adjust one resource limit for a workspace:

```bash
curl -s -X POST "$BASE_URL/api/workspaces/$WORKSPACE_ID/resource-limits" \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data '{"resource_key":"apps","limit":10}'
```
