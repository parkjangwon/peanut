library;

import 'dart:convert';
import 'dart:typed_data';

import 'package:http/http.dart' as http;

typedef JsonMap = Map<String, Object?>;

class PeanutException implements Exception {
  PeanutException(this.statusCode, this.message, this.body);

  final int statusCode;
  final String message;
  final Object? body;

  @override
  String toString() => 'PeanutException($statusCode): $message';
}

class PeanutRetryOptions {
  const PeanutRetryOptions({
    this.maxRetries = 0,
    this.baseDelay = const Duration(milliseconds: 200),
  });

  final int maxRetries;
  final Duration baseDelay;
}

class PeanutClient {
  PeanutClient({
    required String baseUrl,
    required this.appId,
    required this.apiKey,
    String? accessToken,
    this.retry = const PeanutRetryOptions(),
    this.timeout = const Duration(seconds: 30),
    http.Client? httpClient,
  }) : baseUrl = baseUrl.replaceFirst(RegExp(r'/+$'), ''),
       _accessToken = accessToken,
       _httpClient = httpClient ?? http.Client() {
    auth = PeanutAuthClient(this);
    data = PeanutDataClient(this);
    storage = PeanutStorageClient(this);
    push = PeanutPushClient(this);
    functions = PeanutFunctionsClient(this);
  }

  final String baseUrl;
  final String appId;
  final String apiKey;
  final PeanutRetryOptions retry;
  final Duration timeout;
  final http.Client _httpClient;
  String? _accessToken;

  late final PeanutAuthClient auth;
  late final PeanutDataClient data;
  late final PeanutStorageClient storage;
  late final PeanutPushClient push;
  late final PeanutFunctionsClient functions;

  void setAccessToken(String? accessToken) {
    _accessToken = accessToken;
  }

  Future<T> requestJson<T>(String method, String path, {Object? body}) async {
    final headers = _headers(json: body != null);
    final request = http.Request(method, Uri.parse('$baseUrl$path'))
      ..headers.addAll(headers);
    if (body != null) {
      request.body = jsonEncode(body);
    }
    final response = await _sendWithRetry(request);
    if (response.statusCode == 204) {
      return null as T;
    }
    final decoded = _decodeBody(response);
    if (response.statusCode < 200 || response.statusCode >= 300) {
      throw PeanutException(
        response.statusCode,
        _errorMessage(decoded),
        decoded,
      );
    }
    return decoded as T;
  }

  Future<http.Response> requestBytes(
    String method,
    String path, {
    Uint8List? body,
    String? contentType,
  }) async {
    final headers = _headers(json: false);
    if (contentType != null) {
      headers['content-type'] = contentType;
    }
    final request = http.Request(method, Uri.parse('$baseUrl$path'))
      ..headers.addAll(headers);
    if (body != null) {
      request.bodyBytes = body;
    }
    final response = await _sendWithRetry(request);
    if (response.statusCode < 200 || response.statusCode >= 300) {
      final decoded = _decodeBody(response);
      throw PeanutException(
        response.statusCode,
        _errorMessage(decoded),
        decoded,
      );
    }
    return response;
  }

  String appPath(String path) => '/api/apps/${Uri.encodeComponent(appId)}$path';

  Map<String, String> _headers({required bool json}) {
    final headers = <String, String>{'x-peanut-api-key': apiKey};
    final token = _accessToken;
    if (token != null && token.isNotEmpty) {
      headers['authorization'] = 'Bearer $token';
    }
    if (json) {
      headers['content-type'] = 'application/json';
    }
    return headers;
  }

  Future<http.Response> _sendWithRetry(http.Request request) async {
    Object? lastError;
    for (var attempt = 0; attempt <= retry.maxRetries; attempt++) {
      try {
        final response = await http.Response.fromStream(
          await _httpClient.send(_copyRequest(request)).timeout(timeout),
        );
        if (!_isTransientStatus(response.statusCode) ||
            attempt == retry.maxRetries) {
          return response;
        }
      } catch (error) {
        lastError = error;
        if (attempt == retry.maxRetries) {
          rethrow;
        }
      }
      await Future<void>.delayed(retry.baseDelay * (attempt + 1));
    }
    throw lastError ?? StateError('Peanut request failed');
  }

  http.Request _copyRequest(http.Request request) {
    return http.Request(request.method, request.url)
      ..headers.addAll(request.headers)
      ..bodyBytes = request.bodyBytes
      ..followRedirects = request.followRedirects
      ..maxRedirects = request.maxRedirects
      ..persistentConnection = request.persistentConnection;
  }

  Object? _decodeBody(http.Response response) {
    final contentType = response.headers['content-type'] ?? '';
    if (contentType.contains('application/json')) {
      return jsonDecode(response.body);
    }
    return response.body;
  }

  String _errorMessage(Object? body) {
    if (body case {'error': final Object? error}) {
      return error.toString();
    }
    if (body is String && body.isNotEmpty) {
      return body;
    }
    return 'Peanut request failed';
  }
}

bool _isTransientStatus(int statusCode) {
  return statusCode == 408 || statusCode == 429 || statusCode >= 500;
}

class PeanutAuthClient {
  PeanutAuthClient(this._client);

  final PeanutClient _client;

  Future<JsonMap> register(String email, String password) {
    return _client.requestJson<JsonMap>(
      'POST',
      _client.appPath('/auth/register'),
      body: {'email': email, 'password': password},
    );
  }

  Future<JsonMap> login(String email, String password) async {
    final response = await _client.requestJson<JsonMap>(
      'POST',
      _client.appPath('/auth/login'),
      body: {'email': email, 'password': password},
    );
    _client.setAccessToken(response['access_token'] as String?);
    return response;
  }

  Future<JsonMap> refresh(String refreshToken) async {
    final response = await _client.requestJson<JsonMap>(
      'POST',
      _client.appPath('/auth/refresh'),
      body: {'refresh_token': refreshToken},
    );
    _client.setAccessToken(response['access_token'] as String?);
    return response;
  }

  Future<void> logout(String refreshToken) async {
    await _client.requestJson<void>(
      'POST',
      _client.appPath('/auth/logout'),
      body: {'refresh_token': refreshToken},
    );
    _client.setAccessToken(null);
  }

  Future<JsonMap> me() {
    return _client.requestJson<JsonMap>('GET', _client.appPath('/auth/me'));
  }
}

class PeanutDataClient {
  PeanutDataClient(this._client);

  final PeanutClient _client;

  Future<JsonMap> listTables() {
    return _client.requestJson<JsonMap>('GET', _client.appPath('/data/tables'));
  }

  Future<JsonMap> getTable(String table) {
    return _client.requestJson<JsonMap>(
      'GET',
      _client.appPath('/data/tables/${_segment(table)}'),
    );
  }

  Future<JsonMap> listRows(
    String table, [
    Map<String, Object?> params = const {},
  ]) {
    final query = _query(params);
    return _client.requestJson<JsonMap>(
      'GET',
      _client.appPath('/data/tables/${_segment(table)}/rows$query'),
    );
  }

  Future<JsonMap> createRow(String table, Object? data) {
    return _client.requestJson<JsonMap>(
      'POST',
      _client.appPath('/data/tables/${_segment(table)}/rows'),
      body: {'data': data},
    );
  }

  Future<JsonMap> getRow(String table, String rowId) {
    return _client.requestJson<JsonMap>(
      'GET',
      _client.appPath(
        '/data/tables/${_segment(table)}/rows/${_segment(rowId)}',
      ),
    );
  }

  Future<JsonMap> updateRow(String table, String rowId, Object? data) {
    return _client.requestJson<JsonMap>(
      'PATCH',
      _client.appPath(
        '/data/tables/${_segment(table)}/rows/${_segment(rowId)}',
      ),
      body: {'data': data},
    );
  }

  Future<void> deleteRow(String table, String rowId) {
    return _client.requestJson<void>(
      'DELETE',
      _client.appPath(
        '/data/tables/${_segment(table)}/rows/${_segment(rowId)}',
      ),
    );
  }
}

class PeanutStorageClient {
  PeanutStorageClient(this._client);

  final PeanutClient _client;

  Future<JsonMap> listObjects(String bucket, {String? prefix}) {
    final query = prefix == null
        ? ''
        : '?prefix=${Uri.encodeQueryComponent(prefix)}';
    return _client.requestJson<JsonMap>(
      'GET',
      _client.appPath('/storage/buckets/${_segment(bucket)}/objects$query'),
    );
  }

  Future<http.Response> getObject(String bucket, String key) {
    return _client.requestBytes(
      'GET',
      _client.appPath(
        '/storage/buckets/${_segment(bucket)}/objects/${_key(key)}',
      ),
    );
  }

  Future<http.Response> putObject(
    String bucket,
    String key,
    Uint8List body, {
    String contentType = 'application/octet-stream',
  }) {
    return _client.requestBytes(
      'PUT',
      _client.appPath(
        '/storage/buckets/${_segment(bucket)}/objects/${_key(key)}',
      ),
      body: body,
      contentType: contentType,
    );
  }

  Future<void> deleteObject(String bucket, String key) {
    return _client.requestJson<void>(
      'DELETE',
      _client.appPath(
        '/storage/buckets/${_segment(bucket)}/objects/${_key(key)}',
      ),
    );
  }
}

class PeanutPushClient {
  PeanutPushClient(this._client);

  final PeanutClient _client;

  Future<JsonMap> listSubscriptions() {
    return _client.requestJson<JsonMap>(
      'GET',
      _client.appPath('/push/subscriptions'),
    );
  }

  Future<JsonMap> createNtfySubscription(String topic) {
    return _client.requestJson<JsonMap>(
      'POST',
      _client.appPath('/push/subscriptions'),
      body: {'topic': topic},
    );
  }

  Future<JsonMap> createWebPushSubscription(
    String endpoint, {
    required String p256dh,
    required String auth,
  }) {
    return _client.requestJson<JsonMap>(
      'POST',
      _client.appPath('/push/subscriptions'),
      body: {
        'endpoint': endpoint,
        'keys': {'p256dh': p256dh, 'auth': auth},
      },
    );
  }

  Future<void> deleteSubscription(int subscriptionId) {
    return _client.requestJson<void>(
      'DELETE',
      _client.appPath('/push/subscriptions/$subscriptionId'),
    );
  }

  Future<JsonMap> getVapidPublicKey() {
    return _client.requestJson<JsonMap>(
      'GET',
      _client.appPath('/push/vapid-public-key'),
    );
  }

  Future<JsonMap> enqueueMessage({
    required String title,
    required String body,
    String? userId,
  }) {
    final payload = <String, Object?>{'title': title, 'body': body};
    if (userId != null) {
      payload['user_id'] = userId;
    }
    return _client.requestJson<JsonMap>(
      'POST',
      _client.appPath('/push/messages'),
      body: payload,
    );
  }
}

class PeanutFunctionsClient {
  PeanutFunctionsClient(this._client);

  final PeanutClient _client;

  Future<JsonMap> invoke(
    String endpointSlug, {
    Object? input,
    String? apiKey,
    bool? asyncInvoke,
  }) {
    final payload = <String, Object?>{'input': input};
    if (apiKey != null) {
      payload['api_key'] = apiKey;
    }
    if (asyncInvoke != null) {
      payload['async_invoke'] = asyncInvoke;
    }
    return _client.requestJson<JsonMap>(
      'POST',
      _client.appPath('/function-endpoints/${_segment(endpointSlug)}'),
      body: payload,
    );
  }
}

String _segment(String value) => Uri.encodeComponent(value);

String _key(String value) =>
    value.split('/').map(Uri.encodeComponent).join('/');

String _query(Map<String, Object?> params) {
  final query = params.entries
      .where((entry) => entry.value != null)
      .map(
        (entry) =>
            '${Uri.encodeQueryComponent(entry.key)}=${Uri.encodeQueryComponent(entry.value.toString())}',
      )
      .join('&');
  return query.isEmpty ? '' : '?$query';
}
