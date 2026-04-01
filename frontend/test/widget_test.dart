import 'package:flutter_test/flutter_test.dart';
import 'package:hematite_editor/main.dart';

void main() {
  testWidgets('renders root app bar title', (tester) async {
    await tester.pumpWidget(const HematiteApp());
    expect(find.text('Hematite IDE (Flutter + Rust)'), findsOneWidget);
  });
}
