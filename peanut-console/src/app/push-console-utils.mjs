export function buildPushSummary(items) {
  return items.reduce(
    (summary, item) => {
      const status = String(item?.status || '').toLowerCase()
      summary.total += 1
      if (status === 'pending') summary.pending += 1
      else if (status === 'sent') summary.sent += 1
      else if (status === 'failed') summary.failed += 1
      else summary.other += 1
      return summary
    },
    { total: 0, pending: 0, sent: 0, failed: 0, other: 0 },
  )
}

export function summarizeSubscriptionKinds(subscriptions) {
  return subscriptions.reduce(
    (summary, subscription) => {
      const kind = String(subscription?.kind || '').toLowerCase()
      summary.total += 1
      if (kind === 'ntfy') summary.ntfy += 1
      else if (kind === 'web_push') summary.webPush += 1
      else summary.other += 1
      return summary
    },
    { total: 0, ntfy: 0, webPush: 0, other: 0 },
  )
}

export function recentFailedItems(items, limit = 3) {
  return items
    .filter((item) => String(item?.status || '').toLowerCase() === 'failed')
    .sort((left, right) => String(right.created_at || '').localeCompare(String(left.created_at || '')))
    .slice(0, limit)
}
