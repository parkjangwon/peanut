# Peanut JavaScript SDK

```ts
import { PeanutClient } from "@peanut-backend/sdk";

const peanut = new PeanutClient({
  baseUrl: "http://localhost:8080",
  appId: "default",
  apiKey: "pk_...",
  retry: { maxRetries: 2, baseDelayMs: 200 },
  timeoutMs: 30_000
});

const session = await peanut.auth.login("me@example.com", "password123");
const rows = await peanut.data.listRows("notes", { limit: 10 });
await peanut.storage.putObject("assets", "hello.txt", new Blob(["hi"]), "text/plain");
await peanut.functions.invoke("hello", { name: "Peanut" });
```

The SDK works in runtimes with `fetch`: browsers, Node 18+, React Native, workers, and edge runtimes.
