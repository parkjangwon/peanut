import 'dart:async';
import 'dart:convert';

import 'package:http/http.dart' as http;
import 'package:peanut_sdk/peanut_sdk.dart';
import 'package:test/test.dart';

void main() {
  test('request sends app key and bearer token', () async {
    late Map<String, String> seenHeaders;
    final client = PeanutClient(
      baseUrl: 'https://peanut.test',
      appId: 'app_1',
      apiKey: 'pk_test',
      accessToken: 'jwt_test',
      httpClient: _FakeClient((request) {
        seenHeaders = request.headers;
        return http.Response(
          '{"tables":[]}',
          200,
          headers: {'content-type': 'application/json'},
        );
      }),
    );

    await client.data.listTables();

    expect(seenHeaders['x-peanut-api-key'], 'pk_test');
    expect(seenHeaders['authorization'], 'Bearer jwt_test');
  });

  test('request retries transient server failures', () async {
    var attempts = 0;
    final client = PeanutClient(
      baseUrl: 'https://peanut.test',
      appId: 'app_1',
      apiKey: 'pk_test',
      retry: const PeanutRetryOptions(
        maxRetries: 1,
        baseDelay: Duration(milliseconds: 1),
      ),
      httpClient: _FakeClient((request) {
        attempts += 1;
        if (attempts == 1) {
          return http.Response(
            '{"error":"temporary"}',
            503,
            headers: {'content-type': 'application/json'},
          );
        }
        return http.Response(
          '{"tables":[]}',
          200,
          headers: {'content-type': 'application/json'},
        );
      }),
    );

    final response = await client.data.listTables();

    expect(response['tables'], isA<List<Object?>>());
    expect(attempts, 2);
  });

  test('data executeSql posts to app query endpoint', () async {
    late Uri seenUrl;
    late Object? seenBody;
    final client = PeanutClient(
      baseUrl: 'https://peanut.test',
      appId: 'app_1',
      apiKey: 'pk_test',
      httpClient: _FakeClient((request) async {
        seenUrl = request.url;
        if (request is http.Request) {
          seenBody = jsonDecode(request.body);
        }
        return http.Response(
          '{"statement":"select","table":"notes","columns":["title"],"rows":[{"title":"hello"}]}',
          200,
          headers: {'content-type': 'application/json'},
        );
      }),
    );

    final response = await client.data.executeSql('select title from notes');

    expect(seenUrl.toString(), 'https://peanut.test/api/apps/app_1/data/query');
    expect(seenBody, {'sql': 'select title from notes'});
    expect(response['rows'], isA<List<Object?>>());
  });
}

class _FakeClient extends http.BaseClient {
  _FakeClient(this._handler);

  final FutureOr<http.Response> Function(http.BaseRequest request) _handler;

  @override
  Future<http.StreamedResponse> send(http.BaseRequest request) async {
    final response = await _handler(request);
    return http.StreamedResponse(
      Stream.value(response.bodyBytes),
      response.statusCode,
      headers: response.headers,
      request: request,
    );
  }
}
