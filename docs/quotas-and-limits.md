# Peanut Quotas and Limits

Public beta organizations are assigned the `beta_free` plan by default.

## Default Limits

- Apps per organization: 3
- App users per organization: 10,000
- Data rows per organization: 250,000
- Storage per organization: 2GB
- Function invocations per month: 50,000
- Push sends per month: 50,000
- API requests per month: 1,000,000

## Enforcement

Write-style operations are the enforcement point. The first implemented guard is
app creation: if an organization exceeds the `apps` quota, Peanut returns:

```json
{
  "code": "quota_exceeded",
  "quota_key": "apps",
  "used": 3,
  "limit": 3
}
```

Read operations remain available so operators can inspect and recover.

## Pilot Overrides

Platform admins can adjust a quota during beta operations:

```bash
curl -s -X POST "$BASE_URL/api/orgs/$ORG_ID/quotas" \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  --data '{"quota_key":"apps","limit":10}'
```
