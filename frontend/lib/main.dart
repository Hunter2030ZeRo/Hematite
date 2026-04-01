import 'package:flutter/material.dart';

import 'backend_client.dart';

void main() {
  runApp(const HematiteApp());
}

class HematiteApp extends StatelessWidget {
  const HematiteApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Hematite IDE',
      debugShowCheckedModeBanner: false,
      theme: ThemeData.dark(useMaterial3: true),
      home: const EditorScreen(),
    );
  }
}

class EditorScreen extends StatefulWidget {
  const EditorScreen({super.key});

  @override
  State<EditorScreen> createState() => _EditorScreenState();
}

class _EditorScreenState extends State<EditorScreen> {
  final _pathController = TextEditingController(text: 'main.rs');
  final _extensionController = TextEditingController(text: 'rust-analyzer');
  final _editorController = TextEditingController();

  static const _backendUrl = String.fromEnvironment(
    'HEMATITE_BACKEND_WS',
    defaultValue: 'ws://127.0.0.1:8989/rpc',
  );

  final _client = BackendClient(endpoint: Uri.parse(_backendUrl));
  final List<String> _log = [];

  @override
  void initState() {
    super.initState();
    _client.connect().then((_) {
      _append('Connected to backend.');
      _client.events.listen((event) {
        _append('Event: $event');
      });
    }).catchError((Object error) {
      _append('Backend connection failed: $error');
    });
  }

  @override
  void dispose() {
    _pathController.dispose();
    _extensionController.dispose();
    _editorController.dispose();
    _client.dispose();
    super.dispose();
  }

  void _append(String message) {
    setState(() => _log.insert(0, message));
  }

  Future<void> _openFile() async {
    await _client.openFile(path: _pathController.text, content: _editorController.text);
    _append('Opened ${_pathController.text} in workspace cache.');
  }

  Future<void> _saveFile() async {
    await _client.saveFile(path: _pathController.text, content: _editorController.text);
    _append('Saved ${_pathController.text}.');
  }

  Future<void> _loadFile() async {
    final response = await _client.readFile(_pathController.text);
    final content = (response['result'] as Map<String, dynamic>)['content'] as String;
    _editorController.text = content;
    _append('Loaded ${_pathController.text}.');
  }

  Future<void> _installExtension() async {
    await _client.installExtension(name: _extensionController.text, publisher: 'open-vsx');
    _append('Requested extension install: ${_extensionController.text}.');
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Hematite IDE (Flutter + Rust)')),
      body: Row(
        children: [
          Expanded(
            flex: 3,
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                children: [
                  Row(
                    children: [
                      Expanded(
                        child: TextField(
                          controller: _pathController,
                          decoration: const InputDecoration(labelText: 'Workspace file path'),
                        ),
                      ),
                      const SizedBox(width: 12),
                      FilledButton(onPressed: _openFile, child: const Text('Open')),
                      const SizedBox(width: 8),
                      FilledButton(onPressed: _loadFile, child: const Text('Load')),
                      const SizedBox(width: 8),
                      FilledButton(onPressed: _saveFile, child: const Text('Save')),
                    ],
                  ),
                  const SizedBox(height: 16),
                  Expanded(
                    child: TextField(
                      controller: _editorController,
                      expands: true,
                      maxLines: null,
                      minLines: null,
                      textAlignVertical: TextAlignVertical.top,
                      decoration: const InputDecoration(
                        border: OutlineInputBorder(),
                        labelText: 'Editor',
                        alignLabelWithHint: true,
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ),
          Container(width: 1, color: Colors.white24),
          Expanded(
            flex: 2,
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  TextField(
                    controller: _extensionController,
                    decoration: const InputDecoration(labelText: 'Extension Name (Open VSX)'),
                  ),
                  const SizedBox(height: 8),
                  FilledButton(
                    onPressed: _installExtension,
                    child: const Text('Install Extension'),
                  ),
                  const SizedBox(height: 16),
                  const Text('Event Log'),
                  const SizedBox(height: 8),
                  Expanded(
                    child: DecoratedBox(
                      decoration: BoxDecoration(
                        border: Border.all(color: Colors.white24),
                        borderRadius: BorderRadius.circular(8),
                      ),
                      child: ListView.builder(
                        reverse: true,
                        itemCount: _log.length,
                        itemBuilder: (context, index) => ListTile(
                          dense: true,
                          title: Text(_log[index], style: const TextStyle(fontSize: 12)),
                        ),
                      ),
                    ),
                  )
                ],
              ),
            ),
          )
        ],
      ),
    );
  }
}
