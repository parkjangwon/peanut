# Peanut SDKs

Official Peanut client SDKs live in this directory.

The SDKs target Peanut's app-scoped API surface:

- `X-Peanut-Api-Key` is always sent.
- `Authorization: Bearer <accessToken>` is sent when a user session is available.
- Requests are scoped under `/api/apps/:appId`.

Current packages:

- `js/` - TypeScript SDK for browser, Node, React Native, and edge runtimes with `fetch`.
- `dart/` - Dart SDK using `package:http`.
- `java/` - Java 17 SDK using the standard `java.net.http.HttpClient`.
- `swift/` - Swift Package Manager SDK using `URLSession`.

The SDKs expose the same service groups:

- Auth: register, login, refresh, logout, me
- Data: tables and row CRUD
- Storage: list, get, put, delete objects
- Push: subscriptions, VAPID public key, enqueue message
- Functions: invoke endpoint

Every SDK now includes a small request test suite and configurable retry handling for transient HTTP failures (`408`, `429`, and `5xx`).

These are intentionally thin wrappers over the HTTP API so they remain easy to audit and keep in sync with Peanut's self-hosted runtime.
