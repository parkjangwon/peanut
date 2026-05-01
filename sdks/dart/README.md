# Peanut Dart SDK

```dart
import 'dart:convert';
import 'dart:typed_data';

import 'package:peanut_sdk/peanut_sdk.dart';

final peanut = PeanutClient(
  baseUrl: 'http://localhost:8080',
  appId: 'default',
  apiKey: 'pk_...',
);

final session = await peanut.auth.login('me@example.com', 'password123');
final rows = await peanut.data.listRows('notes', {'limit': 10});
await peanut.storage.putObject(
  'assets',
  'hello.txt',
  Uint8List.fromList(utf8.encode('hi')),
  contentType: 'text/plain',
);
await peanut.functions.invoke('hello', input: {'name': 'Peanut'});
```

This package uses `package:http`, so it works in Dart and Flutter targets supported by that package.
