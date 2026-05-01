package com.peanut.sdk;

import java.io.IOException;
import java.net.CookieHandler;
import java.net.ProxySelector;
import java.net.URI;
import java.net.Authenticator;
import java.net.http.HttpClient;
import java.net.http.HttpHeaders;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.Optional;
import java.util.concurrent.CompletableFuture;
import javax.net.ssl.SSLContext;
import javax.net.ssl.SSLParameters;

public final class PeanutClientTest {
    public static void main(String[] args) {
        sendsAppKeyAndBearerToken();
        retriesTransientFailures();
    }

    private static void sendsAppKeyAndBearerToken() {
        FakeHttpClient http = new FakeHttpClient();
        PeanutClient client = PeanutClient.newBuilder()
                .baseUrl("https://peanut.test")
                .appId("app_1")
                .apiKey("pk_test")
                .accessToken("jwt_test")
                .httpClient(http)
                .build();

        client.data().listTables();

        assertEquals("pk_test", http.lastRequest.headers().firstValue("X-Peanut-Api-Key").orElse(null));
        assertEquals("Bearer jwt_test", http.lastRequest.headers().firstValue("Authorization").orElse(null));
    }

    private static void retriesTransientFailures() {
        FakeHttpClient http = new FakeHttpClient();
        http.failOnce = true;
        PeanutClient client = PeanutClient.newBuilder()
                .baseUrl("https://peanut.test")
                .appId("app_1")
                .apiKey("pk_test")
                .retry(new PeanutClient.RetryOptions(1, Duration.ofMillis(1)))
                .httpClient(http)
                .build();

        String response = client.data().listTables();

        assertEquals("{\"tables\":[]}", response);
        assertEquals(2, http.attempts);
    }

    private static void assertEquals(Object expected, Object actual) {
        if (!expected.equals(actual)) {
            throw new AssertionError("expected " + expected + " but got " + actual);
        }
    }

    private static final class FakeHttpClient extends HttpClient {
        int attempts;
        boolean failOnce;
        HttpRequest lastRequest;

        @Override
        public <T> HttpResponse<T> send(HttpRequest request, HttpResponse.BodyHandler<T> responseBodyHandler) throws IOException, InterruptedException {
            attempts += 1;
            lastRequest = request;
            int status = failOnce && attempts == 1 ? 503 : 200;
            String body = status == 503 ? "{\"error\":\"temporary\"}" : "{\"tables\":[]}";
            @SuppressWarnings("unchecked")
            T typedBody = (T) body;
            return new Response<>(request, status, typedBody);
        }

        @Override
        public <T> CompletableFuture<HttpResponse<T>> sendAsync(HttpRequest request, HttpResponse.BodyHandler<T> responseBodyHandler) {
            throw new UnsupportedOperationException();
        }

        @Override
        public <T> CompletableFuture<HttpResponse<T>> sendAsync(HttpRequest request, HttpResponse.BodyHandler<T> responseBodyHandler, HttpResponse.PushPromiseHandler<T> pushPromiseHandler) {
            throw new UnsupportedOperationException();
        }

        @Override
        public Optional<CookieHandler> cookieHandler() { return Optional.empty(); }
        @Override
        public Optional<Duration> connectTimeout() { return Optional.empty(); }
        @Override
        public Redirect followRedirects() { return Redirect.NEVER; }
        @Override
        public Optional<ProxySelector> proxy() { return Optional.empty(); }
        @Override
        public SSLContext sslContext() { return null; }
        @Override
        public SSLParameters sslParameters() { return null; }
        @Override
        public Optional<Authenticator> authenticator() { return Optional.empty(); }
        @Override
        public Version version() { return Version.HTTP_1_1; }
        @Override
        public Optional<java.util.concurrent.Executor> executor() { return Optional.empty(); }
    }

    private record Response<T>(HttpRequest request, int statusCode, T body) implements HttpResponse<T> {
        @Override
        public Optional<HttpResponse<T>> previousResponse() { return Optional.empty(); }
        @Override
        public HttpHeaders headers() { return HttpHeaders.of(java.util.Map.of(), (a, b) -> true); }
        @Override
        public URI uri() { return request.uri(); }
        @Override
        public HttpClient.Version version() { return HttpClient.Version.HTTP_1_1; }
        @Override
        public Optional<javax.net.ssl.SSLSession> sslSession() { return Optional.empty(); }
    }
}
