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

Write-style operations are the enforcement point. The first implemented guard is
app creation. If a workspace exceeds the `apps` resource limit, Peanut returns:

```json
{
  "code": "resource_limit_exceeded",
  "resource_key": "apps",
  "used": 3,
  "limit": 3
}
```

Read operations remain available so operators can inspect and recover.

## Overrides

Instance admins can adjust one resource limit for a workspace:

```bash
curl -s -X POST "$BASE_URL/api/workspaces/$WORKSPACE_ID/resource-limits" \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data '{"resource_key":"apps","limit":10}'
```
