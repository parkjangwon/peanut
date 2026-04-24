'use client'

import type { ReactNode } from 'react'
import { useEffect, useMemo, useState } from 'react'

type HealthResponse = {
  status: string
  message: string
}

type AdminUser = {
  id: string
  email: string
  is_active: boolean
  is_admin: boolean
  created_at: string
}

type StorageListResponse = {
  keys: string[]
}

const TOKEN_KEY = 'peanut.console.token'
const RECENT_KEYS_KEY = 'peanut.console.recentKeys'

function parseMeResponse(text: string) {
  const match = text.match(/Hello, user\s+(.+)!\s+Admin:\s+(true|false)/)
  return {
    raw: text,
    userId: match?.[1] ?? null,
    isAdmin: match?.[2] === 'true',
  }
}

function authHeaders(token: string | null): Record<string, string> {
  return token ? { Authorization: `Bearer ${token}` } : {}
}

export default function ConsoleClient() {
  const [health, setHealth] = useState<HealthResponse | null>(null)
  const [healthError, setHealthError] = useState<string | null>(null)
  const [email, setEmail] = useState('admin@example.com')
  const [password, setPassword] = useState('secret123')
  const [token, setToken] = useState<string | null>(null)
  const [authMessage, setAuthMessage] = useState('첫 사용자는 자동으로 admin + active 됩니다.')
  const [meText, setMeText] = useState<string | null>(null)
  const [sessionError, setSessionError] = useState<string | null>(null)
  const [storageKey, setStorageKey] = useState('notes/welcome.txt')
  const [storageBody, setStorageBody] = useState('hello from Peanut console')
  const [storageStatus, setStorageStatus] = useState('스토리지 작업 대기 중')
  const [storageKeys, setStorageKeys] = useState<string[]>([])
  const [recentKeys, setRecentKeys] = useState<string[]>([])
  const [adminUsers, setAdminUsers] = useState<AdminUser[]>([])
  const [adminStatus, setAdminStatus] = useState('admin 데이터 대기 중')
  const [busyAction, setBusyAction] = useState<string | null>(null)

  const me = useMemo(() => (meText ? parseMeResponse(meText) : null), [meText])

  useEffect(() => {
    if (typeof window === 'undefined') return

    const savedToken = window.localStorage.getItem(TOKEN_KEY)
    if (savedToken) {
      setToken(savedToken)
    }

    const savedRecentKeys = window.localStorage.getItem(RECENT_KEYS_KEY)
    if (savedRecentKeys) {
      try {
        const parsed = JSON.parse(savedRecentKeys) as string[]
        setRecentKeys(parsed)
        if (parsed[0]) {
          setStorageKey(parsed[0])
        }
      } catch {
        window.localStorage.removeItem(RECENT_KEYS_KEY)
      }
    }

    void loadHealth()
  }, [])

  useEffect(() => {
    if (typeof window === 'undefined') return

    if (token) {
      window.localStorage.setItem(TOKEN_KEY, token)
      void refreshSession(token)
      void refreshStorageList(token)
    } else {
      window.localStorage.removeItem(TOKEN_KEY)
      setMeText(null)
      setAdminUsers([])
      setStorageKeys([])
    }
  }, [token])

  useEffect(() => {
    if (typeof window === 'undefined') return
    window.localStorage.setItem(RECENT_KEYS_KEY, JSON.stringify(recentKeys))
  }, [recentKeys])

  async function loadHealth() {
    try {
      setHealthError(null)
      const language = typeof navigator === 'undefined' ? 'en-US' : navigator.language
      const response = await fetch('/api/health', {
        headers: {
          'accept-language': language,
        },
      })
      if (!response.ok) {
        throw new Error(`health request failed with ${response.status}`)
      }
      const data = (await response.json()) as HealthResponse
      setHealth(data)
    } catch (error) {
      setHealthError(error instanceof Error ? error.message : 'health request failed')
    }
  }

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
      const message = await response.text()
      setAuthMessage(`${response.status} ${message}`)
    } catch (error) {
      setAuthMessage(error instanceof Error ? error.message : 'register failed')
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
      const message = await response.text()
      if (!response.ok) {
        throw new Error(`${response.status} ${message}`)
      }
      setToken(message)
      setAuthMessage('로그인 성공. 토큰이 브라우저 localStorage에 저장되었습니다.')
      rememberKey(storageKey)
    } catch (error) {
      setAuthMessage(error instanceof Error ? error.message : 'login failed')
    } finally {
      setBusyAction(null)
    }
  }

  async function logout() {
    setToken(null)
    setAuthMessage('세션을 초기화했습니다.')
    setSessionError(null)
    setAdminStatus('admin 데이터 대기 중')
    setStorageStatus('스토리지 작업 대기 중')
  }

  async function refreshSession(currentToken: string) {
    try {
      setSessionError(null)
      const response = await fetch('/api/me', {
        headers: authHeaders(currentToken),
      })
      const text = await response.text()
      if (!response.ok) {
        throw new Error(`${response.status} ${text}`)
      }
      setMeText(text)
      const parsed = parseMeResponse(text)
      if (parsed.isAdmin) {
        await refreshAdminUsers(currentToken)
      } else {
        setAdminUsers([])
        setAdminStatus('현재 사용자는 admin이 아닙니다.')
      }
    } catch (error) {
      setSessionError(error instanceof Error ? error.message : 'session refresh failed')
    }
  }

  async function refreshAdminUsers(currentToken: string) {
    try {
      const response = await fetch('/api/admin/users', {
        headers: authHeaders(currentToken),
      })
      const body = await response.text()
      if (!response.ok) {
        throw new Error(`${response.status} ${body}`)
      }
      const users = JSON.parse(body) as AdminUser[]
      setAdminUsers(users)
      const pendingCount = users.filter((user) => !user.is_active).length
      setAdminStatus(`admin user view ready · pending ${pendingCount}명`)
    } catch (error) {
      setAdminStatus(error instanceof Error ? error.message : 'admin fetch failed')
    }
  }

  async function activateUser(userId: string) {
    if (!token) return
    setBusyAction(`activate:${userId}`)
    try {
      const response = await fetch(`/api/admin/users/${userId}/activate`, {
        method: 'PUT',
        headers: authHeaders(token),
      })
      if (!response.ok) {
        const text = await response.text()
        throw new Error(`${response.status} ${text}`)
      }
      setAdminStatus(`user ${userId} 활성화 완료`)
      await refreshAdminUsers(token)
    } catch (error) {
      setAdminStatus(error instanceof Error ? error.message : 'activation failed')
    } finally {
      setBusyAction(null)
    }
  }

  async function refreshStorageList(currentToken: string) {
    try {
      const response = await fetch('/api/storage', {
        headers: authHeaders(currentToken),
      })
      const body = await response.text()
      if (!response.ok) {
        throw new Error(`${response.status} ${body}`)
      }
      const data = JSON.parse(body) as StorageListResponse
      setStorageKeys(data.keys)
      if (data.keys[0] && !recentKeys.includes(data.keys[0])) {
        setRecentKeys((previous) => [data.keys[0], ...previous].slice(0, 8))
      }
    } catch (error) {
      setStorageStatus(error instanceof Error ? error.message : 'storage list failed')
    }
  }

  function rememberKey(key: string) {
    const normalized = key.trim()
    if (!normalized) return
    setRecentKeys((previous) => [normalized, ...previous.filter((item) => item !== normalized)].slice(0, 8))
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
      if (!response.ok) {
        const text = await response.text()
        throw new Error(`${response.status} ${text}`)
      }
      rememberKey(storageKey)
      setStorageStatus(`${storageKey} 저장 완료 (${response.status})`)
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
        throw new Error(`${response.status} ${text}`)
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
      if (!response.ok) {
        const text = await response.text()
        throw new Error(`${response.status} ${text}`)
      }
      setStorageStatus(`${key} 삭제 완료`)
      setStorageBody('')
      await refreshStorageList(token)
    } catch (error) {
      setStorageStatus(error instanceof Error ? error.message : 'delete failed')
    } finally {
      setBusyAction(null)
    }
  }

  const pendingUsers = adminUsers.filter((user) => !user.is_active)
  const activeUsers = adminUsers.filter((user) => user.is_active)

  return (
    <div className="min-h-screen bg-neutral-950 text-neutral-100">
      <div className="mx-auto flex w-full max-w-7xl flex-col gap-6 px-6 py-8 md:px-8 lg:px-10">
        <header className="flex flex-col gap-3 rounded-3xl border border-neutral-800 bg-neutral-900/70 p-6 shadow-2xl shadow-black/20">
          <div className="flex flex-col gap-2 md:flex-row md:items-end md:justify-between">
            <div>
              <p className="text-xs uppercase tracking-[0.35em] text-amber-300/80">single-binary backend platform</p>
              <h1 className="mt-2 text-3xl font-semibold tracking-tight md:text-4xl">🥜 Peanut Console</h1>
              <p className="mt-3 max-w-3xl text-sm leading-6 text-neutral-400 md:text-base">
                실제 `/api/health`, `/api/login`, `/api/me`, `/api/admin/users`, `/api/storage`에 연결된 운영 콘솔 MVP.
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
            detail={sessionError ?? me?.raw ?? '로그인 전'}
          />
          <StatusCard
            title="Role"
            value={me?.isAdmin ? 'admin' : token ? 'member' : 'guest'}
            accent={me?.isAdmin ? 'text-amber-300' : 'text-violet-300'}
            detail={me?.userId ?? '세션 없음'}
          />
          <StatusCard
            title="Storage keys"
            value={String(storageKeys.length)}
            accent="text-cyan-300"
            detail={storageKeys[0] ?? '아직 저장된 키 없음'}
          />
        </section>

        <main className="grid gap-6 xl:grid-cols-[1.05fr_1.2fr]">
          <section className="grid gap-6">
            <Panel title="Auth" subtitle="register / login / me flow">
              <div className="grid gap-4">
                <label className="grid gap-2 text-sm text-neutral-300">
                  Email
                  <input
                    className="rounded-2xl border border-neutral-800 bg-neutral-950 px-4 py-3 text-sm text-neutral-100 outline-none transition focus:border-sky-500"
                    onChange={(event) => setEmail(event.target.value)}
                    value={email}
                  />
                </label>
                <label className="grid gap-2 text-sm text-neutral-300">
                  Password
                  <input
                    className="rounded-2xl border border-neutral-800 bg-neutral-950 px-4 py-3 text-sm text-neutral-100 outline-none transition focus:border-sky-500"
                    onChange={(event) => setPassword(event.target.value)}
                    type="password"
                    value={password}
                  />
                </label>
                <div className="flex flex-wrap gap-3">
                  <ActionButton busy={busyAction === 'register'} onClick={() => void register()}>
                    Register
                  </ActionButton>
                  <ActionButton busy={busyAction === 'login'} onClick={() => void login()} primary>
                    Login
                  </ActionButton>
                  <ActionButton onClick={() => void logout()}>Logout</ActionButton>
                </div>
                <InfoBox label="Auth status" value={authMessage} />
                <InfoBox
                  label="JWT token"
                  value={token ? `${token.slice(0, 24)}…` : '토큰 없음'}
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
            <Panel title="Storage" subtitle="save / load / delete objects through the protected API">
              <div className="grid gap-4">
                <label className="grid gap-2 text-sm text-neutral-300">
                  Object key
                  <input
                    className="rounded-2xl border border-neutral-800 bg-neutral-950 px-4 py-3 text-sm text-neutral-100 outline-none transition focus:border-cyan-500"
                    onChange={(event) => setStorageKey(event.target.value)}
                    value={storageKey}
                  />
                </label>
                <label className="grid gap-2 text-sm text-neutral-300">
                  Content
                  <textarea
                    className="min-h-[240px] rounded-3xl border border-neutral-800 bg-neutral-950 px-4 py-3 text-sm text-neutral-100 outline-none transition focus:border-cyan-500"
                    onChange={(event) => setStorageBody(event.target.value)}
                    value={storageBody}
                  />
                </label>
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
                    <EmptyState text="로그인 후 저장된 키 목록을 볼 수 있어." />
                  ) : (
                    storageKeys.map((key) => (
                      <button
                        key={key}
                        className="rounded-2xl border border-neutral-800 bg-neutral-950 px-4 py-3 text-left text-sm text-neutral-200 transition hover:border-cyan-500 hover:bg-neutral-900"
                        onClick={() => void loadObject(key)}
                        type="button"
                      >
                        {key}
                      </button>
                    ))
                  )}
                </div>
              </Panel>

              <Panel title="Recent keys" subtitle="browser localStorage shortcuts">
                <div className="grid gap-3">
                  {recentKeys.length === 0 ? (
                    <EmptyState text="아직 최근에 다룬 키가 없어." />
                  ) : (
                    recentKeys.map((key) => (
                      <button
                        key={key}
                        className="rounded-2xl border border-neutral-800 bg-neutral-950 px-4 py-3 text-left text-sm text-neutral-200 transition hover:border-sky-500 hover:bg-neutral-900"
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
          </section>
        </main>
      </div>
    </div>
  )
}

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

function EmptyState({ text }: { text: string }) {
  return <div className="rounded-2xl border border-dashed border-neutral-800 px-4 py-6 text-sm text-neutral-500">{text}</div>
}
