import 'dart:async';
import 'dart:convert';

import 'package:web_socket_channel/web_socket_channel.dart';

class BackendClient {
  BackendClient({required this.endpoint});

  final Uri endpoint;

  WebSocketChannel? _channel;
  StreamSubscription? _subscription;
  int _nextId = 1;
  final Map<int, Completer<Map<String, dynamic>>> _pending = {};

  final _eventsController = StreamController<Map<String, dynamic>>.broadcast();
  Stream<Map<String, dynamic>> get events => _eventsController.stream;

  Future<void> connect() async {
    _channel = WebSocketChannel.connect(endpoint);
    _subscription = _channel!.stream.listen((raw) {
      final payload = jsonDecode(raw as String) as Map<String, dynamic>;
      final id = payload['id'];
      if (id is int && _pending.containsKey(id)) {
        _pending.remove(id)!.complete(payload);
      } else {
        _eventsController.add(payload);
      }
    });
  }

  Future<void> openFile({required String path, required String content}) async {
    await _send('workspace/open', {'path': path, 'content': content});
  }

  Future<void> saveFile({required String path, required String content}) async {
    await _send('workspace/save', {'path': path, 'content': content});
  }

  Future<Map<String, dynamic>> readFile(String path) {
    return _send('workspace/read', {'path': path});
  }

  Future<void> installExtension({
    required String name,
    required String publisher,
    String version = 'latest',
  }) async {
    await _send('extensions/install', {
      'name': name,
      'publisher': publisher,
      'version': version,
    });
  }

  Future<Map<String, dynamic>> _send(String method, Map<String, dynamic> params) {
    final id = _nextId++;
    final completer = Completer<Map<String, dynamic>>();
    _pending[id] = completer;

    final request = {'id': id, 'method': method, 'params': params};
    _channel!.sink.add(jsonEncode(request));
    return completer.future;
  }

  Future<void> dispose() async {
    await _subscription?.cancel();
    await _eventsController.close();
    await _channel?.sink.close();
  }
}
