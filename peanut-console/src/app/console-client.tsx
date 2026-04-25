'use client'

import type { ReactNode } from 'react'
import { useCallback, useEffect, useMemo, useState } from 'react'

import { buildPushSummary, recentFailedItems, summarizeSubscriptionKinds } from './push-console-utils.mjs'

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
  kind: string
  topic: string | null
  endpoint: string | null
  created_at: string
}

type PushSubscriptionsResponse = {
  subscriptions: PushSubscription[]
}

type DataTableSummary = {
  name: string
  display_name: string
  policy_mode: string
  created_at: string
}

type DataTablesResponse = {
  tables: DataTableSummary[]
}

type DataFieldSpec = {
  type: string
  required?: boolean
  max_length?: number | null
  default?: unknown
}

type DataTableDetail = {
  name: string
  display_name: string
  schema: {
    fields: Record<string, DataFieldSpec>
  }
  access_policy: {
    mode: string
  }
  created_by: string
  created_at: string
}

type DataTableResponse = {
  table: DataTableDetail
}

type DataRow = {
  id: string
  owner_user_id: string | null
  data: Record<string, unknown>
  created_at: string
  updated_at: string
}

type DataRowsResponse = {
  rows: DataRow[]
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

type VapidPublicKeyResponse = {
  public_key: string
}

const RECENT_KEYS_KEY = 'peanut.console.recentKeys'

type ConsoleView = 'overview' | 'auth' | 'data' | 'storage' | 'push' | 'admin'
type StorageSource = 'server' | 'recent'
type DataInspectorMode = 'rows' | 'schema'
type PushTab = 'composer' | 'subscriptions' | 'deliveries'

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
  const [dataStatus, setDataStatus] = useState('data 작업 대기 중')
  const [dataTables, setDataTables] = useState<DataTableSummary[]>([])
  const [selectedTable, setSelectedTable] = useState('')
  const [selectedTableDetail, setSelectedTableDetail] = useState<DataTableDetail | null>(null)
  const [dataRows, setDataRows] = useState<DataRow[]>([])
  const [dataTableName, setDataTableName] = useState('todos')
  const [dataDisplayName, setDataDisplayName] = useState('Todos')
  const [dataSchemaJson, setDataSchemaJson] = useState(`{
  "fields": {
    "title": { "type": "string", "required": true, "max_length": 200 },
    "done": { "type": "boolean", "required": false, "default": false }
  }
}`)
  const [dataPolicyMode, setDataPolicyMode] = useState('owner_private')
  const [dataTitleFilter, setDataTitleFilter] = useState('')
  const [dataDoneFilter, setDataDoneFilter] = useState<'all' | 'true' | 'false'>('all')
  const [dataFilterField, setDataFilterField] = useState('')
  const [dataFilterOp, setDataFilterOp] = useState('contains')
  const [dataFilterValue, setDataFilterValue] = useState('')
  const [dataOrderBy, setDataOrderBy] = useState('created_at')
  const [dataOrder, setDataOrder] = useState<'asc' | 'desc'>('desc')
  const [dataLimit, setDataLimit] = useState('10')
  const [selectedRowId, setSelectedRowId] = useState<string | null>(null)
  const [selectedRowJson, setSelectedRowJson] = useState(`{
  "title": "buy milk"
}`)
  const [webPushEndpoint, setWebPushEndpoint] = useState('')
  const [webPushP256dh, setWebPushP256dh] = useState('')
  const [webPushAuth, setWebPushAuth] = useState('')
  const [vapidPublicKey, setVapidPublicKey] = useState('')
  const [newRowJson, setNewRowJson] = useState(`{
  "title": "buy milk"
}`)
  const [busyAction, setBusyAction] = useState<string | null>(null)
  const [activeView, setActiveView] = useState<ConsoleView>('overview')
  const [storageSource, setStorageSource] = useState<StorageSource>('server')
  const [storageSearch, setStorageSearch] = useState('')
  const [dataInspectorMode, setDataInspectorMode] = useState<DataInspectorMode>('rows')
  const [pushTab, setPushTab] = useState<PushTab>('composer')

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

    const queueSummary = buildPushSummary(queueData.items)
    const subscriptionSummary = summarizeSubscriptionKinds(subscriptionData.subscriptions)
    setPushStatus(
      `subscription ${subscriptionSummary.total}개 · ntfy ${subscriptionSummary.ntfy} · web push ${subscriptionSummary.webPush} · pending ${queueSummary.pending} · failed ${queueSummary.failed}`,
    )
  }, [])

  const refreshVapidPublicKey = useCallback(async (currentToken: string) => {
    const response = await fetch('/api/push/vapid-public-key', {
      headers: authHeaders(currentToken),
    })
    if (!response.ok) {
      setVapidPublicKey('')
      return
    }
    const data = await readJsonOrThrow<VapidPublicKeyResponse>(response)
    setVapidPublicKey(data.public_key)
  }, [])

  const refreshDataRows = useCallback(async (currentToken: string, tableName: string) => {
    const normalizedTable = tableName.trim()
    if (!normalizedTable) {
      setSelectedTableDetail(null)
      setDataRows([])
      return
    }

    const query = new URLSearchParams()
    const trimmedTitle = dataTitleFilter.trim()
    if (trimmedTitle) query.set('title_contains', trimmedTitle)
    if (dataDoneFilter !== 'all') query.set('done', String(dataDoneFilter === 'true'))
    if (dataFilterField.trim() && dataFilterValue.trim()) {
      query.set('filter_field', dataFilterField.trim())
      query.set('filter_op', dataFilterOp)
      query.set('filter_value', dataFilterValue.trim())
    }
    if (dataOrderBy) query.set('order_by', dataOrderBy)
    if (dataOrder) query.set('order', dataOrder)
    if (dataLimit.trim()) query.set('limit', dataLimit.trim())
    const rowUrl = `/api/data/tables/${encodeURIComponent(normalizedTable)}/rows${query.toString() ? `?${query.toString()}` : ''}`

    const [tableResponse, rowsResponse] = await Promise.all([
      fetch(`/api/data/tables/${encodeURIComponent(normalizedTable)}`, {
        headers: authHeaders(currentToken),
      }),
      fetch(rowUrl, {
        headers: authHeaders(currentToken),
      }),
    ])

    const tableData = await readJsonOrThrow<DataTableResponse>(tableResponse)
    const rowsData = await readJsonOrThrow<DataRowsResponse>(rowsResponse)
    setSelectedTableDetail(tableData.table)
    setDataDisplayName(tableData.table.display_name)
    setDataPolicyMode(tableData.table.access_policy.mode)
    setDataSchemaJson(JSON.stringify(tableData.table.schema, null, 2))
    setDataRows(rowsData.rows)
    setDataStatus(`table ${tableData.table.name} · rows ${rowsData.rows.length}건`)
  }, [dataDoneFilter, dataFilterField, dataFilterOp, dataFilterValue, dataLimit, dataOrder, dataOrderBy, dataTitleFilter])

  const refreshDataTables = useCallback(
    async (currentToken: string, preferredTable?: string) => {
      const response = await fetch('/api/data/tables', {
        headers: authHeaders(currentToken),
      })
      const data = await readJsonOrThrow<DataTablesResponse>(response)
      setDataTables(data.tables)

      const tableNames = new Set(data.tables.map((table) => table.name))
      const requestedTable = preferredTable?.trim() || selectedTable.trim()
      const nextTable = requestedTable && tableNames.has(requestedTable)
        ? requestedTable
        : data.tables[0]?.name || ''

      if (!nextTable) {
        setSelectedTable('')
        setSelectedTableDetail(null)
        setDataRows([])
        setDataStatus('등록된 data table이 없어. admin으로 먼저 생성해줘.')
        return
      }

      setSelectedTable(nextTable)
      await refreshDataRows(currentToken, nextTable)
    },
    [refreshDataRows, selectedTable],
  )

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
        refreshVapidPublicKey(currentToken),
        refreshDataTables(currentToken),
        data.user.is_admin
          ? refreshAdminUsers(currentToken)
          : Promise.resolve().then(() => {
              setAdminUsers([])
              setAdminStatus('현재 사용자는 admin이 아니야.')
            }),
      ])
    },
    [refreshAdminUsers, refreshDataTables, refreshPushData, refreshStorageList, refreshVapidPublicKey],
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
  const pushQueueSummary = useMemo<{
    total: number
    pending: number
    sent: number
    failed: number
    other: number
  }>(() => buildPushSummary(queueItems), [queueItems])
  const pushSubscriptionSummary = useMemo<{
    total: number
    ntfy: number
    webPush: number
    other: number
  }>(() => summarizeSubscriptionKinds(subscriptions), [subscriptions])
  const failedQueueItems = useMemo<PushQueueEntry[]>(() => recentFailedItems(queueItems, 3) as PushQueueEntry[], [queueItems])
  const filteredStorageKeys = useMemo(() => {
    const sourceKeys = storageSource === 'recent' ? recentKeys : storageKeys
    const keyword = storageSearch.trim().toLowerCase()
    if (!keyword) {
      return sourceKeys
    }
    return sourceKeys.filter((item) => item.toLowerCase().includes(keyword))
  }, [recentKeys, storageKeys, storageSearch, storageSource])
  const selectedRowPreview = useMemo(() => dataRows.find((row) => row.id === selectedRowId) ?? null, [dataRows, selectedRowId])
  const selectedTableFieldCount = useMemo(
    () => Object.keys(selectedTableDetail?.schema.fields ?? {}).length,
    [selectedTableDetail],
  )

  const viewMeta: Record<ConsoleView, { eyebrow: string; title: string; description: string }> = {
    overview: {
      eyebrow: 'Control center',
      title: 'Platform overview',
      description: '핵심 상태, 리스크, 빠른 액션을 한 화면에서 보는 운영 대시보드.',
    },
    auth: {
      eyebrow: 'Access',
      title: 'Auth & session',
      description: '회원가입, 로그인, 메모리 세션 토큰 상태를 관리해.',
    },
    data: {
      eyebrow: 'Data API',
      title: 'Schema & rows',
      description: 'logical table, 정책, row 조회/편집을 explorer처럼 다뤄.',
    },
    storage: {
      eyebrow: 'Object storage',
      title: 'Keys & editor',
      description: '현재 사용자 스코프의 object key를 탐색하고 내용을 편집해.',
    },
    push: {
      eyebrow: 'Delivery',
      title: 'Push channels',
      description: 'ntfy/Web Push 등록과 전송 큐 상태를 함께 본다.',
    },
    admin: {
      eyebrow: 'Admin',
      title: 'User approval',
      description: 'pending user 승인과 활성 사용자 현황을 관리해.',
    },
  }
  const navigationItems: Array<{ id: ConsoleView; label: string; badge?: string }> = [
    { id: 'overview', label: 'Overview' },
    { id: 'auth', label: 'Auth' },
    { id: 'data', label: 'Data', badge: String(dataTables.length) },
    { id: 'storage', label: 'Storage', badge: String(storageKeys.length) },
    { id: 'push', label: 'Push', badge: pushQueueSummary.failed > 0 ? String(pushQueueSummary.failed) : undefined },
    { id: 'admin', label: 'Admin', badge: pendingUsers.length > 0 ? String(pendingUsers.length) : undefined },
  ]
  const sectionSidebar = {
    overview: { title: 'Project', group: 'Overview', items: ['Home', 'Project', 'Usage'] },
    auth: { title: 'Auth', group: 'Manage', items: ['Users', 'Sessions', 'Access'] },
    data: { title: 'Database', group: 'Manage', items: ['Tables', 'Rows', 'Policies'] },
    storage: { title: 'Storage', group: 'Manage', items: ['Objects', 'Keys', 'Editor'] },
    push: { title: 'Edge & Push', group: 'Manage', items: ['Composer', 'Subscriptions', 'Deliveries'] },
    admin: { title: 'Admin', group: 'Manage', items: ['Pending users', 'Active users'] },
  }[activeView]

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
    setDataTables([])
    setSelectedTableDetail(null)
    setDataRows([])
    setDataStatus('data 작업 대기 중')
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

  async function refreshSelectedTable() {
    if (!token) {
      setDataStatus('먼저 로그인해줘.')
      return
    }
    setBusyAction('refreshData')
    try {
      await refreshDataTables(token, selectedTable)
    } catch (error) {
      setDataStatus(error instanceof Error ? error.message : 'data refresh failed')
    } finally {
      setBusyAction(null)
    }
  }

  async function createDataTable() {
    if (!token) {
      setDataStatus('먼저 로그인해줘.')
      return
    }
    setBusyAction('createDataTable')
    try {
      const parsedSchema = JSON.parse(dataSchemaJson) as { fields: Record<string, unknown> }
      const response = await fetch('/api/data/tables', {
        method: 'POST',
        headers: {
          ...authHeaders(token),
          'content-type': 'application/json',
        },
        body: JSON.stringify({
          name: dataTableName,
          display_name: dataDisplayName,
          schema: parsedSchema,
          access_policy: { mode: dataPolicyMode },
        }),
      })
      const data = await readJsonOrThrow<DataTableResponse>(response)
      setSelectedTable(data.table.name)
      setDataStatus(`table ${data.table.name} 생성 완료`)
      await refreshDataTables(token, data.table.name)
    } catch (error) {
      setDataStatus(error instanceof Error ? error.message : 'table create failed')
    } finally {
      setBusyAction(null)
    }
  }

  async function updateSelectedTable() {
    if (!token) {
      setDataStatus('먼저 로그인해줘.')
      return
    }
    const tableName = selectedTable.trim()
    if (!tableName) {
      setDataStatus('먼저 수정할 table을 선택해줘.')
      return
    }

    setBusyAction('updateDataTable')
    try {
      const parsedSchema = JSON.parse(dataSchemaJson) as { fields: Record<string, unknown> }
      const response = await fetch(`/api/data/tables/${encodeURIComponent(tableName)}`, {
        method: 'PATCH',
        headers: {
          ...authHeaders(token),
          'content-type': 'application/json',
        },
        body: JSON.stringify({
          display_name: dataDisplayName,
          schema: parsedSchema,
          access_policy: { mode: dataPolicyMode },
        }),
      })
      const data = await readJsonOrThrow<DataTableResponse>(response)
      setDataStatus(`table ${data.table.name} 수정 완료`)
      setSelectedTableDetail(data.table)
      setDataSchemaJson(JSON.stringify(data.table.schema, null, 2))
      setDataDisplayName(data.table.display_name)
      setDataPolicyMode(data.table.access_policy.mode)
      await refreshDataTables(token, data.table.name)
    } catch (error) {
      setDataStatus(error instanceof Error ? error.message : 'table update failed')
    } finally {
      setBusyAction(null)
    }
  }

  async function deleteSelectedTable() {
    if (!token) {
      setDataStatus('먼저 로그인해줘.')
      return
    }
    const tableName = selectedTable.trim()
    if (!tableName) {
      setDataStatus('먼저 삭제할 table을 선택해줘.')
      return
    }

    setBusyAction('deleteDataTable')
    try {
      const response = await fetch(`/api/data/tables/${encodeURIComponent(tableName)}`, {
        method: 'DELETE',
        headers: authHeaders(token),
      })
      const data = await readJsonOrThrow<MessageResponse>(response)
      setDataStatus(data.message)
      setSelectedTable('')
      setSelectedTableDetail(null)
      setDataRows([])
      await refreshDataTables(token, '')
    } catch (error) {
      setDataStatus(error instanceof Error ? error.message : 'table delete failed')
    } finally {
      setBusyAction(null)
    }
  }

  async function createDataRow() {
    if (!token) {
      setDataStatus('먼저 로그인해줘.')
      return
    }
    const tableName = selectedTable.trim()
    if (!tableName) {
      setDataStatus('먼저 data table을 선택해줘.')
      return
    }

    setBusyAction('createDataRow')
    try {
      const parsed = JSON.parse(newRowJson) as Record<string, unknown>
      const response = await fetch(`/api/data/tables/${encodeURIComponent(tableName)}/rows`, {
        method: 'POST',
        headers: {
          ...authHeaders(token),
          'content-type': 'application/json',
        },
        body: JSON.stringify({ data: parsed }),
      })
      const data = await readJsonOrThrow<DataRow>(response)
      setDataStatus(`row ${data.id.slice(0, 8)} 생성 완료`)
      setSelectedRowId(data.id)
      setSelectedRowJson(JSON.stringify(data.data, null, 2))
      await refreshDataRows(token, tableName)
    } catch (error) {
      setDataStatus(error instanceof Error ? error.message : 'row create failed')
    } finally {
      setBusyAction(null)
    }
  }

  async function saveSelectedRow() {
    if (!token) {
      setDataStatus('먼저 로그인해줘.')
      return
    }
    const tableName = selectedTable.trim()
    if (!tableName || !selectedRowId) {
      setDataStatus('먼저 수정할 row를 선택해줘.')
      return
    }

    setBusyAction('saveSelectedRow')
    try {
      const parsed = JSON.parse(selectedRowJson) as Record<string, unknown>
      const response = await fetch(`/api/data/tables/${encodeURIComponent(tableName)}/rows/${encodeURIComponent(selectedRowId)}`, {
        method: 'PATCH',
        headers: {
          ...authHeaders(token),
          'content-type': 'application/json',
        },
        body: JSON.stringify({ data: parsed }),
      })
      const data = await readJsonOrThrow<DataRow>(response)
      setDataStatus(`row ${data.id.slice(0, 8)} 수정 완료`)
      setSelectedRowJson(JSON.stringify(data.data, null, 2))
      await refreshDataRows(token, tableName)
    } catch (error) {
      setDataStatus(error instanceof Error ? error.message : 'row update failed')
    } finally {
      setBusyAction(null)
    }
  }

  async function deleteSelectedRow() {
    if (!token) {
      setDataStatus('먼저 로그인해줘.')
      return
    }
    const tableName = selectedTable.trim()
    if (!tableName || !selectedRowId) {
      setDataStatus('먼저 삭제할 row를 선택해줘.')
      return
    }

    setBusyAction('deleteSelectedRow')
    try {
      const response = await fetch(`/api/data/tables/${encodeURIComponent(tableName)}/rows/${encodeURIComponent(selectedRowId)}`, {
        method: 'DELETE',
        headers: authHeaders(token),
      })
      const data = await readJsonOrThrow<MessageResponse>(response)
      setDataStatus(data.message)
      setSelectedRowId(null)
      setSelectedRowJson(newRowJson)
      await refreshDataRows(token, tableName)
    } catch (error) {
      setDataStatus(error instanceof Error ? error.message : 'row delete failed')
    } finally {
      setBusyAction(null)
    }
  }

  async function registerBrowserWebPush() {
    if (!token) {
      setPushStatus('먼저 로그인해줘.')
      return
    }
    if (typeof window === 'undefined' || !('serviceWorker' in navigator)) {
      setPushStatus('이 브라우저는 service worker를 지원하지 않아.')
      return
    }
    if (!vapidPublicKey.trim()) {
      setPushStatus('VAPID public key를 먼저 입력해줘.')
      return
    }

    setBusyAction('registerBrowserWebPush')
    try {
      const permission = await window.Notification.requestPermission()
      if (permission !== 'granted') {
        throw new Error('브라우저 알림 권한이 필요해.')
      }

      const registration = await navigator.serviceWorker.register('/peanut-sw.js')
      const subscription = await registration.pushManager.subscribe({
        userVisibleOnly: true,
        applicationServerKey: base64UrlToArrayBuffer(vapidPublicKey.trim()),
      })
      const json = subscription.toJSON()
      const response = await fetch('/api/push/subscriptions', {
        method: 'POST',
        headers: {
          ...authHeaders(token),
          'content-type': 'application/json',
        },
        body: JSON.stringify({
          endpoint: json.endpoint,
          keys: json.keys,
        }),
      })
      const data = await readJsonOrThrow<MessageResponse>(response)
      setWebPushEndpoint(json.endpoint ?? '')
      setWebPushP256dh(json.keys?.p256dh ?? '')
      setWebPushAuth(json.keys?.auth ?? '')
      setPushStatus(data.message)
      await refreshPushData(token)
    } catch (error) {
      setPushStatus(error instanceof Error ? error.message : 'browser web push registration failed')
    } finally {
      setBusyAction(null)
    }
  }

  async function saveManualWebPushSubscription() {
    if (!token) {
      setPushStatus('먼저 로그인해줘.')
      return
    }
    setBusyAction('saveManualWebPushSubscription')
    try {
      const response = await fetch('/api/push/subscriptions', {
        method: 'POST',
        headers: {
          ...authHeaders(token),
          'content-type': 'application/json',
        },
        body: JSON.stringify({
          endpoint: webPushEndpoint,
          keys: {
            p256dh: webPushP256dh,
            auth: webPushAuth,
          },
        }),
      })
      const data = await readJsonOrThrow<MessageResponse>(response)
      setPushStatus(data.message)
      await refreshPushData(token)
    } catch (error) {
      setPushStatus(error instanceof Error ? error.message : 'manual web push subscription failed')
    } finally {
      setBusyAction(null)
    }
  }

  return (
    <div className="min-h-screen overflow-x-hidden bg-[#f6f8fb] text-slate-900">
      <div className="pointer-events-none fixed inset-0 overflow-hidden">
        <div className="absolute inset-0 bg-[linear-gradient(180deg,#f8fafc_0%,#f6f8fb_100%)]" />
      </div>

      <div className="relative mx-auto min-h-screen w-full max-w-[1680px] px-3 py-3 sm:px-4">
        <header className="sticky top-3 z-20 flex items-center gap-3 rounded-[14px] border border-slate-200 bg-white/95 px-4 py-3 shadow-[0_1px_2px_rgba(15,23,42,0.04)] backdrop-blur">
          <div className="flex h-8 w-8 items-center justify-center rounded-[10px] bg-emerald-500 text-xs font-semibold text-white">P</div>
          <div className="min-w-0 flex items-center gap-2 text-sm text-slate-500">
            <span className="truncate text-slate-900">parkjangwon</span>
            <Badge tone="default">FREE</Badge>
            <span>/</span>
            <span className="truncate text-slate-900">peanut</span>
            <span>/</span>
            <span className="truncate text-slate-900">main</span>
            <Badge tone="warning">PRODUCTION</Badge>
          </div>
          <div className="ml-auto flex items-center gap-2">
            <ActionButton onClick={() => token && void refreshSession(token)} primary={!token}>{token ? 'Refresh' : 'Connect'}</ActionButton>
            <div className="hidden md:flex min-w-[220px] items-center rounded-[10px] border border-slate-200 bg-slate-50 px-3 py-2 text-sm text-slate-400">Search…</div>
          </div>
        </header>

        <div className="mt-3 flex min-h-[calc(100vh-4.5rem)] gap-3">
          <aside className="hidden w-[64px] shrink-0 rounded-[14px] border border-slate-200 bg-white p-2 shadow-[0_1px_2px_rgba(15,23,42,0.04)] lg:flex lg:flex-col lg:items-center lg:gap-2">
            {navigationItems.map((item) => (
              <button
                key={item.id}
                aria-label={item.label}
                className={[
                  'flex h-10 w-10 cursor-pointer items-center justify-center rounded-[10px] border text-[11px] font-semibold transition duration-200',
                  activeView === item.id
                    ? 'border-slate-300 bg-slate-100 text-slate-900'
                    : 'border-transparent bg-transparent text-slate-400 hover:border-slate-200 hover:bg-slate-50 hover:text-slate-700',
                ].join(' ')}
                onClick={() => setActiveView(item.id)}
                type="button"
              >
                {item.label.slice(0,1)}
              </button>
            ))}
            <div className="mt-auto flex w-full flex-col items-center gap-2 pt-2">
              <div className="h-px w-8 bg-slate-200" />
              <MiniIconStat label="H" value={health?.status === 'ok' ? '●' : '○'} />
              <MiniIconStat label="S" value={token ? '●' : '○'} />
            </div>
          </aside>

          <main className="min-w-0 flex-1 rounded-[14px] border border-slate-200 bg-white shadow-[0_1px_2px_rgba(15,23,42,0.04)]">
            <div className="border-b border-slate-200 px-5 py-4">
              <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
                <div>
                  <p className="text-[11px] uppercase tracking-[0.24em] text-slate-400">{viewMeta[activeView].eyebrow}</p>
                  <h1 className="mt-1 text-2xl font-semibold tracking-tight text-slate-900">{viewMeta[activeView].title}</h1>
                  <p className="mt-2 text-sm text-slate-500">{viewMeta[activeView].description}</p>
                </div>
                <div className="flex flex-wrap items-center gap-2">
                  <Badge tone="default">workspace {activeView}</Badge>
                  <Badge tone={token ? 'success' : 'default'}>{token ? 'connected' : 'anonymous'}</Badge>
                  <Badge tone={selectedTable ? 'info' : 'default'}>{selectedTable || 'no table selected'}</Badge>
                </div>
              </div>
            </div>

            <div className="mt-4 grid gap-3 sm:grid-cols-2 lg:hidden">
              <StatusTile label="Live user" value={session?.user.email ?? 'anonymous'} detail={session?.user.id ?? '세션 없음'} />
              <StatusTile label="Selected table" value={selectedTableDetail?.display_name ?? 'none'} detail={selectedTableDetail?.access_policy.mode ?? 'policy n/a'} />
            </div>

            <div className="mt-5 grid gap-5 p-5 lg:grid-cols-[220px_minmax(0,1fr)]">
              <aside className="hidden lg:block">
                <div className="rounded-[12px] border border-slate-200 bg-white p-4">
                  <h2 className="text-lg font-semibold text-slate-900">{sectionSidebar.title}</h2>
                  <div className="mt-6">
                    <p className="text-[10px] uppercase tracking-[0.24em] text-slate-400">{sectionSidebar.group}</p>
                    <div className="mt-3 grid gap-1.5">
                      {sectionSidebar.items.map((item, index) => (
                        <div
                          key={item}
                          className={[
                            'rounded-[10px] px-3 py-2.5 text-sm',
                            index === 0 ? 'bg-slate-100 font-medium text-slate-900' : 'text-slate-600',
                          ].join(' ')}
                        >
                          {item}
                        </div>
                      ))}
                    </div>
                  </div>
                </div>
              </aside>

              <div className="grid gap-5 xl:grid-cols-[minmax(0,1.62fr)_320px]">
              <section className="min-w-0 grid gap-5">
                {activeView === 'overview' ? (
                  <>
                    <div className="grid gap-5 xl:grid-cols-[minmax(0,1.15fr)_minmax(340px,0.85fr)]">
                      <div className="grid gap-4">
                        <Surface title="Project overview" subtitle="현재 프로젝트 상태와 베이스 리소스를 요약">
                          <div className="flex flex-col gap-5 xl:flex-row xl:items-start xl:justify-between">
                            <div>
                              <div className="flex items-center gap-2">
                                <h3 className="text-3xl font-semibold tracking-tight text-slate-900">peanut</h3>
                                <Badge tone="default">NANO</Badge>
                              </div>
                              <div className="mt-4 flex items-center gap-3 text-sm text-slate-500">
                                <span>Project URL unavailable</span>
                                <ActionButton onClick={() => navigator.clipboard?.writeText('http://127.0.0.1:3022')}>Copy</ActionButton>
                              </div>
                            </div>
                            <div className="grid min-w-[260px] gap-3 sm:grid-cols-2 xl:w-[320px]">
                              <StatusTile label="Live user" value={session?.user.email ?? 'anonymous'} detail={session?.user.id ?? '세션 없음'} />
                              <StatusTile label="Selected table" value={selectedTableDetail?.display_name ?? 'none'} detail={selectedTableDetail?.access_policy.mode ?? 'policy n/a'} />
                            </div>
                          </div>

                          <div className="mt-6 grid gap-2.5 lg:max-w-[720px]">
                            <OverviewStatRow label="STATUS" value={health?.status === 'ok' ? 'Healthy' : health?.status ?? 'Checking...'} detail={healthError ?? '서비스 상태를 확인 중'} />
                            <OverviewStatRow label="LAST MIGRATION" value={selectedTableDetail?.name ?? 'No migration yet'} detail={selectedTableDetail ? `${selectedTableFieldCount} fields · ${selectedTableDetail.created_at}` : '최근 schema 변경 내역 없음'} />
                            <OverviewStatRow label="LAST BACKUP" value={queueItems.length > 0 ? queueItems[0].created_at : 'No backups'} detail={queueItems.length > 0 ? '가장 최근 작업 기준' : '백업 기록이 아직 없어'} />
                            <OverviewStatRow label="RECENT BRANCH" value={selectedTable || 'main'} detail={selectedTable ? '현재 활성 데이터 컨텍스트' : '분기 정보 없음'} />
                          </div>
                        </Surface>
                      </div>

                      <Surface title="Infrastructure" subtitle="리전 및 주요 인프라 footprint">
                        <div className="rounded-[16px] border border-dashed border-slate-200 bg-[radial-gradient(circle_at_1px_1px,#e2e8f0_1px,transparent_0)] [background-size:18px_18px] p-6 min-h-[360px] flex items-center">
                          <div className="mx-auto max-w-[360px] rounded-[16px] border border-slate-200 bg-white p-4 shadow-[0_8px_30px_rgba(15,23,42,0.06)]">
                            <div className="flex items-start justify-between gap-3">
                              <div className="flex items-start gap-3">
                                <div className="flex h-10 w-10 items-center justify-center rounded-[12px] bg-emerald-50 text-sm font-semibold text-emerald-700">DB</div>
                                <div>
                                  <p className="font-medium text-slate-900">Primary Database</p>
                                  <p className="mt-1 text-sm text-slate-500">Seoul Region</p>
                                  <p className="mt-1 text-xs text-slate-400">ap-northeast-2 · peanut.nano</p>
                                </div>
                              </div>
                              <div className="rounded-full border border-slate-200 px-2 py-1 text-xs text-slate-500">KR</div>
                            </div>
                          </div>
                        </div>
                      </Surface>
                    </div>

                    <Surface title={`${pushQueueSummary.total} Total Requests`} subtitle="Last 60 minutes 기준 주요 surface usage">
                      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
                        <RequestCard label="DATABASE REQUESTS" value={String(dataRows.length)} />
                        <RequestCard label="AUTH REQUESTS" value={token ? '1' : '0'} />
                        <RequestCard label="STORAGE REQUESTS" value={String(storageKeys.length)} />
                        <RequestCard label="PUSH REQUESTS" value={String(queueItems.length)} />
                      </div>
                    </Surface>
                  </>
                ) : null}

                {activeView === 'auth' ? (
                  <div className="grid gap-4">
                    <Surface title="Users & Access" subtitle="사용자 등록, 로그인, 세션 연결 흐름을 한 곳에서 관리">
                      <div className="flex flex-wrap gap-2">
                        <ActionButton onClick={() => void register()} busy={busyAction === 'register'}>Create user</ActionButton>
                        <ActionButton onClick={() => void login()} busy={busyAction === 'login'} primary>Open session</ActionButton>
                        <ActionButton onClick={logout}>Clear session</ActionButton>
                        <ActionButton onClick={() => token && void refreshSession(token)}>Refresh</ActionButton>
                      </div>

                      <div className="mt-4 grid gap-4 xl:grid-cols-[minmax(0,1fr)_320px]">
                        <div className="grid gap-4 rounded-[16px] border border-slate-200 bg-white p-4">
                          <div className="grid gap-4 md:grid-cols-2">
                            <Field label="Email">
                              <input className={inputClassName} name="email" autoComplete="email" type="email" spellCheck={false} onChange={(event) => setEmail(event.target.value)} value={email} />
                            </Field>
                            <Field label="Password">
                              <input className={inputClassName} name="password" autoComplete="current-password" type="password" onChange={(event) => setPassword(event.target.value)} value={password} />
                            </Field>
                          </div>
                          <StatusBanner label="Auth status" tone={token ? 'success' : 'default'} value={authStatus} />
                          {sessionError ? <StatusBanner label="Session error" tone="danger" value={sessionError} /> : null}
                        </div>

                        <div className="grid gap-4">
                          <InfoStat label="Role" value={session?.user.is_admin ? 'admin' : token ? 'member' : 'guest'} detail={session?.user.email ?? '로그인 전'} />
                          <div className="rounded-[16px] border border-slate-200 bg-white p-4">
                            <p className="text-[10px] uppercase tracking-[0.24em] text-slate-400">Token preview</p>
                            <p className="mt-3 break-all font-mono text-xs leading-6 text-slate-600">{token ? `${token.slice(0, 24)}… (메모리 전용)` : '토큰 없음'}</p>
                          </div>
                        </div>
                      </div>
                    </Surface>
                  </div>
                ) : null}

                {activeView === 'storage' ? (
                  <div className="grid gap-4">
                    <Surface title="Storage" subtitle="object browser와 editor를 나눠서 관리">
                      <div className="flex flex-wrap gap-2">
                        <TabButton active={storageSource === 'server'} onClick={() => setStorageSource('server')}>Server</TabButton>
                        <TabButton active={storageSource === 'recent'} onClick={() => setStorageSource('recent')}>Recent</TabButton>
                        <ActionButton onClick={() => token && void refreshStorageList(token)}>Refresh keys</ActionButton>
                      </div>

                      <div className="mt-5 grid gap-4 xl:grid-cols-[280px_minmax(0,1fr)]">
                        <div className="rounded-[16px] border border-slate-200 bg-white p-4">
                          <Field label="Search key">
                            <input className={inputClassName} value={storageSearch} onChange={(event) => setStorageSearch(event.target.value)} placeholder="Search key" />
                          </Field>
                          <div className="mt-4 overflow-hidden rounded-[14px] border border-slate-200 bg-white">
                            <div className="border-b border-slate-200 px-4 py-3 text-[10px] uppercase tracking-[0.24em] text-slate-400">{storageSource === 'server' ? 'Stored keys' : 'Recent shortcuts'}</div>
                            <div className="grid gap-px bg-slate-200">
                              {filteredStorageKeys.length === 0 ? (
                                <div className="bg-white p-4"><EmptyState text={storageSource === 'server' ? '저장된 object key가 아직 없어.' : '최근에 다룬 key가 아직 없어.'} /></div>
                              ) : (
                                filteredStorageKeys.map((key) => (
                                  <button
                                    key={key}
                                    className={[
                                      'cursor-pointer bg-white px-4 py-3 text-left text-sm transition duration-200 hover:bg-slate-50',
                                      storageKey === key ? 'bg-emerald-50 text-slate-900' : 'text-slate-600',
                                    ].join(' ')}
                                    onClick={() => {
                                      setStorageKey(key)
                                      void loadObject(key)
                                    }}
                                    type="button"
                                  >
                                    <span className="block truncate">{key}</span>
                                  </button>
                                ))
                              )}
                            </div>
                          </div>
                        </div>

                        <div className="rounded-[16px] border border-slate-200 bg-white p-4">
                          <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_180px]">
                            <Field label="Object key">
                              <input className={inputClassName} value={storageKey} onChange={(event) => setStorageKey(event.target.value)} />
                            </Field>
                            <Field label="Recent keys">
                              <div className="rounded-[12px] border border-slate-200 bg-slate-50 px-4 py-2.5 text-sm text-slate-600">{recentKeys.length}</div>
                            </Field>
                          </div>
                          <div className="mt-4 flex flex-wrap gap-2.5">
                            <ActionButton busy={busyAction === 'saveObject'} onClick={() => void saveObject()} primary>Save object</ActionButton>
                            <ActionButton busy={busyAction === 'loadObject'} onClick={() => void loadObject()}>Load object</ActionButton>
                            <ActionButton busy={busyAction === 'deleteObject'} onClick={() => void deleteObject()}>Delete object</ActionButton>
                          </div>
                          <div className="mt-4">
                            <StatusBanner label="Storage status" tone={storageStatus.includes('완료') ? 'success' : 'default'} value={storageStatus} />
                          </div>
                          <div className="mt-4">
                            <Field label="Content">
                              <textarea className={textareaClassName} value={storageBody} onChange={(event) => setStorageBody(event.target.value)} />
                            </Field>
                          </div>
                        </div>
                      </div>
                    </Surface>
                  </div>
                ) : null}

                {activeView === 'data' ? (
                  <div className="grid gap-4">
                    <Surface title="Database" subtitle="logical tables, row explorer, inspector를 콘솔형으로 관리">
                      <div className="flex flex-wrap gap-2">
                        <ActionButton busy={busyAction === 'createDataTable'} onClick={() => void createDataTable()} primary>Create table</ActionButton>
                        <ActionButton busy={busyAction === 'updateDataTable'} onClick={() => void updateSelectedTable()}>Update table</ActionButton>
                        <ActionButton busy={busyAction === 'deleteDataTable'} onClick={() => void deleteSelectedTable()}>Delete table</ActionButton>
                        <ActionButton busy={busyAction === 'refreshData'} onClick={() => void refreshSelectedTable()}>Refresh</ActionButton>
                      </div>

                      <div className="mt-5 grid gap-4 xl:grid-cols-[280px_minmax(0,1fr)_320px]">
                        <div className="rounded-[16px] border border-slate-200 bg-white p-4">
                          <div className="grid gap-3">
                            <Field label="Table name">
                              <input className={inputClassName} value={dataTableName} onChange={(event) => setDataTableName(event.target.value)} />
                            </Field>
                            <Field label="Display name">
                              <input className={inputClassName} value={dataDisplayName} onChange={(event) => setDataDisplayName(event.target.value)} />
                            </Field>
                            <Field label="Access policy">
                              <select className={inputClassName} value={dataPolicyMode} onChange={(event) => setDataPolicyMode(event.target.value)}>
                                <option value="owner_private">owner_private</option>
                                <option value="admin_only">admin_only</option>
                                <option value="authenticated_shared_rw">authenticated_shared_rw</option>
                              </select>
                            </Field>
                          </div>
                          <div className="mt-4 overflow-hidden rounded-[14px] border border-slate-200 bg-white">
                            <div className="border-b border-slate-200 px-4 py-3 text-[10px] uppercase tracking-[0.24em] text-slate-400">Tables</div>
                            <div className="grid gap-px bg-slate-200">
                              {dataTables.length === 0 ? (
                                <div className="bg-white p-4"><EmptyState text="등록된 table이 없어. admin으로 sample table을 먼저 만들 수 있어." /></div>
                              ) : (
                                dataTables.map((table) => (
                                  <button
                                    key={table.name}
                                    className={[
                                      'cursor-pointer bg-white px-4 py-3 text-left transition duration-200 hover:bg-slate-50',
                                      selectedTable === table.name ? 'bg-emerald-50' : '',
                                    ].join(' ')}
                                    onClick={() => {
                                      setSelectedTable(table.name)
                                      setDataDisplayName(table.display_name)
                                      setDataPolicyMode(table.policy_mode)
                                      if (token) void refreshDataTables(token, table.name)
                                    }}
                                    type="button"
                                  >
                                    <div className="flex items-center justify-between gap-2">
                                      <span className="font-medium text-slate-900">{table.display_name}</span>
                                      <Badge tone="default">{table.policy_mode}</Badge>
                                    </div>
                                    <p className="mt-1 text-xs text-slate-400">{table.name} · {table.created_at}</p>
                                  </button>
                                ))
                              )}
                            </div>
                          </div>
                          <div className="mt-4">
                            <StatusBanner label="Data status" tone={dataStatus.includes('완료') ? 'success' : 'default'} value={dataStatus} />
                          </div>
                        </div>

                        <div className="rounded-[16px] border border-slate-200 bg-white p-4">
                          <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
                            <Field label="Title contains">
                              <input className={inputClassName} value={dataTitleFilter} onChange={(event) => setDataTitleFilter(event.target.value)} />
                            </Field>
                            <Field label="Done filter">
                              <select className={inputClassName} value={dataDoneFilter} onChange={(event) => setDataDoneFilter(event.target.value as 'all' | 'true' | 'false')}>
                                <option value="all">all</option>
                                <option value="false">false</option>
                                <option value="true">true</option>
                              </select>
                            </Field>
                            <Field label="Field">
                              <input className={inputClassName} value={dataFilterField} onChange={(event) => setDataFilterField(event.target.value)} />
                            </Field>
                            <Field label="Operator">
                              <select className={inputClassName} value={dataFilterOp} onChange={(event) => setDataFilterOp(event.target.value)}>
                                <option value="contains">contains</option>
                                <option value="eq">eq</option>
                                <option value="ne">ne</option>
                                <option value="gt">gt</option>
                                <option value="gte">gte</option>
                                <option value="lt">lt</option>
                                <option value="lte">lte</option>
                              </select>
                            </Field>
                            <Field label="Value">
                              <input className={inputClassName} value={dataFilterValue} onChange={(event) => setDataFilterValue(event.target.value)} />
                            </Field>
                            <Field label="Order / limit">
                              <div className="grid gap-2 sm:grid-cols-[1fr_1fr_84px]">
                                <select className={inputClassName} value={dataOrderBy} onChange={(event) => setDataOrderBy(event.target.value)}>
                                  <option value="created_at">created_at</option>
                                  <option value="updated_at">updated_at</option>
                                  <option value="title">title</option>
                                  <option value="done">done</option>
                                </select>
                                <select className={inputClassName} value={dataOrder} onChange={(event) => setDataOrder(event.target.value as 'asc' | 'desc')}>
                                  <option value="desc">desc</option>
                                  <option value="asc">asc</option>
                                </select>
                                <input className={inputClassName} value={dataLimit} onChange={(event) => setDataLimit(event.target.value)} />
                              </div>
                            </Field>
                          </div>

                          <div className="mt-4 flex flex-wrap gap-2">
                            <TabButton active={dataInspectorMode === 'rows'} onClick={() => setDataInspectorMode('rows')}>Rows</TabButton>
                            <TabButton active={dataInspectorMode === 'schema'} onClick={() => setDataInspectorMode('schema')}>Schema</TabButton>
                          </div>

                          {dataInspectorMode === 'rows' ? (
                            <div className="mt-4 overflow-hidden rounded-[14px] border border-slate-200 bg-white">
                              <div className="grid grid-cols-[1.2fr_0.8fr_1fr] gap-3 border-b border-slate-200 px-4 py-3 text-[10px] uppercase tracking-[0.24em] text-slate-400">
                                <span>Row</span>
                                <span>Owner</span>
                                <span>Preview</span>
                              </div>
                              {dataRows.length === 0 ? (
                                <div className="p-4"><EmptyState text="선택한 table에 아직 row가 없어." /></div>
                              ) : (
                                dataRows.map((row) => (
                                  <button
                                    key={row.id}
                                    className={[
                                      'grid w-full cursor-pointer grid-cols-[1.2fr_0.8fr_1fr] gap-3 border-b border-slate-200 px-4 py-3 text-left transition duration-200 last:border-b-0',
                                      selectedRowId === row.id ? 'bg-emerald-50' : 'hover:bg-slate-50',
                                    ].join(' ')}
                                    onClick={() => {
                                      setSelectedRowId(row.id)
                                      setSelectedRowJson(JSON.stringify(row.data, null, 2))
                                    }}
                                    type="button"
                                  >
                                    <div className="min-w-0">
                                      <p className="truncate font-medium text-slate-900">{row.id}</p>
                                      <p className="mt-1 text-xs text-slate-400">{row.created_at}</p>
                                    </div>
                                    <div className="truncate text-sm text-slate-400">{row.owner_user_id ?? 'shared'}</div>
                                    <div className="truncate font-mono text-xs text-slate-400">{compactJson(row.data, 120)}</div>
                                  </button>
                                ))
                              )}
                            </div>
                          ) : (
                            <div className="mt-4 rounded-[14px] border border-slate-200 bg-white p-4">
                              <Field label="Schema JSON">
                                <textarea className={textareaClassName} value={dataSchemaJson} onChange={(event) => setDataSchemaJson(event.target.value)} />
                              </Field>
                            </div>
                          )}
                        </div>

                        <div className="rounded-[16px] border border-slate-200 bg-white p-4">
                          <div className="grid gap-3">
                            <InfoStat label="Selected table" value={selectedTableDetail?.display_name ?? 'none'} detail={selectedTableDetail?.name ?? 'table 없음'} />
                            <InfoStat label="Fields" value={String(selectedTableFieldCount)} detail={selectedTableDetail?.access_policy.mode ?? 'policy 없음'} />
                            <Field label="New row JSON">
                              <textarea className={smallTextareaClassName} value={newRowJson} onChange={(event) => setNewRowJson(event.target.value)} />
                            </Field>
                            <ActionButton busy={busyAction === 'createDataRow'} onClick={() => void createDataRow()} primary>Create row</ActionButton>
                            <Field label="Selected row JSON">
                              <textarea className={textareaClassName} value={selectedRowJson} onChange={(event) => setSelectedRowJson(event.target.value)} />
                            </Field>
                            <div className="flex flex-wrap gap-2.5">
                              <ActionButton busy={busyAction === 'saveSelectedRow'} onClick={() => void saveSelectedRow()} primary>Save row</ActionButton>
                              <ActionButton busy={busyAction === 'deleteSelectedRow'} onClick={() => void deleteSelectedRow()}>Delete row</ActionButton>
                            </div>
                          </div>
                        </div>
                      </div>
                    </Surface>
                  </div>
                ) : null}

                {activeView === 'push' ? (
                  <div className="grid gap-4">
                    <Surface title="Push channels" subtitle="topic, browser web push, queue delivery를 하나의 워크플로우로 관리">
                      <div className="flex flex-wrap gap-2">
                        <ActionButton onClick={() => token && void refreshPushData(token)}>Refresh data</ActionButton>
                        <ActionButton onClick={() => token && void refreshVapidPublicKey(token)}>Load VAPID key</ActionButton>
                        <ActionButton onClick={() => setPushTab('composer')} primary>Open composer</ActionButton>
                      </div>

                      <div className="mt-5 grid gap-4 xl:grid-cols-3">
                        <SetupOptionCard
                          eyebrow="Via Topic"
                          title="Topic subscription"
                          description="ntfy topic을 즉시 연결해서 단일 테스트 채널을 준비해."
                          actionLabel="Subscribe topic"
                          onAction={() => void subscribeTopic()}
                          busy={busyAction === 'subscribeTopic'}
                        >
                          <Field label="Topic">
                            <input className={inputClassName} value={pushTopic} onChange={(event) => setPushTopic(event.target.value)} />
                          </Field>
                        </SetupOptionCard>

                        <SetupOptionCard
                          eyebrow="Via Browser"
                          title="Browser Web Push"
                          description="브라우저 권한과 VAPID key를 사용해서 endpoint를 자동 등록해."
                          actionLabel="Register browser"
                          onAction={() => void registerBrowserWebPush()}
                          busy={busyAction === 'registerBrowserWebPush'}
                        >
                          <Field label="VAPID public key">
                            <input className={inputClassName} value={vapidPublicKey} onChange={(event) => setVapidPublicKey(event.target.value)} />
                          </Field>
                        </SetupOptionCard>

                        <SetupOptionCard
                          eyebrow="Via API"
                          title="Manual subscription"
                          description="외부 endpoint, p256dh, auth를 수동 입력해서 채널을 저장해."
                          actionLabel="Save manual"
                          onAction={() => void saveManualWebPushSubscription()}
                          busy={busyAction === 'saveManualWebPushSubscription'}
                        >
                          <Field label="Endpoint">
                            <input className={inputClassName} value={webPushEndpoint} onChange={(event) => setWebPushEndpoint(event.target.value)} />
                          </Field>
                        </SetupOptionCard>
                      </div>
                    </Surface>

                    <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_320px]">
                      <Surface title="Composer" subtitle="선택한 채널에 메시지를 보낼 payload를 준비">
                        <div className="grid gap-4 md:grid-cols-2">
                          <Field label="Push title">
                            <input className={inputClassName} value={pushTitle} onChange={(event) => setPushTitle(event.target.value)} />
                          </Field>
                          <Field label="Topic">
                            <input className={inputClassName} value={pushTopic} onChange={(event) => setPushTopic(event.target.value)} />
                          </Field>
                        </div>
                        <div className="mt-4">
                          <Field label="Push body">
                            <textarea className={smallTextareaClassName} value={pushBody} onChange={(event) => setPushBody(event.target.value)} />
                          </Field>
                        </div>
                        <div className="mt-4 flex flex-wrap gap-2.5">
                          <ActionButton busy={busyAction === 'enqueuePush'} onClick={() => void enqueuePush()} primary>Enqueue push</ActionButton>
                          <ActionButton onClick={() => setPushTab('subscriptions')}>View subscriptions</ActionButton>
                          <ActionButton onClick={() => setPushTab('deliveries')}>View deliveries</ActionButton>
                        </div>
                        <div className="mt-4">
                          <StatusBanner label="Push status" tone={pushStatus.includes('failed') ? 'danger' : 'default'} value={pushStatus} />
                        </div>
                      </Surface>

                      <Surface title="Delivery metrics" subtitle="현재 채널과 큐 상태">
                        <div className="grid gap-3">
                          <InfoStat label="Subscriptions" value={String(pushSubscriptionSummary.total)} detail={`ntfy ${pushSubscriptionSummary.ntfy} · web ${pushSubscriptionSummary.webPush}`} />
                          <InfoStat label="Queue pending" value={String(pushQueueSummary.pending)} detail={`sent ${pushQueueSummary.sent}`} />
                          <InfoStat label="Failures" value={String(pushQueueSummary.failed)} detail={pushQueueSummary.failed > 0 ? '재시도/점검 필요' : '문제 없음'} />
                        </div>
                      </Surface>
                    </div>

                    {pushTab === 'subscriptions' ? (
                      <Surface title="Subscriptions" subtitle="현재 저장된 채널 목록">
                        <div className="overflow-hidden rounded-[18px] border border-slate-200 bg-white">
                          <div className="grid grid-cols-[0.8fr_1.8fr_110px] gap-3 border-b border-slate-200 px-4 py-3 text-[10px] uppercase tracking-[0.24em] text-slate-400">
                            <span>Type</span>
                            <span>Target</span>
                            <span>Action</span>
                          </div>
                          {subscriptions.length === 0 ? (
                            <div className="p-4"><EmptyState text="먼저 topic 또는 web push 구독을 등록해줘." /></div>
                          ) : (
                            subscriptions.map((subscription) => (
                              <div key={subscription.id} className="grid grid-cols-[0.8fr_1.8fr_110px] gap-3 border-b border-slate-200 px-4 py-3 last:border-b-0">
                                <div className="flex items-center"><Badge tone={subscription.kind === 'web_push' ? 'info' : 'default'}>{subscription.kind}</Badge></div>
                                <div className="min-w-0">
                                  <p className="truncate font-medium text-slate-900">{subscription.topic ?? subscription.endpoint ?? 'unknown subscription'}</p>
                                  <p className="mt-1 truncate text-xs text-slate-400">{subscription.endpoint && subscription.kind === 'web_push' ? subscription.endpoint : subscription.created_at}</p>
                                </div>
                                <div className="flex items-center"><ActionButton busy={busyAction === `deleteSubscription:${subscription.id}`} onClick={() => void deleteSubscription(subscription.id)}>Delete</ActionButton></div>
                              </div>
                            ))
                          )}
                        </div>
                      </Surface>
                    ) : null}

                    {pushTab === 'deliveries' ? (
                      <Surface title="Deliveries" subtitle="큐와 실패 항목을 확인">
                        <div className="grid gap-4 xl:grid-cols-[320px_minmax(0,1fr)]">
                          <SurfaceCard title="Recent failed deliveries" subtitle="최근 실패 항목 우선 노출">
                            {failedQueueItems.length === 0 ? (
                              <EmptyState text="최근 실패가 없어. 전달 큐가 안정적이야." />
                            ) : (
                              <div className="grid gap-3">
                                {failedQueueItems.map((item) => (
                                  <FailureCard key={`failed-${item.id}`} title={item.title} detail={item.last_error ?? 'unknown error'} meta={`${item.created_at} · retry ${item.retry_count}`} />
                                ))}
                              </div>
                            )}
                          </SurfaceCard>

                          <div className="overflow-hidden rounded-[18px] border border-slate-200 bg-white">
                            <div className="grid grid-cols-[1.2fr_110px_0.9fr] gap-3 border-b border-slate-200 px-4 py-3 text-[10px] uppercase tracking-[0.24em] text-slate-400">
                              <span>Message</span>
                              <span>Status</span>
                              <span>Runtime</span>
                            </div>
                            {queueItems.length === 0 ? (
                              <div className="p-4"><EmptyState text="아직 큐에 들어간 메시지가 없어." /></div>
                            ) : (
                              queueItems.map((item) => (
                                <div key={item.id} className="border-b border-slate-200 px-4 py-3 last:border-b-0">
                                  <div className="grid grid-cols-[1.2fr_110px_0.9fr] gap-3">
                                    <div className="min-w-0">
                                      <p className="truncate font-medium text-slate-900">{item.title}</p>
                                      <p className="mt-1 truncate text-sm text-slate-400">{item.body}</p>
                                    </div>
                                    <div className="flex items-start"><Badge tone={item.status === 'failed' ? 'danger' : item.status === 'sent' ? 'success' : 'info'}>{item.status}</Badge></div>
                                    <div className="text-xs leading-6 text-slate-400">retries {item.retry_count}<br />created {item.created_at}</div>
                                  </div>
                                  {item.last_error ? <div className="mt-3 rounded-[14px] border border-rose-200 bg-rose-50 p-3 font-mono text-xs leading-6 text-rose-700">{item.last_error}</div> : null}
                                </div>
                              ))
                            )}
                          </div>
                        </div>
                      </Surface>
                    ) : null}
                  </div>
                ) : null}

                {activeView === 'admin' ? (
                  <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
                    <Surface title="Pending approvals" subtitle="회원가입 후 active 전환 대기 사용자">
                      <StatusBanner label="Admin status" tone={session?.user.is_admin ? 'info' : 'warning'} value={adminStatus} />
                      <div className="mt-4 overflow-hidden rounded-[16px] border border-slate-200 bg-white">
                        <div className="grid grid-cols-[1.2fr_1fr_120px] gap-3 border-b border-slate-200 px-4 py-3 text-[10px] uppercase tracking-[0.24em] text-slate-400">
                          <span>User</span>
                          <span>ID</span>
                          <span>Action</span>
                        </div>
                        {pendingUsers.length === 0 ? (
                          <div className="p-4"><EmptyState text="대기 중인 유저가 없거나 현재 계정이 admin이 아니야." /></div>
                        ) : (
                          pendingUsers.map((user) => (
                            <div key={user.id} className="grid grid-cols-[1.2fr_1fr_120px] gap-3 border-b border-slate-200 px-4 py-3 last:border-b-0">
                              <div>
                                <p className="font-medium text-slate-900">{user.email}</p>
                                <p className="mt-1 text-xs text-slate-400">pending approval</p>
                              </div>
                              <div className="truncate text-sm text-slate-400">{user.id}</div>
                              <div className="flex items-center"><ActionButton busy={busyAction === `activate:${user.id}`} onClick={() => void activateUser(user.id)} primary>Activate</ActionButton></div>
                            </div>
                          ))
                        )}
                      </div>
                    </Surface>

                    <Surface title="Active users" subtitle="현재 활성 사용자 목록">
                      <div className="overflow-hidden rounded-[16px] border border-slate-200 bg-white">
                        <div className="grid grid-cols-[1.2fr_1fr_120px] gap-3 border-b border-slate-200 px-4 py-3 text-[10px] uppercase tracking-[0.24em] text-slate-400">
                          <span>User</span>
                          <span>ID</span>
                          <span>Role</span>
                        </div>
                        {activeUsers.length === 0 ? (
                          <div className="p-4"><EmptyState text="활성 유저가 아직 없어." /></div>
                        ) : (
                          activeUsers.map((user) => (
                            <div key={user.id} className="grid grid-cols-[1.2fr_1fr_120px] gap-3 border-b border-slate-200 px-4 py-3 last:border-b-0">
                              <div>
                                <p className="font-medium text-slate-900">{user.email}</p>
                                <p className="mt-1 text-xs text-slate-400">created {user.created_at}</p>
                              </div>
                              <div className="truncate text-sm text-slate-400">{user.id}</div>
                              <div className="flex items-center"><Badge tone={user.is_admin ? 'warning' : 'default'}>{user.is_admin ? 'admin' : 'member'}</Badge></div>
                            </div>
                          ))
                        )}
                      </div>
                    </Surface>
                  </div>
                ) : null}

              </section>

              <aside className="min-w-0 grid gap-4 xl:sticky xl:top-5 xl:h-fit">
                <Surface title="Operational summary" subtitle="현재 세션과 리소스 상태 요약">
                  <div className="grid gap-3">
                    <InfoStat label="Health" value={health?.status ?? 'loading'} detail={healthError ?? health?.message ?? 'API 응답 대기 중'} />
                    <InfoStat label="Role" value={session?.user.is_admin ? 'admin' : token ? 'member' : 'guest'} detail={session?.user.email ?? '로그인 전'} />
                    <InfoStat label="Data rows" value={String(dataRows.length)} detail={selectedTableDetail?.name ?? 'table 없음'} />
                    <InfoStat label="Storage keys" value={String(storageKeys.length)} detail={`${recentKeys.length} recent`} />
                  </div>
                </Surface>

                <Surface title="Selected context" subtitle="선택된 리소스를 compact summary로 표시">
                  <div className="grid gap-3">
                    <ContextRow
                      label="Table"
                      value={selectedTableDetail?.display_name ?? 'none'}
                      detail={selectedTableDetail ? `${selectedTableDetail.name} · ${selectedTableFieldCount} fields · ${selectedTableDetail.access_policy.mode}` : '선택된 table 없음'}
                    />
                    <ContextRow
                      label="Row"
                      value={selectedRowPreview?.id ?? 'none'}
                      detail={selectedRowPreview ? `${selectedRowPreview.owner_user_id ?? 'shared'} · updated ${selectedRowPreview.updated_at}` : '선택된 row 없음'}
                    />
                    <ContextRow
                      label="Push"
                      value={String(pushSubscriptionSummary.total)}
                      detail={`subscriptions · failed ${pushQueueSummary.failed} · pending ${pushQueueSummary.pending}`}
                    />
                  </div>
                </Surface>
              </aside>
            </div>
          </div>
        </main>
        </div>
      </div>
    </div>
  )
}

const inputClassName =
  'w-full rounded-[14px] border border-slate-200 bg-white px-3.5 py-2.5 text-sm text-slate-900 outline-none transition duration-200 placeholder:text-slate-400 focus:border-emerald-400/30 focus:bg-white focus:ring-2 focus:ring-emerald-400/12'

const textareaClassName =
  'min-h-[220px] w-full rounded-[16px] border border-slate-200 bg-white px-3.5 py-3 font-mono text-sm leading-6 text-slate-200 outline-none transition duration-200 placeholder:text-slate-400 focus:border-emerald-400/30 focus:ring-2 focus:ring-emerald-400/12'

const smallTextareaClassName =
  'min-h-[132px] w-full rounded-[16px] border border-slate-200 bg-white px-3.5 py-3 font-mono text-sm leading-6 text-slate-200 outline-none transition duration-200 placeholder:text-slate-400 focus:border-cyan-400/30 focus:ring-2 focus:ring-cyan-400/12'


function toneClasses(tone: 'default' | 'success' | 'warning' | 'danger' | 'info') {
  switch (tone) {
    case 'success':
      return 'border-emerald-400/18 bg-emerald-50 text-emerald-100'
    case 'warning':
      return 'border-amber-200 bg-amber-400/8 text-amber-100'
    case 'danger':
      return 'border-rose-200 bg-rose-50 text-rose-100'
    case 'info':
      return 'border-cyan-200 bg-cyan-50 text-cyan-100'
    default:
      return 'border-slate-200 bg-slate-50 text-slate-100'
  }
}

function compactJson(value: unknown, maxLength = 180): string {
  const text = typeof value === 'string' ? value : JSON.stringify(value, null, 2)
  return text.length > maxLength ? `${text.slice(0, maxLength)}…` : text
}

function base64UrlToArrayBuffer(value: string): ArrayBuffer {
  const normalized = value.replace(/-/g, '+').replace(/_/g, '/')
  const padded = normalized + '='.repeat((4 - (normalized.length % 4 || 4)) % 4)
  const raw = window.atob(padded)
  const bytes = Uint8Array.from(raw, (char) => char.charCodeAt(0))
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength)
}

type SurfaceProps = {
  title: string
  subtitle: string
  children: ReactNode
}

function Surface({ title, subtitle, children }: SurfaceProps) {
  return (
    <section className="rounded-[20px] border border-slate-200 bg-[linear-gradient(180deg,rgba(255,255,255,0.02),rgba(255,255,255,0.01))] p-5 shadow-[inset_0_1px_0_rgba(255,255,255,0.03)]">
      <div className="mb-5 flex flex-col gap-1">
        <h3 className="text-[15px] font-semibold tracking-tight text-slate-900">{title}</h3>
        <p className="text-sm leading-6 text-slate-400">{subtitle}</p>
      </div>
      {children}
    </section>
  )
}

function SurfaceCard({ title, subtitle, children }: SurfaceProps) {
  return (
    <div className="rounded-[18px] border border-slate-200 bg-white p-4">
      <div className="mb-4">
        <h4 className="text-sm font-semibold text-slate-900">{title}</h4>
        <p className="mt-1 text-xs leading-5 text-slate-400">{subtitle}</p>
      </div>
      {children}
    </div>
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
        'inline-flex min-h-10 cursor-pointer items-center justify-center rounded-[12px] border px-3.5 py-2 text-sm font-medium transition duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-400/16',
        primary
          ? 'border-emerald-400/22 bg-emerald-400 text-[#08110d] hover:bg-emerald-300'
          : 'border-slate-200 bg-slate-50 text-slate-900 hover:border-white/12 hover:bg-slate-100',
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

function TabButton({ active, children, onClick }: { active: boolean; children: ReactNode; onClick: () => void }) {
  return (
    <button
      className={[
        'inline-flex min-h-9 cursor-pointer items-center rounded-[999px] border px-3 py-1.5 text-sm transition duration-200',
        active
          ? 'border-emerald-400/18 bg-emerald-50 text-emerald-50'
          : 'border-slate-200 bg-slate-50 text-slate-600 hover:border-white/12 hover:text-slate-900',
      ].join(' ')}
      onClick={onClick}
      type="button"
    >
      {children}
    </button>
  )
}

function Badge({ children, tone = 'default' }: { children: ReactNode; tone?: 'default' | 'success' | 'warning' | 'danger' | 'info' }) {
  return <span className={['rounded-full border px-2 py-0.5 text-[10px] uppercase tracking-[0.22em]', toneClasses(tone)].join(' ')}>{children}</span>
}

function MiniIconStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex h-8 w-8 items-center justify-center rounded-[10px] border border-slate-200 bg-slate-50 text-[11px] font-medium text-slate-500" aria-label={label}>
      {value}
    </div>
  )
}

function StatusTile({ label, value, detail }: { label: string; value: string; detail: string }) {
  return (
    <div className="rounded-[16px] border border-slate-200 bg-white p-4">
      <p className="text-[10px] uppercase tracking-[0.24em] text-slate-400">{label}</p>
      <p className="mt-3 truncate text-sm font-semibold text-slate-900">{value}</p>
      <p className="mt-2 truncate text-xs text-slate-400">{detail}</p>
    </div>
  )
}

function OverviewStatRow({ label, value, detail }: { label: string; value: string; detail: string }) {
  return (
    <div className="flex items-center gap-4 rounded-[14px] border border-slate-200 bg-white px-4 py-3">
      <div className="flex h-10 w-10 items-center justify-center rounded-[12px] bg-slate-50 text-xs font-semibold text-slate-500">•</div>
      <div className="min-w-0">
        <p className="text-[10px] uppercase tracking-[0.24em] text-slate-400">{label}</p>
        <p className="mt-1 text-sm font-medium text-slate-900">{value}</p>
        <p className="mt-1 text-xs text-slate-400">{detail}</p>
      </div>
    </div>
  )
}

function RequestCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-[16px] border border-slate-200 bg-white p-4">
      <p className="text-[10px] uppercase tracking-[0.24em] text-slate-400">{label}</p>
      <p className="mt-3 text-3xl font-semibold tracking-tight text-slate-900">{value}</p>
      <div className="mt-6 h-24 rounded-[12px] border border-slate-100 bg-slate-50" />
    </div>
  )
}

function SetupOptionCard({
  eyebrow,
  title,
  description,
  actionLabel,
  onAction,
  busy,
  children,
}: {
  eyebrow: string
  title: string
  description: string
  actionLabel: string
  onAction: () => void
  busy?: boolean
  children: ReactNode
}) {
  return (
    <div className="rounded-[16px] border border-slate-200 bg-white p-4">
      <p className="text-[10px] uppercase tracking-[0.24em] text-slate-400">{eyebrow}</p>
      <h4 className="mt-3 text-base font-semibold text-slate-900">{title}</h4>
      <p className="mt-2 text-sm leading-6 text-slate-500">{description}</p>
      <div className="mt-4">{children}</div>
      <div className="mt-4">
        <ActionButton busy={busy} onClick={onAction}>{actionLabel}</ActionButton>
      </div>
    </div>
  )
}

function StatusBanner({ label, value, tone = 'default' }: { label: string; value: string; tone?: 'default' | 'success' | 'warning' | 'danger' | 'info' }) {
  return (
    <div className={['rounded-[16px] border p-4', toneClasses(tone)].join(' ')}>
      <p className="text-[10px] uppercase tracking-[0.24em] text-slate-400">{label}</p>
      <p className="mt-2 whitespace-pre-wrap break-all text-sm leading-6 text-slate-900">{value}</p>
    </div>
  )
}

function InfoStat({ label, value, detail }: { label: string; value: string; detail: string }) {
  return (
    <div className="rounded-[16px] border border-slate-200 bg-white p-4">
      <p className="text-[10px] uppercase tracking-[0.24em] text-slate-400">{label}</p>
      <p className="mt-3 text-xl font-semibold text-slate-900">{value}</p>
      <p className="mt-2 text-xs leading-5 text-slate-400">{detail}</p>
    </div>
  )
}

function ContextRow({ label, value, detail }: { label: string; value: string; detail: string }) {
  return (
    <div className="rounded-[16px] border border-slate-200 bg-white p-4">
      <div className="flex items-center justify-between gap-3">
        <p className="text-[10px] uppercase tracking-[0.24em] text-slate-400">{label}</p>
        <p className="truncate text-sm font-semibold text-slate-900">{value}</p>
      </div>
      <p className="mt-2 text-xs leading-5 text-slate-400">{detail}</p>
    </div>
  )
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="grid gap-2 text-sm text-slate-400">
      <span className="text-xs font-medium uppercase tracking-[0.18em] text-slate-400">{label}</span>
      {children}
    </label>
  )
}

function EmptyState({ text }: { text: string }) {
  return <div className="rounded-[16px] border border-dashed border-white/10 px-4 py-8 text-sm leading-6 text-slate-400">{text}</div>
}

function FailureCard({ title, detail, meta }: { title: string; detail: string; meta: string }) {
  return (
    <div className="rounded-[16px] border border-rose-200 bg-rose-50 p-4">
      <p className="font-medium text-slate-900">{title}</p>
      <p className="mt-2 text-xs text-rose-100/70">{meta}</p>
      <p className="mt-3 whitespace-pre-wrap break-words text-xs leading-6 text-rose-50">{detail}</p>
    </div>
  )
}
