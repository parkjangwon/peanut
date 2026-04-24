'use client'

import type { ReactNode } from 'react'
import { useCallback, useEffect, useMemo, useState } from 'react'

type ApiError = {
  error: string
}

type MessageResponse = {
  message: string
}

type HealthResponse = {
  status: string
  message: string
}

type UserSummary = {
  id: string
  email: string
  is_active: boolean
  is_admin: boolean
}

type RegisterResponse = {
  message: string
  user: UserSummary
}

type LoginResponse = {
  access_token: string
  token_type: string
  expires_at: string
  user: UserSummary
}

type SessionResponse = {
  user: UserSummary
}

type AdminUser = UserSummary & {
  created_at: string
}

type AdminUsersResponse = {
  users: AdminUser[]
}

type StorageListResponse = {
  keys: string[]
}

type PushSubscription = {
  id: number
  topic: string
  created_at: string
}

type PushSubscriptionsResponse = {
  subscriptions: PushSubscription[]
}

type PushQueueEntry = {
  id: number
  user_id: string
  title: string
  body: string
  status: string
  retry_count: number
  last_error: string | null
  created_at: string
  processed_at: string | null
}

type PushQueueResponse = {
  items: PushQueueEntry[]
}

const RECENT_KEYS_KEY = 'peanut.console.recentKeys'

function authHeaders(token: string | null): Record<string, string> {
  return token ? { Authorization: `Bearer ${token}` } : {}
}

function readRecentKeys(): string[] {
  if (typeof window === 'undefined') {
    return []
  }

  const raw = window.localStorage.getItem(RECENT_KEYS_KEY)
  if (!raw) {
    return []
  }

  try {
    const parsed = JSON.parse(raw) as string[]
    return Array.isArray(parsed) ? parsed.filter((value) => typeof value === 'string') : []
  } catch {
    window.localStorage.removeItem(RECENT_KEYS_KEY)
    return []
  }
}

async function readJsonOrThrow<T>(response: Response): Promise<T> {
  const text = await response.text()
  if (!response.ok) {
    try {
      const error = JSON.parse(text) as ApiError
      throw new Error(error.error)
    } catch {
      throw new Error(text || `request failed with ${response.status}`)
    }
  }
  return JSON.parse(text) as T
}

export default function ConsoleClient() {
  const [token, setToken] = useState<string | null>(null)
  const [health, setHealth] = useState<HealthResponse | null>(null)
  const [healthError, setHealthError] = useState<string | null>(null)
  const [email, setEmail] = useState('admin@example.com')
  const [password, setPassword] = useState('secret123')
  const [authStatus, setAuthStatus] = useState('첫 사용자는 자동으로 active admin으로 생성돼.')
  const [session, setSession] = useState<SessionResponse | null>(null)
  const [sessionError, setSessionError] = useState<string | null>(null)
  const [storageKey, setStorageKey] = useState('notes/welcome.txt')
  const [storageBody, setStorageBody] = useState('hello from Peanut console')
  const [storageStatus, setStorageStatus] = useState('스토리지 작업 대기 중')
  const [storageKeys, setStorageKeys] = useState<string[]>([])
  const [recentKeys, setRecentKeys] = useState<string[]>(() => readRecentKeys())
  const [adminUsers, setAdminUsers] = useState<AdminUser[]>([])
  const [adminStatus, setAdminStatus] = useState('admin 데이터 대기 중')
  const [pushTopic, setPushTopic] = useState('alerts_main')
  const [pushTitle, setPushTitle] = useState('Peanut notification')
  const [pushBody, setPushBody] = useState('Single-binary backend is alive.')
  const [pushStatus, setPushStatus] = useState('push 작업 대기 중')
  const [subscriptions, setSubscriptions] = useState<PushSubscription[]>([])
  const [queueItems, setQueueItems] = useState<PushQueueEntry[]>([])
  const [busyAction, setBusyAction] = useState<string | null>(null)

  useEffect(() => {
    if (typeof window !== 'undefined') {
      window.localStorage.setItem(RECENT_KEYS_KEY, JSON.stringify(recentKeys))
    }
  }, [recentKeys])

  const rememberKey = useCallback((key: string) => {
    const normalized = key.trim()
    if (!normalized) return
    setRecentKeys((previous) => [normalized, ...previous.filter((item) => item !== normalized)].slice(0, 8))
  }, [])

  const loadHealth = useCallback(async () => {
    try {
      setHealthError(null)
      const language = typeof navigator === 'undefined' ? 'en-US' : navigator.language
      const response = await fetch('/api/health', {
        headers: {
          'accept-language': language,
        },
      })
      const data = await readJsonOrThrow<HealthResponse>(response)
      setHealth(data)
    } catch (error) {
      setHealthError(error instanceof Error ? error.message : 'health request failed')
    }
  }, [])

  const refreshAdminUsers = useCallback(async (currentToken: string) => {
    const response = await fetch('/api/admin/users', {
      headers: authHeaders(currentToken),
    })
    const data = await readJsonOrThrow<AdminUsersResponse>(response)
    setAdminUsers(data.users)
    setAdminStatus(`admin user view ready · pending ${data.users.filter((user) => !user.is_active).length}명`)
  }, [])

  const refreshStorageList = useCallback(async (currentToken: string) => {
    const response = await fetch('/api/storage', {
      headers: authHeaders(currentToken),
    })
    const data = await readJsonOrThrow<StorageListResponse>(response)
    setStorageKeys(data.keys)
  }, [])

  const refreshPushData = useCallback(async (currentToken: string) => {
    const [subscriptionsResponse, queueResponse] = await Promise.all([
      fetch('/api/push/subscriptions', {
        headers: authHeaders(currentToken),
      }),
      fetch('/api/push/queue', {
        headers: authHeaders(currentToken),
      }),
    ])

    const subscriptionData = await readJsonOrThrow<PushSubscriptionsResponse>(subscriptionsResponse)
    const queueData = await readJsonOrThrow<PushQueueResponse>(queueResponse)
    setSubscriptions(subscriptionData.subscriptions)
    setQueueItems(queueData.items)
    setPushStatus(`subscription ${subscriptionData.subscriptions.length}개 · queue ${queueData.items.length}건`)
  }, [])

  const refreshSession = useCallback(
    async (currentToken: string) => {
      const response = await fetch('/api/me', {
        headers: authHeaders(currentToken),
      })
      const data = await readJsonOrThrow<SessionResponse>(response)
      setSession(data)
      setSessionError(null)

      await Promise.all([
        refreshStorageList(currentToken),
        refreshPushData(currentToken),
        data.user.is_admin
          ? refreshAdminUsers(currentToken)
          : Promise.resolve().then(() => {
              setAdminUsers([])
              setAdminStatus('현재 사용자는 admin이 아니야.')
            }),
      ])
    },
    [refreshAdminUsers, refreshPushData, refreshStorageList],
  )

  useEffect(() => {
    const timeoutId = window.setTimeout(() => {
      void loadHealth()
    }, 0)

    return () => window.clearTimeout(timeoutId)
  }, [loadHealth])

  useEffect(() => {
    if (!token) {
      return
    }

    const timeoutId = window.setTimeout(() => {
      void refreshSession(token).catch((error) => {
        setSessionError(error instanceof Error ? error.message : 'session refresh failed')
      })
    }, 0)

    return () => window.clearTimeout(timeoutId)
  }, [refreshSession, token])

  const pendingUsers = useMemo(() => adminUsers.filter((user) => !user.is_active), [adminUsers])
  const activeUsers = useMemo(() => adminUsers.filter((user) => user.is_active), [adminUsers])

  async function register() {
    setBusyAction('register')
    try {
      const response = await fetch('/api/register', {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
        },
        body: JSON.stringify({ email, password }),
      })
      const data = await readJsonOrThrow<RegisterResponse>(response)
      setAuthStatus(data.message)
    } catch (error) {
      setAuthStatus(error instanceof Error ? error.message : 'register failed')
    } finally {
      setBusyAction(null)
    }
  }

  async function login() {
    setBusyAction('login')
    try {
      const response = await fetch('/api/login', {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
        },
        body: JSON.stringify({ email, password }),
      })
      const data = await readJsonOrThrow<LoginResponse>(response)
      setToken(data.access_token)
      setSession({ user: data.user })
      setAuthStatus(`로그인 성공 · expires ${new Date(data.expires_at).toLocaleString()}`)
      setSessionError(null)
    } catch (error) {
      setAuthStatus(error instanceof Error ? error.message : 'login failed')
    } finally {
      setBusyAction(null)
    }
  }

  function logout() {
    setToken(null)
    setSession(null)
    setSessionError(null)
    setAdminUsers([])
    setAdminStatus('admin 데이터 대기 중')
    setStorageKeys([])
    setStorageStatus('스토리지 작업 대기 중')
    setSubscriptions([])
    setQueueItems([])
    setPushStatus('push 작업 대기 중')
    setAuthStatus('세션을 메모리에서 제거했어.')
  }

  async function activateUser(userId: string) {
    if (!token) return
    setBusyAction(`activate:${userId}`)
    try {
      const response = await fetch(`/api/admin/users/${userId}/activate`, {
        method: 'PUT',
        headers: authHeaders(token),
      })
      const data = await readJsonOrThrow<MessageResponse>(response)
      setAdminStatus(data.message)
      await refreshAdminUsers(token)
    } catch (error) {
      setAdminStatus(error instanceof Error ? error.message : 'activation failed')
    } finally {
      setBusyAction(null)
    }
  }

  async function saveObject() {
    if (!token) {
      setStorageStatus('먼저 로그인해줘.')
      return
    }
    setBusyAction('saveObject')
    try {
      const response = await fetch(`/api/storage/${encodeURI(storageKey)}`, {
        method: 'PUT',
        headers: authHeaders(token),
        body: storageBody,
      })
      const data = await readJsonOrThrow<MessageResponse>(response)
      rememberKey(storageKey)
      setStorageStatus(data.message)
      await refreshStorageList(token)
    } catch (error) {
      setStorageStatus(error instanceof Error ? error.message : 'save failed')
    } finally {
      setBusyAction(null)
    }
  }

  async function loadObject(key = storageKey) {
    if (!token) {
      setStorageStatus('먼저 로그인해줘.')
      return
    }
    setBusyAction('loadObject')
    try {
      const response = await fetch(`/api/storage/${encodeURI(key)}`, {
        headers: authHeaders(token),
      })
      const text = await response.text()
      if (!response.ok) {
        try {
          const error = JSON.parse(text) as ApiError
          throw new Error(error.error)
        } catch {
          throw new Error(text || 'failed to load object')
        }
      }
      setStorageKey(key)
      setStorageBody(text)
      rememberKey(key)
      setStorageStatus(`${key} 불러오기 완료`)
    } catch (error) {
      setStorageStatus(error instanceof Error ? error.message : 'load failed')
    } finally {
      setBusyAction(null)
    }
  }

  async function deleteObject(key = storageKey) {
    if (!token) {
      setStorageStatus('먼저 로그인해줘.')
      return
    }
    setBusyAction('deleteObject')
    try {
      const response = await fetch(`/api/storage/${encodeURI(key)}`, {
        method: 'DELETE',
        headers: authHeaders(token),
      })
      const data = await readJsonOrThrow<MessageResponse>(response)
      setStorageStatus(data.message)
      setStorageBody('')
      await refreshStorageList(token)
    } catch (error) {
      setStorageStatus(error instanceof Error ? error.message : 'delete failed')
    } finally {
      setBusyAction(null)
    }
  }

  async function subscribeTopic() {
    if (!token) {
      setPushStatus('먼저 로그인해줘.')
      return
    }
    setBusyAction('subscribeTopic')
    try {
      const response = await fetch('/api/push/subscriptions', {
        method: 'POST',
        headers: {
          ...authHeaders(token),
          'content-type': 'application/json',
        },
        body: JSON.stringify({ topic: pushTopic }),
      })
      const data = await readJsonOrThrow<MessageResponse>(response)
      setPushStatus(data.message)
      await refreshPushData(token)
    } catch (error) {
      setPushStatus(error instanceof Error ? error.message : 'subscription failed')
    } finally {
      setBusyAction(null)
    }
  }

  async function deleteSubscription(subscriptionId: number) {
    if (!token) return
    setBusyAction(`deleteSubscription:${subscriptionId}`)
    try {
      const response = await fetch(`/api/push/subscriptions/${subscriptionId}`, {
        method: 'DELETE',
        headers: authHeaders(token),
      })
      const data = await readJsonOrThrow<MessageResponse>(response)
      setPushStatus(data.message)
      await refreshPushData(token)
    } catch (error) {
      setPushStatus(error instanceof Error ? error.message : 'delete subscription failed')
    } finally {
      setBusyAction(null)
    }
  }

  async function enqueuePush() {
    if (!token) {
      setPushStatus('먼저 로그인해줘.')
      return
    }
    setBusyAction('enqueuePush')
    try {
      const response = await fetch('/api/push/messages', {
        method: 'POST',
        headers: {
          ...authHeaders(token),
          'content-type': 'application/json',
        },
        body: JSON.stringify({ title: pushTitle, body: pushBody }),
      })
      const data = await readJsonOrThrow<MessageResponse>(response)
      setPushStatus(data.message)
      await refreshPushData(token)
    } catch (error) {
      setPushStatus(error instanceof Error ? error.message : 'enqueue failed')
    } finally {
      setBusyAction(null)
    }
  }

  return (
    <div className="min-h-screen bg-neutral-950 text-neutral-100">
      <div className="mx-auto flex w-full max-w-7xl flex-col gap-6 px-6 py-8 md:px-8 lg:px-10">
        <header className="rounded-3xl border border-neutral-800 bg-neutral-900/70 p-6 shadow-2xl shadow-black/20">
          <div className="flex flex-col gap-4 md:flex-row md:items-end md:justify-between">
            <div>
              <p className="text-xs uppercase tracking-[0.35em] text-amber-300/80">single-binary backend platform</p>
              <h1 className="mt-2 text-3xl font-semibold tracking-tight md:text-4xl">🥜 Peanut Console</h1>
              <p className="mt-3 max-w-3xl text-sm leading-6 text-neutral-400 md:text-base">
                health/auth/admin/storage/push를 같은 바이너리에서 운영하는 self-host 콘솔.
              </p>
            </div>
            <button
              className="rounded-full border border-neutral-700 px-4 py-2 text-sm text-neutral-200 transition hover:border-neutral-500 hover:bg-neutral-800"
              onClick={() => void loadHealth()}
              type="button"
            >
              health 새로고침
            </button>
          </div>
        </header>

        <section className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
          <StatusCard
            title="Health"
            value={health?.status ?? 'loading'}
            accent="text-emerald-400"
            detail={healthError ?? health?.message ?? 'API 응답 대기 중'}
          />
          <StatusCard
            title="Session"
            value={token ? 'authenticated' : 'anonymous'}
            accent={token ? 'text-sky-400' : 'text-neutral-300'}
            detail={sessionError ?? session?.user.email ?? '로그인 전'}
          />
          <StatusCard
            title="Role"
            value={session?.user.is_admin ? 'admin' : token ? 'member' : 'guest'}
            accent={session?.user.is_admin ? 'text-amber-300' : 'text-violet-300'}
            detail={session?.user.id ?? '세션 없음'}
          />
          <StatusCard
            title="Push queue"
            value={String(queueItems.length)}
            accent="text-cyan-300"
            detail={queueItems[0]?.status ?? '큐 비어 있음'}
          />
        </section>

        <main className="grid gap-6 xl:grid-cols-[1.05fr_1.2fr]">
          <section className="grid gap-6">
            <Panel title="Auth" subtitle="register / login / session flow">
              <div className="grid gap-4">
                <Field label="Email">
                  <input
                    className={inputClassName}
                    onChange={(event) => setEmail(event.target.value)}
                    value={email}
                  />
                </Field>
                <Field label="Password">
                  <input
                    className={inputClassName}
                    onChange={(event) => setPassword(event.target.value)}
                    type="password"
                    value={password}
                  />
                </Field>
                <div className="flex flex-wrap gap-3">
                  <ActionButton busy={busyAction === 'register'} onClick={() => void register()}>
                    Register
                  </ActionButton>
                  <ActionButton busy={busyAction === 'login'} onClick={() => void login()} primary>
                    Login
                  </ActionButton>
                  <ActionButton onClick={logout}>Logout</ActionButton>
                </div>
                <InfoBox label="Auth status" value={authStatus} />
                <InfoBox
                  label="Session token"
                  value={token ? `${token.slice(0, 24)}… (메모리 전용)` : '토큰 없음'}
                />
              </div>
            </Panel>

            <Panel title="Admin" subtitle="pending user approval">
              <div className="grid gap-4">
                <InfoBox label="Admin status" value={adminStatus} />
                <div className="grid gap-3">
                  <h3 className="text-sm font-medium text-neutral-200">Pending users</h3>
                  {pendingUsers.length === 0 ? (
                    <EmptyState text="대기 중인 유저가 없거나 현재 계정이 admin이 아니야." />
                  ) : (
                    pendingUsers.map((user) => (
                      <div
                        key={user.id}
                        className="flex flex-col gap-3 rounded-2xl border border-amber-500/20 bg-amber-500/5 p-4 md:flex-row md:items-center md:justify-between"
                      >
                        <div>
                          <p className="font-medium text-neutral-100">{user.email}</p>
                          <p className="text-xs text-neutral-400">{user.id}</p>
                        </div>
                        <ActionButton
                          busy={busyAction === `activate:${user.id}`}
                          onClick={() => void activateUser(user.id)}
                          primary
                        >
                          Activate
                        </ActionButton>
                      </div>
                    ))
                  )}
                </div>
                <div className="grid gap-3">
                  <h3 className="text-sm font-medium text-neutral-200">Active users</h3>
                  {activeUsers.length === 0 ? (
                    <EmptyState text="활성 유저가 아직 없어." />
                  ) : (
                    activeUsers.map((user) => (
                      <div key={user.id} className="rounded-2xl border border-neutral-800 bg-neutral-950/80 p-4">
                        <p className="font-medium text-neutral-100">{user.email}</p>
                        <p className="mt-1 text-xs text-neutral-400">{user.id}</p>
                        <p className="mt-2 text-xs text-neutral-500">
                          role: {user.is_admin ? 'admin' : 'member'} · {user.created_at}
                        </p>
                      </div>
                    ))
                  )}
                </div>
              </div>
            </Panel>
          </section>

          <section className="grid gap-6">
            <Panel title="Storage" subtitle="user-scoped object operations">
              <div className="grid gap-4">
                <Field label="Object key">
                  <input
                    className={inputClassName}
                    onChange={(event) => setStorageKey(event.target.value)}
                    value={storageKey}
                  />
                </Field>
                <Field label="Content">
                  <textarea
                    className="min-h-[220px] rounded-3xl border border-neutral-800 bg-neutral-950 px-4 py-3 text-sm text-neutral-100 outline-none transition focus:border-cyan-500"
                    onChange={(event) => setStorageBody(event.target.value)}
                    value={storageBody}
                  />
                </Field>
                <div className="flex flex-wrap gap-3">
                  <ActionButton busy={busyAction === 'saveObject'} onClick={() => void saveObject()} primary>
                    Save object
                  </ActionButton>
                  <ActionButton busy={busyAction === 'loadObject'} onClick={() => void loadObject()}>
                    Load object
                  </ActionButton>
                  <ActionButton busy={busyAction === 'deleteObject'} onClick={() => void deleteObject()}>
                    Delete object
                  </ActionButton>
                  <ActionButton onClick={() => token && void refreshStorageList(token)}>Refresh keys</ActionButton>
                </div>
                <InfoBox label="Storage status" value={storageStatus} />
              </div>
            </Panel>

            <div className="grid gap-6 md:grid-cols-2">
              <Panel title="Stored keys" subtitle="server-side list API">
                <div className="grid gap-3">
                  {storageKeys.length === 0 ? (
                    <EmptyState text="로그인한 유저 기준으로 저장된 키만 보여줘." />
                  ) : (
                    storageKeys.map((key) => (
                      <button
                        key={key}
                        className={listButtonClassName}
                        onClick={() => void loadObject(key)}
                        type="button"
                      >
                        {key}
                      </button>
                    ))
                  )}
                </div>
              </Panel>

              <Panel title="Recent keys" subtitle="non-sensitive browser shortcuts">
                <div className="grid gap-3">
                  {recentKeys.length === 0 ? (
                    <EmptyState text="아직 최근에 다룬 키가 없어." />
                  ) : (
                    recentKeys.map((key) => (
                      <button
                        key={key}
                        className={listButtonClassName}
                        onClick={() => {
                          setStorageKey(key)
                          void loadObject(key)
                        }}
                        type="button"
                      >
                        {key}
                      </button>
                    ))
                  )}
                </div>
              </Panel>
            </div>

            <Panel title="Push (ntfy MVP)" subtitle="subscription + queue">
              <div className="grid gap-4">
                <Field label="Topic">
                  <input
                    className={inputClassName}
                    onChange={(event) => setPushTopic(event.target.value)}
                    value={pushTopic}
                  />
                </Field>
                <div className="flex flex-wrap gap-3">
                  <ActionButton busy={busyAction === 'subscribeTopic'} onClick={() => void subscribeTopic()} primary>
                    Subscribe topic
                  </ActionButton>
                  <ActionButton onClick={() => token && void refreshPushData(token)}>Refresh push</ActionButton>
                </div>
                <Field label="Push title">
                  <input
                    className={inputClassName}
                    onChange={(event) => setPushTitle(event.target.value)}
                    value={pushTitle}
                  />
                </Field>
                <Field label="Push body">
                  <textarea
                    className="min-h-[120px] rounded-3xl border border-neutral-800 bg-neutral-950 px-4 py-3 text-sm text-neutral-100 outline-none transition focus:border-fuchsia-500"
                    onChange={(event) => setPushBody(event.target.value)}
                    value={pushBody}
                  />
                </Field>
                <ActionButton busy={busyAction === 'enqueuePush'} onClick={() => void enqueuePush()}>
                  Enqueue push
                </ActionButton>
                <InfoBox label="Push status" value={pushStatus} />

                <div className="grid gap-3 md:grid-cols-2">
                  <div className="grid gap-3">
                    <h3 className="text-sm font-medium text-neutral-200">Subscriptions</h3>
                    {subscriptions.length === 0 ? (
                      <EmptyState text="먼저 topic을 구독해야 ntfy 메시지를 받을 수 있어." />
                    ) : (
                      subscriptions.map((subscription) => (
                        <div
                          key={subscription.id}
                          className="flex items-center justify-between gap-3 rounded-2xl border border-neutral-800 bg-neutral-950/80 p-4"
                        >
                          <div>
                            <p className="font-medium text-neutral-100">{subscription.topic}</p>
                            <p className="text-xs text-neutral-500">{subscription.created_at}</p>
                          </div>
                          <ActionButton
                            busy={busyAction === `deleteSubscription:${subscription.id}`}
                            onClick={() => void deleteSubscription(subscription.id)}
                          >
                            Delete
                          </ActionButton>
                        </div>
                      ))
                    )}
                  </div>

                  <div className="grid gap-3">
                    <h3 className="text-sm font-medium text-neutral-200">Queue</h3>
                    {queueItems.length === 0 ? (
                      <EmptyState text="아직 큐에 들어간 메시지가 없어." />
                    ) : (
                      queueItems.map((item) => (
                        <div key={item.id} className="rounded-2xl border border-neutral-800 bg-neutral-950/80 p-4">
                          <div className="flex items-center justify-between gap-3">
                            <p className="font-medium text-neutral-100">{item.title}</p>
                            <span className="text-xs uppercase tracking-[0.2em] text-fuchsia-300">
                              {item.status}
                            </span>
                          </div>
                          <p className="mt-2 text-sm text-neutral-300">{item.body}</p>
                          <p className="mt-3 text-xs text-neutral-500">
                            retries: {item.retry_count} · created: {item.created_at}
                          </p>
                          {item.last_error ? (
                            <p className="mt-2 text-xs text-rose-300">last error: {item.last_error}</p>
                          ) : null}
                        </div>
                      ))
                    )}
                  </div>
                </div>
              </div>
            </Panel>
          </section>
        </main>
      </div>
    </div>
  )
}

const inputClassName =
  'rounded-2xl border border-neutral-800 bg-neutral-950 px-4 py-3 text-sm text-neutral-100 outline-none transition focus:border-sky-500'

const listButtonClassName =
  'rounded-2xl border border-neutral-800 bg-neutral-950 px-4 py-3 text-left text-sm text-neutral-200 transition hover:border-cyan-500 hover:bg-neutral-900'

type StatusCardProps = {
  title: string
  value: string
  detail: string
  accent: string
}

function StatusCard({ title, value, detail, accent }: StatusCardProps) {
  return (
    <div className="rounded-3xl border border-neutral-800 bg-neutral-900/80 p-5 shadow-xl shadow-black/10">
      <p className="text-xs uppercase tracking-[0.24em] text-neutral-500">{title}</p>
      <p className={`mt-3 text-2xl font-semibold ${accent}`}>{value}</p>
      <p className="mt-3 text-sm leading-6 text-neutral-400">{detail}</p>
    </div>
  )
}

type PanelProps = {
  title: string
  subtitle: string
  children: ReactNode
}

function Panel({ title, subtitle, children }: PanelProps) {
  return (
    <section className="rounded-3xl border border-neutral-800 bg-neutral-900/70 p-5 shadow-xl shadow-black/10 md:p-6">
      <div className="mb-5">
        <h2 className="text-lg font-semibold text-neutral-100">{title}</h2>
        <p className="mt-1 text-sm text-neutral-400">{subtitle}</p>
      </div>
      {children}
    </section>
  )
}

type ActionButtonProps = {
  children: ReactNode
  onClick: () => void
  busy?: boolean
  primary?: boolean
}

function ActionButton({ children, onClick, busy, primary }: ActionButtonProps) {
  return (
    <button
      className={[
        'rounded-full px-4 py-2 text-sm font-medium transition',
        primary
          ? 'bg-sky-500 text-sky-950 hover:bg-sky-400'
          : 'border border-neutral-700 bg-neutral-900 text-neutral-100 hover:border-neutral-500 hover:bg-neutral-800',
        busy ? 'cursor-wait opacity-60' : '',
      ].join(' ')}
      disabled={busy}
      onClick={onClick}
      type="button"
    >
      {busy ? '처리 중…' : children}
    </button>
  )
}

type InfoBoxProps = {
  label: string
  value: string
}

function InfoBox({ label, value }: InfoBoxProps) {
  return (
    <div className="rounded-2xl border border-neutral-800 bg-neutral-950/80 p-4">
      <p className="text-xs uppercase tracking-[0.24em] text-neutral-500">{label}</p>
      <p className="mt-2 whitespace-pre-wrap break-all text-sm leading-6 text-neutral-200">{value}</p>
    </div>
  )
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="grid gap-2 text-sm text-neutral-300">
      {label}
      {children}
    </label>
  )
}

function EmptyState({ text }: { text: string }) {
  return (
    <div className="rounded-2xl border border-dashed border-neutral-800 px-4 py-6 text-sm text-neutral-500">
      {text}
    </div>
  )
}
