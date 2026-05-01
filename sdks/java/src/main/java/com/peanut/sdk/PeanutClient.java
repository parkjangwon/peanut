package com.peanut.sdk;

import java.io.IOException;
import java.net.URI;
import java.net.URLEncoder;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.Map;
import java.util.Objects;
import java.util.Optional;
import java.util.StringJoiner;

public final class PeanutClient {
    private final String baseUrl;
    private final String appId;
    private final String apiKey;
    private final HttpClient httpClient;
    private volatile String accessToken;

    private final Auth auth = new Auth();
    private final Data data = new Data();
    private final Storage storage = new Storage();
    private final Push push = new Push();
    private final Functions functions = new Functions();

    private PeanutClient(Builder builder) {
        this.baseUrl = stripTrailingSlash(required(builder.baseUrl, "baseUrl"));
        this.appId = required(builder.appId, "appId");
        this.apiKey = required(builder.apiKey, "apiKey");
        this.accessToken = builder.accessToken;
        this.httpClient = builder.httpClient == null
                ? HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(10)).build()
                : builder.httpClient;
    }

    public static Builder newBuilder() {
        return new Builder();
    }

    public Auth auth() {
        return auth;
    }

    public Data data() {
        return data;
    }

    public Storage storage() {
        return storage;
    }

    public Push push() {
        return push;
    }

    public Functions functions() {
        return functions;
    }

    public void setAccessToken(String accessToken) {
        this.accessToken = accessToken;
    }

    public String requestJson(String method, String path, String jsonBody) {
        HttpRequest.Builder builder = baseRequest(method, path)
                .header("Accept", "application/json");
        if (jsonBody == null) {
            builder.method(method, HttpRequest.BodyPublishers.noBody());
        } else {
            builder.header("Content-Type", "application/json")
                    .method(method, HttpRequest.BodyPublishers.ofString(jsonBody));
        }
        return sendText(builder.build());
    }

    public HttpResponse<byte[]> requestBytes(String method, String path, byte[] body, String contentType) {
        HttpRequest.Builder builder = baseRequest(method, path);
        if (body == null) {
            builder.method(method, HttpRequest.BodyPublishers.noBody());
        } else {
            builder.header("Content-Type", contentType == null ? "application/octet-stream" : contentType)
                    .method(method, HttpRequest.BodyPublishers.ofByteArray(body));
        }
        return sendBytes(builder.build());
    }

    private HttpRequest.Builder baseRequest(String method, String path) {
        HttpRequest.Builder builder = HttpRequest.newBuilder(URI.create(baseUrl + path))
                .header("X-Peanut-Api-Key", apiKey)
                .timeout(Duration.ofSeconds(30));
        String token = accessToken;
        if (token != null && !token.isBlank()) {
            builder.header("Authorization", "Bearer " + token);
        }
        return builder;
    }

    private String appPath(String path) {
        return "/api/apps/" + segment(appId) + path;
    }

    private String sendText(HttpRequest request) {
        try {
            HttpResponse<String> response = httpClient.send(request, HttpResponse.BodyHandlers.ofString());
            if (response.statusCode() < 200 || response.statusCode() >= 300) {
                throw new PeanutException(response.statusCode(), response.body());
            }
            return response.body();
        } catch (IOException e) {
            throw new PeanutException("Peanut request failed", e);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new PeanutException("Peanut request interrupted", e);
        }
    }

    private HttpResponse<byte[]> sendBytes(HttpRequest request) {
        try {
            HttpResponse<byte[]> response = httpClient.send(request, HttpResponse.BodyHandlers.ofByteArray());
            if (response.statusCode() < 200 || response.statusCode() >= 300) {
                throw new PeanutException(response.statusCode(), new String(response.body(), StandardCharsets.UTF_8));
            }
            return response;
        } catch (IOException e) {
            throw new PeanutException("Peanut request failed", e);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new PeanutException("Peanut request interrupted", e);
        }
    }

    public final class Auth {
        public String register(String email, String password) {
            return requestJson("POST", appPath("/auth/register"), object(
                    "email", email,
                    "password", password
            ));
        }

        public String login(String email, String password) {
            String response = requestJson("POST", appPath("/auth/login"), object(
                    "email", email,
                    "password", password
            ));
            Optional<String> token = Json.extractString(response, "access_token");
            token.ifPresent(PeanutClient.this::setAccessToken);
            return response;
        }

        public String refresh(String refreshToken) {
            String response = requestJson("POST", appPath("/auth/refresh"), object("refresh_token", refreshToken));
            Optional<String> token = Json.extractString(response, "access_token");
            token.ifPresent(PeanutClient.this::setAccessToken);
            return response;
        }

        public String logout(String refreshToken) {
            String response = requestJson("POST", appPath("/auth/logout"), object("refresh_token", refreshToken));
            setAccessToken(null);
            return response;
        }

        public String me() {
            return requestJson("GET", appPath("/auth/me"), null);
        }
    }

    public final class Data {
        public String listTables() {
            return requestJson("GET", appPath("/data/tables"), null);
        }

        public String getTable(String table) {
            return requestJson("GET", appPath("/data/tables/" + segment(table)), null);
        }

        public String listRows(String table, Map<String, String> params) {
            return requestJson("GET", appPath("/data/tables/" + segment(table) + "/rows" + query(params)), null);
        }

        public String createRow(String table, String jsonData) {
            return requestJson("POST", appPath("/data/tables/" + segment(table) + "/rows"), "{\"data\":" + jsonData + "}");
        }

        public String getRow(String table, String rowId) {
            return requestJson("GET", appPath("/data/tables/" + segment(table) + "/rows/" + segment(rowId)), null);
        }

        public String updateRow(String table, String rowId, String jsonData) {
            return requestJson("PATCH", appPath("/data/tables/" + segment(table) + "/rows/" + segment(rowId)), "{\"data\":" + jsonData + "}");
        }

        public String deleteRow(String table, String rowId) {
            return requestJson("DELETE", appPath("/data/tables/" + segment(table) + "/rows/" + segment(rowId)), null);
        }
    }

    public final class Storage {
        public String listObjects(String bucket, String prefix) {
            Map<String, String> params = prefix == null ? Map.of() : Map.of("prefix", prefix);
            return requestJson("GET", appPath("/storage/buckets/" + segment(bucket) + "/objects" + query(params)), null);
        }

        public HttpResponse<byte[]> getObject(String bucket, String key) {
            return requestBytes("GET", appPath("/storage/buckets/" + segment(bucket) + "/objects/" + keyPath(key)), null, null);
        }

        public HttpResponse<byte[]> putObject(String bucket, String key, byte[] body, String contentType) {
            return requestBytes("PUT", appPath("/storage/buckets/" + segment(bucket) + "/objects/" + keyPath(key)), body, contentType);
        }

        public String deleteObject(String bucket, String key) {
            return requestJson("DELETE", appPath("/storage/buckets/" + segment(bucket) + "/objects/" + keyPath(key)), null);
        }
    }

    public final class Push {
        public String listSubscriptions() {
            return requestJson("GET", appPath("/push/subscriptions"), null);
        }

        public String createNtfySubscription(String topic) {
            return requestJson("POST", appPath("/push/subscriptions"), object("topic", topic));
        }

        public String createWebPushSubscription(String endpoint, String p256dh, String auth) {
            return requestJson("POST", appPath("/push/subscriptions"),
                    "{\"endpoint\":" + quote(endpoint) + ",\"keys\":{\"p256dh\":" + quote(p256dh) + ",\"auth\":" + quote(auth) + "}}");
        }

        public String deleteSubscription(long subscriptionId) {
            return requestJson("DELETE", appPath("/push/subscriptions/" + subscriptionId), null);
        }

        public String getVapidPublicKey() {
            return requestJson("GET", appPath("/push/vapid-public-key"), null);
        }

        public String enqueueMessage(String title, String body, String userId) {
            String json = userId == null
                    ? object("title", title, "body", body)
                    : object("title", title, "body", body, "user_id", userId);
            return requestJson("POST", appPath("/push/messages"), json);
        }
    }

    public final class Functions {
        public String invoke(String endpointSlug, String jsonInput) {
            return invoke(endpointSlug, jsonInput, null, null);
        }

        public String invoke(String endpointSlug, String jsonInput, String apiKey, Boolean asyncInvoke) {
            StringJoiner fields = new StringJoiner(",", "{", "}");
            fields.add("\"input\":" + (jsonInput == null ? "null" : jsonInput));
            if (apiKey != null) {
                fields.add("\"api_key\":" + quote(apiKey));
            }
            if (asyncInvoke != null) {
                fields.add("\"async_invoke\":" + asyncInvoke);
            }
            return requestJson("POST", appPath("/functions/endpoints/" + segment(endpointSlug)), fields.toString());
        }
    }

    public static final class Builder {
        private String baseUrl;
        private String appId;
        private String apiKey;
        private String accessToken;
        private HttpClient httpClient;

        public Builder baseUrl(String baseUrl) {
            this.baseUrl = baseUrl;
            return this;
        }

        public Builder appId(String appId) {
            this.appId = appId;
            return this;
        }

        public Builder apiKey(String apiKey) {
            this.apiKey = apiKey;
            return this;
        }

        public Builder accessToken(String accessToken) {
            this.accessToken = accessToken;
            return this;
        }

        public Builder httpClient(HttpClient httpClient) {
            this.httpClient = httpClient;
            return this;
        }

        public PeanutClient build() {
            return new PeanutClient(this);
        }
    }

    private static String required(String value, String field) {
        if (value == null || value.isBlank()) {
            throw new IllegalArgumentException(field + " is required");
        }
        return value;
    }

    private static String stripTrailingSlash(String value) {
        return value.replaceFirst("/+$", "");
    }

    private static String segment(String value) {
        return URLEncoder.encode(value, StandardCharsets.UTF_8).replace("+", "%20");
    }

    private static String keyPath(String value) {
        String[] parts = value.split("/", -1);
        StringJoiner joiner = new StringJoiner("/");
        for (String part : parts) {
            joiner.add(segment(part));
        }
        return joiner.toString();
    }

    private static String query(Map<String, String> params) {
        if (params == null || params.isEmpty()) {
            return "";
        }
        StringJoiner joiner = new StringJoiner("&", "?", "");
        params.forEach((key, value) -> {
            if (value != null) {
                joiner.add(segment(key) + "=" + segment(value));
            }
        });
        String query = joiner.toString();
        return query.equals("?") ? "" : query;
    }

    private static String object(String... pairs) {
        if (pairs.length % 2 != 0) {
            throw new IllegalArgumentException("pairs must be key/value pairs");
        }
        StringJoiner joiner = new StringJoiner(",", "{", "}");
        for (int i = 0; i < pairs.length; i += 2) {
            joiner.add(quote(pairs[i]) + ":" + quote(pairs[i + 1]));
        }
        return joiner.toString();
    }

    private static String quote(String value) {
        Objects.requireNonNull(value, "value");
        StringBuilder builder = new StringBuilder("\"");
        for (int i = 0; i < value.length(); i++) {
            char ch = value.charAt(i);
            switch (ch) {
                case '"' -> builder.append("\\\"");
                case '\\' -> builder.append("\\\\");
                case '\b' -> builder.append("\\b");
                case '\f' -> builder.append("\\f");
                case '\n' -> builder.append("\\n");
                case '\r' -> builder.append("\\r");
                case '\t' -> builder.append("\\t");
                default -> {
                    if (ch < 0x20) {
                        builder.append(String.format("\\u%04x", (int) ch));
                    } else {
                        builder.append(ch);
                    }
                }
            }
        }
        return builder.append('"').toString();
    }
}
