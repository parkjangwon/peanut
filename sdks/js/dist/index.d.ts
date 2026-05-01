export type JsonValue = null | boolean | number | string | JsonValue[] | {
    [key: string]: JsonValue;
};
export interface PeanutClientOptions {
    baseUrl: string;
    appId: string;
    apiKey: string;
    accessToken?: string;
    fetch?: typeof globalThis.fetch;
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
export declare class PeanutError extends Error {
    readonly status: number;
    readonly body: unknown;
    constructor(status: number, message: string, body: unknown);
}
export declare class PeanutClient {
    readonly auth: PeanutAuthClient;
    readonly data: PeanutDataClient;
    readonly storage: PeanutStorageClient;
    readonly push: PeanutPushClient;
    readonly functions: PeanutFunctionsClient;
    private readonly baseUrl;
    private readonly appId;
    private readonly apiKey;
    private accessToken?;
    private readonly fetchImpl;
    constructor(options: PeanutClientOptions);
    setAccessToken(accessToken?: string): void;
    request<T>(method: string, path: string, options?: {
        body?: unknown;
        headers?: HeadersInit;
        rawBody?: BodyInit;
    }): Promise<T>;
    requestBinary(method: string, path: string): Promise<Response>;
    requestBinaryWithBody(method: string, path: string, body: BodyInit, contentType?: string): Promise<Response>;
    appPath(path: string): string;
}
export declare class PeanutAuthClient {
    private readonly client;
    constructor(client: PeanutClient);
    register(email: string, password: string): Promise<{
        message: string;
        user: PeanutUser;
    }>;
    login(email: string, password: string): Promise<PeanutLoginResponse>;
    refresh(refreshToken: string): Promise<PeanutLoginResponse>;
    logout(refreshToken: string): Promise<void>;
    me(): Promise<{
        user: PeanutUser;
    }>;
}
export declare class PeanutDataClient {
    private readonly client;
    constructor(client: PeanutClient);
    listTables(): Promise<{
        tables: unknown[];
    }>;
    getTable(table: string): Promise<{
        table: unknown;
    }>;
    listRows(table: string, params?: Record<string, string | number | boolean | undefined>): Promise<{
        rows: unknown[];
    }>;
    createRow(table: string, data: JsonValue): Promise<unknown>;
    getRow(table: string, rowId: string): Promise<unknown>;
    updateRow(table: string, rowId: string, data: JsonValue): Promise<unknown>;
    deleteRow(table: string, rowId: string): Promise<void>;
}
export declare class PeanutStorageClient {
    private readonly client;
    constructor(client: PeanutClient);
    listObjects(bucket: string, prefix?: string): Promise<{
        objects: PeanutObjectSummary[];
    }>;
    getObject(bucket: string, key: string): Promise<Response>;
    putObject(bucket: string, key: string, body: BodyInit, contentType?: string): Promise<Response>;
    deleteObject(bucket: string, key: string): Promise<void>;
}
export declare class PeanutPushClient {
    private readonly client;
    constructor(client: PeanutClient);
    listSubscriptions(): Promise<{
        subscriptions: unknown[];
    }>;
    createNtfySubscription(topic: string): Promise<unknown>;
    createWebPushSubscription(endpoint: string, keys: {
        p256dh: string;
        auth: string;
    }): Promise<unknown>;
    deleteSubscription(subscriptionId: number): Promise<void>;
    getVapidPublicKey(): Promise<{
        public_key: string;
    }>;
    enqueueMessage(payload: {
        title: string;
        body: string;
        user_id?: string;
    }): Promise<unknown>;
}
export declare class PeanutFunctionsClient {
    private readonly client;
    constructor(client: PeanutClient);
    invoke(endpointSlug: string, input?: JsonValue, options?: {
        apiKey?: string;
        async?: boolean;
    }): Promise<unknown>;
}
