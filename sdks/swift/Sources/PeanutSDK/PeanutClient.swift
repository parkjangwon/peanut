import Foundation

public typealias PeanutJSONObject = [String: PeanutJSONValue]

public enum PeanutJSONValue: Codable, Sendable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([PeanutJSONValue])
    case object(PeanutJSONObject)

    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let value = try? container.decode(Bool.self) {
            self = .bool(value)
        } else if let value = try? container.decode(Double.self) {
            self = .number(value)
        } else if let value = try? container.decode(String.self) {
            self = .string(value)
        } else if let value = try? container.decode([PeanutJSONValue].self) {
            self = .array(value)
        } else {
            self = .object(try container.decode(PeanutJSONObject.self))
        }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .null:
            try container.encodeNil()
        case .bool(let value):
            try container.encode(value)
        case .number(let value):
            try container.encode(value)
        case .string(let value):
            try container.encode(value)
        case .array(let value):
            try container.encode(value)
        case .object(let value):
            try container.encode(value)
        }
    }
}

public struct PeanutError: Error, Sendable {
    public let statusCode: Int
    public let message: String
    public let body: Data
}

public final class PeanutClient: @unchecked Sendable {
    public let auth: PeanutAuthClient
    public let data: PeanutDataClient
    public let storage: PeanutStorageClient
    public let push: PeanutPushClient
    public let functions: PeanutFunctionsClient

    let baseURL: URL
    let appId: String
    let apiKey: String
    let session: URLSession
    private let lock = NSLock()
    private var accessToken: String?

    public init(
        baseURL: URL,
        appId: String,
        apiKey: String,
        accessToken: String? = nil,
        session: URLSession = .shared
    ) {
        self.baseURL = baseURL
        self.appId = appId
        self.apiKey = apiKey
        self.accessToken = accessToken
        self.session = session
        self.auth = PeanutAuthClient()
        self.data = PeanutDataClient()
        self.storage = PeanutStorageClient()
        self.push = PeanutPushClient()
        self.functions = PeanutFunctionsClient()
        self.auth.client = self
        self.data.client = self
        self.storage.client = self
        self.push.client = self
        self.functions.client = self
    }

    public func setAccessToken(_ token: String?) {
        lock.lock()
        accessToken = token
        lock.unlock()
    }

    func requestJSON<T: Decodable>(
        _ type: T.Type,
        method: String,
        path: String,
        body: Encodable? = nil
    ) async throws -> T {
        var request = URLRequest(url: url(path: path))
        request.httpMethod = method
        applyHeaders(to: &request)
        if let body {
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = try JSONEncoder().encode(AnyEncodable(body))
        }
        let (data, response) = try await session.data(for: request)
        try validate(data: data, response: response)
        if T.self == EmptyResponse.self {
            return EmptyResponse() as! T
        }
        return try JSONDecoder().decode(T.self, from: data)
    }

    func requestBytes(
        method: String,
        path: String,
        body: Data? = nil,
        contentType: String? = nil
    ) async throws -> (Data, HTTPURLResponse) {
        var request = URLRequest(url: url(path: path))
        request.httpMethod = method
        applyHeaders(to: &request)
        if let body {
            request.httpBody = body
            request.setValue(contentType ?? "application/octet-stream", forHTTPHeaderField: "Content-Type")
        }
        let (data, response) = try await session.data(for: request)
        try validate(data: data, response: response)
        return (data, response as! HTTPURLResponse)
    }

    func appPath(_ path: String) -> String {
        "/api/apps/\(Self.segment(appId))\(path)"
    }

    private func applyHeaders(to request: inout URLRequest) {
        request.setValue(apiKey, forHTTPHeaderField: "X-Peanut-Api-Key")
        lock.lock()
        let token = accessToken
        lock.unlock()
        if let token, !token.isEmpty {
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }
    }

    private func url(path: String) -> URL {
        URL(string: baseURL.absoluteString.trimmingCharacters(in: CharacterSet(charactersIn: "/")) + path)!
    }

    private func validate(data: Data, response: URLResponse) throws {
        guard let http = response as? HTTPURLResponse else {
            throw PeanutError(statusCode: -1, message: "Invalid Peanut response", body: data)
        }
        guard (200..<300).contains(http.statusCode) else {
            let message = (try? JSONDecoder().decode(ErrorEnvelope.self, from: data).error)
                ?? String(data: data, encoding: .utf8)
                ?? "Peanut request failed"
            throw PeanutError(statusCode: http.statusCode, message: message, body: data)
        }
    }

    static func segment(_ value: String) -> String {
        value.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? value
    }

    static func keyPath(_ value: String) -> String {
        value.split(separator: "/", omittingEmptySubsequences: false)
            .map { segment(String($0)) }
            .joined(separator: "/")
    }

    static func query(_ values: [String: String]) -> String {
        let items = values
            .map { key, value in "\(segment(key))=\(segment(value))" }
            .sorted()
            .joined(separator: "&")
        return items.isEmpty ? "" : "?\(items)"
    }
}

public struct PeanutUser: Codable, Sendable {
    public let id: String
    public let email: String
    public let is_active: Bool
    public let is_admin: Bool
}

public struct LoginResponse: Codable, Sendable {
    public let access_token: String
    public let refresh_token: String
    public let token_type: String
    public let expires_at: String
    public let user: PeanutUser
}

public struct RegisterResponse: Codable, Sendable {
    public let message: String
    public let user: PeanutUser
}

public struct UserResponse: Codable, Sendable {
    public let user: PeanutUser
}

public struct EmptyResponse: Codable, Sendable {
    public init() {}
}

public final class PeanutAuthClient: @unchecked Sendable {
    weak var client: PeanutClient?

    init() {}

    public func register(email: String, password: String) async throws -> RegisterResponse {
        try await requireClient().requestJSON(
            RegisterResponse.self,
            method: "POST",
            path: requireClient().appPath("/auth/register"),
            body: ["email": PeanutJSONValue.string(email), "password": PeanutJSONValue.string(password)]
        )
    }

    public func login(email: String, password: String) async throws -> LoginResponse {
        let client = requireClient()
        let response = try await client.requestJSON(
            LoginResponse.self,
            method: "POST",
            path: client.appPath("/auth/login"),
            body: ["email": PeanutJSONValue.string(email), "password": PeanutJSONValue.string(password)]
        )
        client.setAccessToken(response.access_token)
        return response
    }

    public func refresh(refreshToken: String) async throws -> LoginResponse {
        let client = requireClient()
        let response = try await client.requestJSON(
            LoginResponse.self,
            method: "POST",
            path: client.appPath("/auth/refresh"),
            body: ["refresh_token": PeanutJSONValue.string(refreshToken)]
        )
        client.setAccessToken(response.access_token)
        return response
    }

    public func logout(refreshToken: String) async throws {
        let client = requireClient()
        _ = try await client.requestJSON(
            EmptyResponse.self,
            method: "POST",
            path: client.appPath("/auth/logout"),
            body: ["refresh_token": PeanutJSONValue.string(refreshToken)]
        )
        client.setAccessToken(nil)
    }

    public func me() async throws -> UserResponse {
        let client = requireClient()
        return try await client.requestJSON(UserResponse.self, method: "GET", path: client.appPath("/auth/me"))
    }

    private func requireClient() -> PeanutClient {
        guard let client else { preconditionFailure("PeanutAuthClient is not attached") }
        return client
    }
}

public final class PeanutDataClient: @unchecked Sendable {
    weak var client: PeanutClient?

    init() {}

    public func listTables() async throws -> PeanutJSONObject {
        let client = requireClient()
        return try await client.requestJSON(PeanutJSONObject.self, method: "GET", path: client.appPath("/data/tables"))
    }

    public func getTable(_ table: String) async throws -> PeanutJSONObject {
        let client = requireClient()
        return try await client.requestJSON(PeanutJSONObject.self, method: "GET", path: client.appPath("/data/tables/\(PeanutClient.segment(table))"))
    }

    public func listRows(_ table: String, params: [String: String] = [:]) async throws -> PeanutJSONObject {
        let client = requireClient()
        return try await client.requestJSON(PeanutJSONObject.self, method: "GET", path: client.appPath("/data/tables/\(PeanutClient.segment(table))/rows\(PeanutClient.query(params))"))
    }

    public func createRow(_ table: String, data: PeanutJSONValue) async throws -> PeanutJSONObject {
        let client = requireClient()
        return try await client.requestJSON(PeanutJSONObject.self, method: "POST", path: client.appPath("/data/tables/\(PeanutClient.segment(table))/rows"), body: ["data": data])
    }

    public func getRow(_ table: String, rowId: String) async throws -> PeanutJSONObject {
        let client = requireClient()
        return try await client.requestJSON(PeanutJSONObject.self, method: "GET", path: client.appPath("/data/tables/\(PeanutClient.segment(table))/rows/\(PeanutClient.segment(rowId))"))
    }

    public func updateRow(_ table: String, rowId: String, data: PeanutJSONValue) async throws -> PeanutJSONObject {
        let client = requireClient()
        return try await client.requestJSON(PeanutJSONObject.self, method: "PATCH", path: client.appPath("/data/tables/\(PeanutClient.segment(table))/rows/\(PeanutClient.segment(rowId))"), body: ["data": data])
    }

    public func deleteRow(_ table: String, rowId: String) async throws {
        let client = requireClient()
        _ = try await client.requestJSON(EmptyResponse.self, method: "DELETE", path: client.appPath("/data/tables/\(PeanutClient.segment(table))/rows/\(PeanutClient.segment(rowId))"))
    }

    private func requireClient() -> PeanutClient {
        guard let client else { preconditionFailure("PeanutDataClient is not attached") }
        return client
    }
}

public final class PeanutStorageClient: @unchecked Sendable {
    weak var client: PeanutClient?

    init() {}

    public func listObjects(bucket: String, prefix: String? = nil) async throws -> PeanutJSONObject {
        let client = requireClient()
        let query = prefix.map { "?prefix=\(PeanutClient.segment($0))" } ?? ""
        return try await client.requestJSON(PeanutJSONObject.self, method: "GET", path: client.appPath("/storage/buckets/\(PeanutClient.segment(bucket))/objects\(query)"))
    }

    public func getObject(bucket: String, key: String) async throws -> (Data, HTTPURLResponse) {
        let client = requireClient()
        return try await client.requestBytes(method: "GET", path: client.appPath("/storage/buckets/\(PeanutClient.segment(bucket))/objects/\(PeanutClient.keyPath(key))"))
    }

    public func putObject(bucket: String, key: String, body: Data, contentType: String = "application/octet-stream") async throws -> HTTPURLResponse {
        let client = requireClient()
        let (_, response) = try await client.requestBytes(method: "PUT", path: client.appPath("/storage/buckets/\(PeanutClient.segment(bucket))/objects/\(PeanutClient.keyPath(key))"), body: body, contentType: contentType)
        return response
    }

    public func deleteObject(bucket: String, key: String) async throws {
        let client = requireClient()
        _ = try await client.requestJSON(EmptyResponse.self, method: "DELETE", path: client.appPath("/storage/buckets/\(PeanutClient.segment(bucket))/objects/\(PeanutClient.keyPath(key))"))
    }

    private func requireClient() -> PeanutClient {
        guard let client else { preconditionFailure("PeanutStorageClient is not attached") }
        return client
    }
}

public final class PeanutPushClient: @unchecked Sendable {
    weak var client: PeanutClient?

    init() {}

    public func listSubscriptions() async throws -> PeanutJSONObject {
        let client = requireClient()
        return try await client.requestJSON(PeanutJSONObject.self, method: "GET", path: client.appPath("/push/subscriptions"))
    }

    public func createNtfySubscription(topic: String) async throws -> PeanutJSONObject {
        let client = requireClient()
        return try await client.requestJSON(PeanutJSONObject.self, method: "POST", path: client.appPath("/push/subscriptions"), body: ["topic": PeanutJSONValue.string(topic)])
    }

    public func createWebPushSubscription(endpoint: String, p256dh: String, auth: String) async throws -> PeanutJSONObject {
        let client = requireClient()
        return try await client.requestJSON(PeanutJSONObject.self, method: "POST", path: client.appPath("/push/subscriptions"), body: [
            "endpoint": PeanutJSONValue.string(endpoint),
            "keys": PeanutJSONValue.object([
                "p256dh": PeanutJSONValue.string(p256dh),
                "auth": PeanutJSONValue.string(auth)
            ])
        ])
    }

    public func deleteSubscription(_ subscriptionId: Int64) async throws {
        let client = requireClient()
        _ = try await client.requestJSON(EmptyResponse.self, method: "DELETE", path: client.appPath("/push/subscriptions/\(subscriptionId)"))
    }

    public func getVapidPublicKey() async throws -> PeanutJSONObject {
        let client = requireClient()
        return try await client.requestJSON(PeanutJSONObject.self, method: "GET", path: client.appPath("/push/vapid-public-key"))
    }

    public func enqueueMessage(title: String, body: String, userId: String? = nil) async throws -> PeanutJSONObject {
        let client = requireClient()
        var payload: PeanutJSONObject = ["title": .string(title), "body": .string(body)]
        if let userId {
            payload["user_id"] = .string(userId)
        }
        return try await client.requestJSON(PeanutJSONObject.self, method: "POST", path: client.appPath("/push/messages"), body: payload)
    }

    private func requireClient() -> PeanutClient {
        guard let client else { preconditionFailure("PeanutPushClient is not attached") }
        return client
    }
}

public final class PeanutFunctionsClient: @unchecked Sendable {
    weak var client: PeanutClient?

    init() {}

    public func invoke(endpointSlug: String, input: PeanutJSONValue = .null, apiKey: String? = nil, asyncInvoke: Bool? = nil) async throws -> PeanutJSONObject {
        let client = requireClient()
        var payload: PeanutJSONObject = ["input": input]
        if let apiKey {
            payload["api_key"] = .string(apiKey)
        }
        if let asyncInvoke {
            payload["async_invoke"] = .bool(asyncInvoke)
        }
        return try await client.requestJSON(PeanutJSONObject.self, method: "POST", path: client.appPath("/functions/endpoints/\(PeanutClient.segment(endpointSlug))"), body: payload)
    }

    private func requireClient() -> PeanutClient {
        guard let client else { preconditionFailure("PeanutFunctionsClient is not attached") }
        return client
    }
}

private struct ErrorEnvelope: Decodable {
    let error: String
}

private struct AnyEncodable: Encodable {
    private let encodeClosure: (Encoder) throws -> Void

    init(_ value: Encodable) {
        self.encodeClosure = value.encode
    }

    func encode(to encoder: Encoder) throws {
        try encodeClosure(encoder)
    }
}
