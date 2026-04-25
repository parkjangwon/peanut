self.addEventListener('push', (event) => {
  const payload = (() => {
    try {
      return event.data?.json() ?? {}
    } catch {
      return { title: 'Peanut notification', body: event.data?.text() ?? '' }
    }
  })()

  const title = payload.title || 'Peanut notification'
  const body = payload.body || '새 알림이 도착했어.'

  event.waitUntil(
    self.registration.showNotification(title, {
      body,
      icon: '/next.svg',
      badge: '/next.svg',
    }),
  )
})

self.addEventListener('notificationclick', (event) => {
  event.notification.close()
  event.waitUntil(self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then((clients) => {
    const client = clients[0]
    if (client) {
      return client.focus()
    }
    return self.clients.openWindow('/')
  }))
})
