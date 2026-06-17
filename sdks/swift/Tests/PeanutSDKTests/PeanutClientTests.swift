import Foundation
import XCTest
@testable import PeanutSDK

final class PeanutClientTests: XCTestCase {
    override func setUp() {
        super.setUp()
        MockURLProtocol.requests = []
        MockURLProtocol.bodies = []
        MockURLProtocol.responses = []
    }

    func testRequestSendsAppKeyAndBearerToken() async throws {
        MockURLProtocol.responses = [
            (200, Data(#"{"tables":[]}"#.utf8))
        ]
        let peanut = PeanutClient(
            baseURL: URL(string: "https://peanut.test")!,
            appId: "app_1",
            apiKey: "pk_test",
            accessToken: "jwt_test",
            session: makeSession()
        )

        _ = try await peanut.data.listTables()

        let request = try XCTUnwrap(MockURLProtocol.requests.first)
        XCTAssertEqual(request.value(forHTTPHeaderField: "X-Peanut-Api-Key"), "pk_test")
        XCTAssertEqual(request.value(forHTTPHeaderField: "Authorization"), "Bearer jwt_test")
    }

    func testRequestRetriesTransientFailures() async throws {
        MockURLProtocol.responses = [
            (503, Data(#"{"error":"temporary"}"#.utf8)),
            (200, Data(#"{"tables":[]}"#.utf8))
        ]
        let peanut = PeanutClient(
            baseURL: URL(string: "https://peanut.test")!,
            appId: "app_1",
            apiKey: "pk_test",
            retry: PeanutRetryOptions(maxRetries: 1, baseDelayMilliseconds: 1),
            session: makeSession()
        )

        _ = try await peanut.data.listTables()

        XCTAssertEqual(MockURLProtocol.requests.count, 2)
    }

    func testDataExecuteSqlPostsToAppQueryEndpoint() async throws {
        MockURLProtocol.responses = [
            (200, Data(#"{"statement":"select","table":"notes","columns":["title"],"rows":[{"title":"hello"}]}"#.utf8))
        ]
        let peanut = PeanutClient(
            baseURL: URL(string: "https://peanut.test")!,
            appId: "app_1",
            apiKey: "pk_test",
            session: makeSession()
        )

        _ = try await peanut.data.executeSql("select title from notes")

        let request = try XCTUnwrap(MockURLProtocol.requests.first)
        XCTAssertEqual(request.url?.absoluteString, "https://peanut.test/api/apps/app_1/data/query")
        XCTAssertEqual(request.httpMethod, "POST")
        let body = try XCTUnwrap(MockURLProtocol.bodies.first)
        let payload = try JSONSerialization.jsonObject(with: body) as? [String: String]
        XCTAssertEqual(payload?["sql"], "select title from notes")
    }

    private func makeSession() -> URLSession {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [MockURLProtocol.self]
        return URLSession(configuration: configuration)
    }
}

final class MockURLProtocol: URLProtocol {
    nonisolated(unsafe) static var requests: [URLRequest] = []
    nonisolated(unsafe) static var bodies: [Data] = []
    nonisolated(unsafe) static var responses: [(Int, Data)] = []

    override class func canInit(with request: URLRequest) -> Bool {
        true
    }

    override class func canonicalRequest(for request: URLRequest) -> URLRequest {
        request
    }

    override func startLoading() {
        Self.requests.append(request)
        Self.bodies.append(Self.bodyData(from: request))
        let (status, body) = Self.responses.isEmpty
            ? (500, Data())
            : Self.responses.removeFirst()
        let response = HTTPURLResponse(
            url: request.url!,
            statusCode: status,
            httpVersion: "HTTP/1.1",
            headerFields: ["Content-Type": "application/json"]
        )!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: body)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}

    private static func bodyData(from request: URLRequest) -> Data {
        if let body = request.httpBody {
            return body
        }
        guard let stream = request.httpBodyStream else {
            return Data()
        }
        stream.open()
        defer { stream.close() }
        var data = Data()
        let bufferSize = 1024
        let buffer = UnsafeMutablePointer<UInt8>.allocate(capacity: bufferSize)
        defer { buffer.deallocate() }
        while stream.hasBytesAvailable {
            let count = stream.read(buffer, maxLength: bufferSize)
            if count <= 0 {
                break
            }
            data.append(buffer, count: count)
        }
        return data
    }
}
