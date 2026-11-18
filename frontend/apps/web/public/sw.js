/* Receives encrypted, content-minimal Web Push payloads and displays them even when the app is closed. */
self.addEventListener('push', (event) => {
  event.waitUntil((async () => {
    const windows = await self.clients.matchAll({ type: 'window', includeUncontrolled: true })
    if (windows.some((client) => client.visibilityState === 'visible' && client.focused)) return
    let payload = { title: 'RustChat', body: 'You have a new notification.', url: '/' }
    try { if (event.data) payload = { ...payload, ...event.data.json() } } catch { /* Keep the generic fallback. */ }
    const requested = new URL(typeof payload.url === 'string' ? payload.url : '/', self.location.origin)
    const url = requested.origin === self.location.origin ? requested.pathname + requested.search + requested.hash : '/'
    await self.registration.showNotification(payload.title, {
      body: payload.body,
      icon: '/favicon.svg',
      badge: '/favicon.svg',
      tag: 'rustchat-notification',
      data: { url },
    })
  })())
})

self.addEventListener('notificationclick', (event) => {
  event.notification.close()
  const target = new URL(event.notification.data?.url || '/', self.location.origin).href
  event.waitUntil(self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then((clients) => {
    const existing = clients.find((client) => new URL(client.url).origin === self.location.origin)
    if (existing) return existing.focus().then(() => existing.navigate(target))
    return self.clients.openWindow(target)
  }))
})
