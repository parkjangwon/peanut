export class PeanutError extends Error {
    status;
    body;
    constructor(status, message, body) {
        super(message);
        this.name = "PeanutError";
        this.status = status;
        this.body = body;
    }
}
export class PeanutClient {
    auth;
    data;
    storage;
    push;
    functions;
    baseUrl;
    appId;
    apiKey;
    accessToken;
    fetchImpl;
    timeoutMs;
    retry;
    constructor(options) {
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
    setAccessToken(accessToken) {
        this.accessToken = accessToken;
    }
    async request(method, path, options = {}) {
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
            return undefined;
        }
        const contentType = response.headers.get("content-type") ?? "";
        const value = contentType.includes("application/json")
            ? await response.json()
            : await response.text();
        if (!response.ok) {
            throw new PeanutError(response.status, errorMessage(value), value);
        }
        return value;
    }
    async requestBinary(method, path) {
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
    async requestBinaryWithBody(method, path, body, contentType = "application/octet-stream") {
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
    appPath(path) {
        return `/api/apps/${encodeURIComponent(this.appId)}${path}`;
    }
    async fetchWithRetry(input, init) {
        let attempt = 0;
        let lastError;
        while (attempt <= this.retry.maxRetries) {
            const controller = new AbortController();
            const timeout = setTimeout(() => controller.abort(), this.timeoutMs);
            try {
                const response = await this.fetchImpl(input, { ...init, signal: controller.signal });
                clearTimeout(timeout);
                if (!isTransientStatus(response.status) || attempt === this.retry.maxRetries) {
                    return response;
                }
            }
            catch (error) {
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
    client;
    constructor(client) {
        this.client = client;
    }
    register(email, password) {
        return this.client.request("POST", this.client.appPath("/auth/register"), {
            body: { email, password },
        });
    }
    async login(email, password) {
        const response = await this.client.request("POST", this.client.appPath("/auth/login"), { body: { email, password } });
        this.client.setAccessToken(response.access_token);
        return response;
    }
    async refresh(refreshToken) {
        const response = await this.client.request("POST", this.client.appPath("/auth/refresh"), { body: { refresh_token: refreshToken } });
        this.client.setAccessToken(response.access_token);
        return response;
    }
    async logout(refreshToken) {
        await this.client.request("POST", this.client.appPath("/auth/logout"), {
            body: { refresh_token: refreshToken },
        });
        this.client.setAccessToken(undefined);
    }
    me() {
        return this.client.request("GET", this.client.appPath("/auth/me"));
    }
}
export class PeanutDataClient {
    client;
    constructor(client) {
        this.client = client;
    }
    listTables() {
        return this.client.request("GET", this.client.appPath("/data/tables"));
    }
    getTable(table) {
        return this.client.request("GET", this.client.appPath(`/data/tables/${encodePath(table)}`));
    }
    listRows(table, params = {}) {
        const query = toQuery(params);
        return this.client.request("GET", this.client.appPath(`/data/tables/${encodePath(table)}/rows${query}`));
    }
    createRow(table, data) {
        return this.client.request("POST", this.client.appPath(`/data/tables/${encodePath(table)}/rows`), {
            body: { data },
        });
    }
    getRow(table, rowId) {
        return this.client.request("GET", this.client.appPath(`/data/tables/${encodePath(table)}/rows/${encodePath(rowId)}`));
    }
    updateRow(table, rowId, data) {
        return this.client.request("PATCH", this.client.appPath(`/data/tables/${encodePath(table)}/rows/${encodePath(rowId)}`), {
            body: { data },
        });
    }
    deleteRow(table, rowId) {
        return this.client.request("DELETE", this.client.appPath(`/data/tables/${encodePath(table)}/rows/${encodePath(rowId)}`));
    }
    executeSql(sql) {
        return this.client.request("POST", this.client.appPath("/data/query"), {
            body: { sql },
        });
    }
}
export class PeanutStorageClient {
    client;
    constructor(client) {
        this.client = client;
    }
    listObjects(bucket, prefix) {
        const query = prefix ? `?prefix=${encodeURIComponent(prefix)}` : "";
        return this.client.request("GET", this.client.appPath(`/storage/buckets/${encodePath(bucket)}/objects${query}`));
    }
    getObject(bucket, key) {
        return this.client.requestBinary("GET", this.client.appPath(`/storage/buckets/${encodePath(bucket)}/objects/${encodeKey(key)}`));
    }
    putObject(bucket, key, body, contentType = "application/octet-stream") {
        return this.client.requestBinaryWithBody("PUT", this.client.appPath(`/storage/buckets/${encodePath(bucket)}/objects/${encodeKey(key)}`), body, contentType);
    }
    deleteObject(bucket, key) {
        return this.client.request("DELETE", this.client.appPath(`/storage/buckets/${encodePath(bucket)}/objects/${encodeKey(key)}`));
    }
}
export class PeanutPushClient {
    client;
    constructor(client) {
        this.client = client;
    }
    listSubscriptions() {
        return this.client.request("GET", this.client.appPath("/push/subscriptions"));
    }
    createNtfySubscription(topic) {
        return this.client.request("POST", this.client.appPath("/push/subscriptions"), {
            body: { topic },
        });
    }
    createWebPushSubscription(endpoint, keys) {
        return this.client.request("POST", this.client.appPath("/push/subscriptions"), {
            body: { endpoint, keys },
        });
    }
    deleteSubscription(subscriptionId) {
        return this.client.request("DELETE", this.client.appPath(`/push/subscriptions/${subscriptionId}`));
    }
    getVapidPublicKey() {
        return this.client.request("GET", this.client.appPath("/push/vapid-public-key"));
    }
    enqueueMessage(payload) {
        return this.client.request("POST", this.client.appPath("/push/messages"), { body: payload });
    }
}
export class PeanutFunctionsClient {
    client;
    constructor(client) {
        this.client = client;
    }
    invoke(endpointSlug, input = null, options = {}) {
        return this.client.request("POST", this.client.appPath(`/function-endpoints/${encodePath(endpointSlug)}`), {
            body: {
                input,
                api_key: options.apiKey,
                async_invoke: options.async,
            },
        });
    }
}
function encodePath(value) {
    return encodeURIComponent(value);
}
function encodeKey(value) {
    return value.split("/").map(encodeURIComponent).join("/");
}
function toQuery(params) {
    const search = new URLSearchParams();
    for (const [key, value] of Object.entries(params)) {
        if (value !== undefined) {
            search.set(key, String(value));
        }
    }
    const query = search.toString();
    return query ? `?${query}` : "";
}
function errorMessage(value) {
    if (typeof value === "object" && value !== null && "error" in value) {
        return String(value.error);
    }
    return typeof value === "string" && value ? value : "Peanut request failed";
}
function isTransientStatus(status) {
    return status === 408 || status === 429 || status >= 500;
}
function delay(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}
