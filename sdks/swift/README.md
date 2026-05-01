# Peanut Swift SDK

```swift
import Foundation
import PeanutSDK

let peanut = PeanutClient(
    baseURL: URL(string: "http://localhost:8080")!,
    appId: "default",
    apiKey: "pk_..."
)

let session = try await peanut.auth.login(email: "me@example.com", password: "password123")
let rows = try await peanut.data.listRows("notes", params: ["limit": "10"])
_ = try await peanut.storage.putObject(
    bucket: "assets",
    key: "hello.txt",
    body: Data("hi".utf8),
    contentType: "text/plain"
)
_ = try await peanut.functions.invoke(
    endpointSlug: "hello",
    input: .object(["name": .string("Peanut")])
)
```

The package supports iOS 15+ and macOS 12+.
