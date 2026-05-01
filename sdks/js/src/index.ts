export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

export interface PeanutClientOptions {
  baseUrl: string;
  appId: string;
  apiKey: string;
  accessToken?: string;
  timeoutMs?: number;
  retry?: PeanutRetryOptions;
  fetch?: typeof globalThis.fetch;
}

export interface PeanutRetryOptions {
  maxRetries?: number;
  baseDelayMs?: number;
}

export interface PeanutUser {
  id: string;
  email: string;
  is_active: boolean;
  is_admin: boolean;
}

export interface PeanutLoginResponse {
  access_token: string;
  refresh_token: string;
  token_type: string;
  expires_at: string;
  user: PeanutUser;
}

export interface PeanutObjectSummary {
  key: string;
  size: number;
  content_type?: string | null;
  etag: string;
  updated_at: string;
}

export class PeanutError extends Error {
  readonly status: number;
  readonly body: unknown;

  constructor(status: number, message: string, body: unknown) {
    super(message);
    this.name = "PeanutError";
    this.status = status;
    this.body = body;
  }
}

export class PeanutClient {
  readonly auth: PeanutAuthClient;
  readonly data: PeanutDataClient;
  readonly storage: PeanutStorageClient;
  readonly push: PeanutPushClient;
  readonly functions: PeanutFunctionsClient;

  private readonly baseUrl: string;
  private readonly appId: string;
  private readonly apiKey: string;
  private accessToken?: string;
  private readonly fetchImpl: typeof globalThis.fetch;
  private readonly timeoutMs: number;
  private readonly retry: Required<PeanutRetryOptions>;

  constructor(options: PeanutClientOptions) {
    this.baseUrl = options.baseUrl.replace(/\/+$/, "");
    this.appId = options.appId;
    this.apiKey = options.apiKey;
    this.accessToken = options.accessToken;
    this.fetchImpl = options.fetch ?? globalThis.fetch;
    this.timeoutMs = options.timeoutMs ?? 30_000;
    this.retry = {
      maxRetries: options.retry?.maxRetries ?? 0,
      baseDelayMs: options.retry?.baseDelayMs ?? 200,
    };
    if (!this.fetchImpl) {
      throw new Error("A fetch implementation is required");
    }
    this.auth = new PeanutAuthClient(this);
    this.data = new PeanutDataClient(this);
    this.storage = new PeanutStorageClient(this);
    this.push = new PeanutPushClient(this);
    this.functions = new PeanutFunctionsClient(this);
  }

  setAccessToken(accessToken?: string): void {
    this.accessToken = accessToken;
  }

  async request<T>(
    method: string,
    path: string,
    options: { body?: unknown; headers?: HeadersInit; rawBody?: BodyInit } = {},
  ): Promise<T> {
    const headers = new Headers(options.headers);
    headers.set("X-Peanut-Api-Key", this.apiKey);
    if (this.accessToken) {
      headers.set("Authorization", `Bearer ${this.accessToken}`);
    }
    let body = options.rawBody;
    if (options.body !== undefined) {
      headers.set("Content-Type", "application/json");
      body = JSON.stringify(options.body);
    }
    const response = await this.fetchWithRetry(`${this.baseUrl}${path}`, {
      method,
      headers,
      body,
    });
    if (response.status === 204) {
      return undefined as T;
    }
    const contentType = response.headers.get("content-type") ?? "";
    const value = contentType.includes("application/json")
      ? await response.json()
      : await response.text();
    if (!response.ok) {
      throw new PeanutError(response.status, errorMessage(value), value);
    }
    return value as T;
  }

  async requestBinary(method: string, path: string): Promise<Response> {
    const headers = new Headers();
    headers.set("X-Peanut-Api-Key", this.apiKey);
    if (this.accessToken) {
      headers.set("Authorization", `Bearer ${this.accessToken}`);
    }
    const response = await this.fetchWithRetry(`${this.baseUrl}${path}`, { method, headers });
    if (!response.ok) {
      const contentType = response.headers.get("content-type") ?? "";
      const value = contentType.includes("application/json")
        ? await response.json()
        : await response.text();
      throw new PeanutError(response.status, errorMessage(value), value);
    }
    return response;
  }

  async requestBinaryWithBody(
    method: string,
    path: string,
    body: BodyInit,
    contentType = "application/octet-stream",
  ): Promise<Response> {
    const headers = new Headers({ "Content-Type": contentType });
    headers.set("X-Peanut-Api-Key", this.apiKey);
    if (this.accessToken) {
      headers.set("Authorization", `Bearer ${this.accessToken}`);
    }
    const response = await this.fetchWithRetry(`${this.baseUrl}${path}`, { method, headers, body });
    if (!response.ok) {
      const contentType = response.headers.get("content-type") ?? "";
      const value = contentType.includes("application/json")
        ? await response.json()
        : await response.text();
      throw new PeanutError(response.status, errorMessage(value), value);
    }
    return response;
  }

  appPath(path: string): string {
    return `/api/apps/${encodeURIComponent(this.appId)}${path}`;
  }

  private async fetchWithRetry(input: string, init: RequestInit): Promise<Response> {
    let attempt = 0;
    let lastError: unknown;
    while (attempt <= this.retry.maxRetries) {
      const controller = new AbortController();
      const timeout = setTimeout(() => controller.abort(), this.timeoutMs);
      try {
        const response = await this.fetchImpl(input, { ...init, signal: controller.signal });
        clearTimeout(timeout);
        if (!isTransientStatus(response.status) || attempt === this.retry.maxRetries) {
          return response;
        }
      } catch (error) {
        clearTimeout(timeout);
        lastError = error;
        if (attempt === this.retry.maxRetries) {
          throw error;
        }
      }
      await delay(this.retry.baseDelayMs * Math.max(1, attempt + 1));
      attempt += 1;
    }
    throw lastError;
  }
}

export class PeanutAuthClient {
  constructor(private readonly client: PeanutClient) {}

  register(email: string, password: string): Promise<{ message: string; user: PeanutUser }> {
    return this.client.request("POST", this.client.appPath("/auth/register"), {
      body: { email, password },
    });
  }

  async login(email: string, password: string): Promise<PeanutLoginResponse> {
    const response = await this.client.request<PeanutLoginResponse>(
      "POST",
      this.client.appPath("/auth/login"),
      { body: { email, password } },
    );
    this.client.setAccessToken(response.access_token);
    return response;
  }

  async refresh(refreshToken: string): Promise<PeanutLoginResponse> {
    const response = await this.client.request<PeanutLoginResponse>(
      "POST",
      this.client.appPath("/auth/refresh"),
      { body: { refresh_token: refreshToken } },
    );
    this.client.setAccessToken(response.access_token);
    return response;
  }

  async logout(refreshToken: string): Promise<void> {
    await this.client.request("POST", this.client.appPath("/auth/logout"), {
      body: { refresh_token: refreshToken },
    });
    this.client.setAccessToken(undefined);
  }

  me(): Promise<{ user: PeanutUser }> {
    return this.client.request("GET", this.client.appPath("/auth/me"));
  }
}

export class PeanutDataClient {
  constructor(private readonly client: PeanutClient) {}

  listTables(): Promise<{ tables: unknown[] }> {
    return this.client.request("GET", this.client.appPath("/data/tables"));
  }

  getTable(table: string): Promise<{ table: unknown }> {
    return this.client.request("GET", this.client.appPath(`/data/tables/${encodePath(table)}`));
  }

  listRows(table: string, params: Record<string, string | number | boolean | undefined> = {}): Promise<{ rows: unknown[] }> {
    const query = toQuery(params);
    return this.client.request("GET", this.client.appPath(`/data/tables/${encodePath(table)}/rows${query}`));
  }

  createRow(table: string, data: JsonValue): Promise<unknown> {
    return this.client.request("POST", this.client.appPath(`/data/tables/${encodePath(table)}/rows`), {
      body: { data },
    });
  }

  getRow(table: string, rowId: string): Promise<unknown> {
    return this.client.request("GET", this.client.appPath(`/data/tables/${encodePath(table)}/rows/${encodePath(rowId)}`));
  }

  updateRow(table: string, rowId: string, data: JsonValue): Promise<unknown> {
    return this.client.request("PATCH", this.client.appPath(`/data/tables/${encodePath(table)}/rows/${encodePath(rowId)}`), {
      body: { data },
    });
  }

  deleteRow(table: string, rowId: string): Promise<void> {
    return this.client.request("DELETE", this.client.appPath(`/data/tables/${encodePath(table)}/rows/${encodePath(rowId)}`));
  }
}

export class PeanutStorageClient {
  constructor(private readonly client: PeanutClient) {}

  listObjects(bucket: string, prefix?: string): Promise<{ objects: PeanutObjectSummary[] }> {
    const query = prefix ? `?prefix=${encodeURIComponent(prefix)}` : "";
    return this.client.request("GET", this.client.appPath(`/storage/buckets/${encodePath(bucket)}/objects${query}`));
  }

  getObject(bucket: string, key: string): Promise<Response> {
    return this.client.requestBinary("GET", this.client.appPath(`/storage/buckets/${encodePath(bucket)}/objects/${encodeKey(key)}`));
  }

  putObject(bucket: string, key: string, body: BodyInit, contentType = "application/octet-stream"): Promise<Response> {
    return this.client.requestBinaryWithBody("PUT", this.client.appPath(`/storage/buckets/${encodePath(bucket)}/objects/${encodeKey(key)}`), body, contentType);
  }

  deleteObject(bucket: string, key: string): Promise<void> {
    return this.client.request("DELETE", this.client.appPath(`/storage/buckets/${encodePath(bucket)}/objects/${encodeKey(key)}`));
  }
}

export class PeanutPushClient {
  constructor(private readonly client: PeanutClient) {}

  listSubscriptions(): Promise<{ subscriptions: unknown[] }> {
    return this.client.request("GET", this.client.appPath("/push/subscriptions"));
  }

  createNtfySubscription(topic: string): Promise<unknown> {
    return this.client.request("POST", this.client.appPath("/push/subscriptions"), {
      body: { topic },
    });
  }

  createWebPushSubscription(endpoint: string, keys: { p256dh: string; auth: string }): Promise<unknown> {
    return this.client.request("POST", this.client.appPath("/push/subscriptions"), {
      body: { endpoint, keys },
    });
  }

  deleteSubscription(subscriptionId: number): Promise<void> {
    return this.client.request("DELETE", this.client.appPath(`/push/subscriptions/${subscriptionId}`));
  }

  getVapidPublicKey(): Promise<{ public_key: string }> {
    return this.client.request("GET", this.client.appPath("/push/vapid-public-key"));
  }

  enqueueMessage(payload: { title: string; body: string; user_id?: string }): Promise<unknown> {
    return this.client.request("POST", this.client.appPath("/push/messages"), { body: payload });
  }
}

export class PeanutFunctionsClient {
  constructor(private readonly client: PeanutClient) {}

  invoke(endpointSlug: string, input: JsonValue = null, options: { apiKey?: string; async?: boolean } = {}): Promise<unknown> {
    return this.client.request("POST", this.client.appPath(`/functions/endpoints/${encodePath(endpointSlug)}`), {
      body: {
        input,
        api_key: options.apiKey,
        async_invoke: options.async,
      },
    });
  }
}

function encodePath(value: string): string {
  return encodeURIComponent(value);
}

function encodeKey(value: string): string {
  return value.split("/").map(encodeURIComponent).join("/");
}

function toQuery(params: Record<string, string | number | boolean | undefined>): string {
  const search = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined) {
      search.set(key, String(value));
    }
  }
  const query = search.toString();
  return query ? `?${query}` : "";
}

function errorMessage(value: unknown): string {
  if (typeof value === "object" && value !== null && "error" in value) {
    return String((value as { error: unknown }).error);
  }
  return typeof value === "string" && value ? value : "Peanut request failed";
}

function isTransientStatus(status: number): boolean {
  return status === 408 || status === 429 || status >= 500;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
