# Peanut Java SDK

Thin Java 17 SDK for Peanut app-scoped APIs.

```java
import java.time.Duration;

PeanutClient peanut = PeanutClient.newBuilder()
    .baseUrl("http://localhost:8080")
    .appId("default")
    .apiKey("pk_...")
    .retry(new PeanutClient.RetryOptions(2, Duration.ofMillis(200)))
    .build();

String login = peanut.auth().login("me@example.com", "password123");
String rows = peanut.data().listRows("notes", Map.of("limit", "10"));
```

The SDK uses only the Java standard library.
