import test from 'node:test'
import assert from 'node:assert/strict'

import {
  buildPushSummary,
  summarizeSubscriptionKinds,
  recentFailedItems,
} from './push-console-utils.mjs'

test('buildPushSummary counts queue statuses', () => {
  const items = [
    { status: 'pending' },
    { status: 'pending' },
    { status: 'sent' },
    { status: 'failed' },
  ]

  assert.deepEqual(buildPushSummary(items), {
    total: 4,
    pending: 2,
    sent: 1,
    failed: 1,
    other: 0,
  })
})

test('summarizeSubscriptionKinds counts ntfy and web_push kinds', () => {
  const subscriptions = [
    { kind: 'ntfy' },
    { kind: 'web_push' },
    { kind: 'web_push' },
    { kind: 'unknown' },
  ]

  assert.deepEqual(summarizeSubscriptionKinds(subscriptions), {
    total: 4,
    ntfy: 1,
    webPush: 2,
    other: 1,
  })
})

test('recentFailedItems returns newest failed items first and limits count', () => {
  const items = [
    { id: 1, status: 'sent', created_at: '2026-04-25 01:00:00', last_error: null },
    { id: 2, status: 'failed', created_at: '2026-04-25 01:01:00', last_error: 'timeout' },
    { id: 3, status: 'failed', created_at: '2026-04-25 01:03:00', last_error: 'invalid subscription' },
    { id: 4, status: 'failed', created_at: '2026-04-25 01:02:00', last_error: 'network' },
  ]

  assert.deepEqual(
    recentFailedItems(items, 2).map((item) => item.id),
    [3, 4],
  )
})
