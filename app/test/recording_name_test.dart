import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:listening_desk/main.dart';

Map<String, dynamic> recording(String id, String title, String phase) => {
  'id': id,
  'title': title,
  'phase': phase,
  'createdAt': 1700000000000,
  'durationMs': 2000,
  'source': 'mic',
  'pair': 'en-zh',
  'segments': [
    {'id': 'first', 'text': 'Original', 'translation': '译文'},
  ],
  'sections': [],
  'error': '',
};

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  final calls = <MethodCall>[];
  final renames = <Map<String, dynamic>>[];
  late Map<String, dynamic> current;
  late Map<String, dynamic> archived;
  bool failSave = false;

  setUp(() async {
    await AppStrings.load('zh');
    calls.clear();
    renames.clear();
    failSave = false;
    current = recording('1001', '本次录音', 'recording');
    archived = recording('900', '以前的录音', 'ended');
    messenger.setMockMethodCallHandler(PlatformDesk.channel, (call) async {
      calls.add(call);
      if (call.method == 'settings') {
        return jsonEncode({...defaults, 'effectiveLocale': 'zh'});
      }
      final request = jsonDecode(call.arguments as String? ?? '{}') as Map;
      if (call.method == 'core') {
        if (request['op'] == 'list') {
          return jsonEncode([
            archived,
            if (current['phase'] == 'ended') current,
          ]);
        }
        if (request['op'] == 'rename') {
          renames.add(Map<String, dynamic>.from(request));
          if (failSave) return jsonEncode({'error': '字幕保存失败'});
          final record = request['id'] == current['id'] ? current : archived;
          record['title'] = request['title'];
          return jsonEncode(record);
        }
        return jsonEncode(current);
      }
      if (call.method == 'digest') {
        return jsonEncode({...archived, 'digestStatus': 'ready'});
      }
      return '{}';
    });
    messenger.setMockMethodCallHandler(
      const MethodChannel('desk/events'),
      (_) async => null,
    );
  });

  tearDown(() {
    messenger.setMockMethodCallHandler(PlatformDesk.channel, null);
    messenger.setMockMethodCallHandler(
      const MethodChannel('desk/events'),
      null,
    );
  });

  Finder getNameField() => find.descendant(
    of: find.byType(AlertDialog),
    matching: find.byType(TextField),
  );
  Finder saveButton() => find.widgetWithText(FilledButton, tr('保存'));
  Finder cancelButton() => find.widgetWithText(TextButton, tr('取消'));

  Future<void> emit(WidgetTester tester) async {
    await messenger.handlePlatformMessage(
      'desk/events',
      const StandardMethodCodec().encodeSuccessEnvelope(jsonEncode(current)),
      (_) {},
    );
    await tester.pumpAndSettle();
  }

  Future<void> showHome(WidgetTester tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(390, 844);
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(const DeskApp());
    await tester.pumpAndSettle();
    await emit(tester);
  }

  Future<void> openName(WidgetTester tester) async {
    await tester.tap(find.byKey(const ValueKey('rename-current-recording')));
    await tester.pumpAndSettle();
    expect(
      tester.widget<TextField>(getNameField()).controller!.text,
      current['title'],
    );
  }

  void expectNoRecordingControl() {
    expect(
      calls.where(
        (c) => ['start', 'pause', 'resume', 'stop'].contains(c.method),
      ),
      isEmpty,
    );
  }

  for (final phase in ['recording', 'paused', 'ended']) {
    testWidgets(
      'rename in $phase persists name while retaining captions and phase',
      (tester) async {
        current['phase'] = phase;
        await showHome(tester);
        await openName(tester);
        await tester.enterText(getNameField(), '  会議 ✨ 新名称  ');
        // Incoming progress cannot discard the name draft.
        current['durationMs'] = 7000;
        await emit(tester);
        expect(
          tester.widget<TextField>(getNameField()).controller!.text,
          '  会議 ✨ 新名称  ',
        );
        await tester.tap(saveButton());
        await tester.pumpAndSettle();
        expect(renames.single, {
          'op': 'rename',
          'id': '1001',
          'title': '会議 ✨ 新名称',
        });
        expect(find.byType(AlertDialog), findsNothing);
        expect(find.text('会議 ✨ 新名称'), findsOneWidget);
        expect(current['phase'], phase);
        expect(find.text('译文'), findsOneWidget);
        current['durationMs'] = 9000;
        await emit(tester);
        expect(find.text('会議 ✨ 新名称'), findsOneWidget);
        expect(find.text('00:00:09'), findsOneWidget);
        expectNoRecordingControl();
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox());
      },
    );
  }

  testWidgets('current name stays editable on small and landscape screens', (
    tester,
  ) async {
    await showHome(tester);
    for (final size in [const Size(320, 640), const Size(800, 360)]) {
      tester.view.physicalSize = size;
      for (final phase in ['recording', 'paused', 'ended']) {
        current['phase'] = phase;
        current['title'] = '带有较长名称的录音记录 ' * 10;
        await emit(tester);
        expect(tester.takeException(), isNull, reason: '$size $phase');
        await openName(tester);
        await tester.enterText(getNameField(), '屏幕旋转后改名');
        await tester.tap(saveButton());
        await tester.pumpAndSettle();
        expect(
          find.descendant(
            of: find.byKey(const ValueKey('rename-current-recording')),
            matching: find.text('屏幕旋转后改名'),
          ),
          findsOneWidget,
        );
        expect(tester.takeException(), isNull, reason: '$size $phase saved');
      }
    }
    expectNoRecordingControl();
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('empty name and cancel preserve the name without a native save', (
    tester,
  ) async {
    await showHome(tester);
    await openName(tester);
    await tester.enterText(getNameField(), '   ');
    await tester.tap(saveButton());
    await tester.pumpAndSettle();
    expect(find.text(tr('记录名称不能为空')), findsOneWidget);
    expect(renames, isEmpty);
    await tester.tap(cancelButton());
    await tester.pumpAndSettle();
    expect(find.text('本次录音'), findsOneWidget);
    expectNoRecordingControl();
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('save failure retains draft and old name, then retries', (
    tester,
  ) async {
    await showHome(tester);
    await openName(tester);
    await tester.enterText(getNameField(), '需要重试的名称');
    failSave = true;
    await tester.tap(saveButton());
    await tester.pumpAndSettle();
    expect(find.text(tr('字幕保存失败')), findsOneWidget);
    expect(
      tester.widget<TextField>(getNameField()).controller!.text,
      '需要重试的名称',
    );
    expect(current['title'], '本次录音');
    failSave = false;
    await tester.tap(saveButton());
    await tester.pumpAndSettle();
    expect(renames, hasLength(2));
    expect(renames[0], renames[1]);
    expect(find.text('需要重试的名称'), findsOneWidget);
    expectNoRecordingControl();
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets(
    'recording settings name remains enabled and opens the same editor',
    (tester) async {
      await showHome(tester);
      await tester.tap(find.byTooltip(tr('录音设置')));
      await tester.pumpAndSettle();
      final field = find.byType(TextField);
      expect(tester.widget<TextField>(field).enabled, isNot(false));
      await tester.tap(field);
      await tester.pumpAndSettle();
      await tester.enterText(getNameField(), '设置中的新名称');
      await tester.tap(saveButton());
      await tester.pumpAndSettle();
      expect(renames.single['id'], '1001');
      expect(current['title'], '设置中的新名称');
      expect(
        tester.widget<TextField>(find.byType(TextField)).controller!.text,
        '设置中的新名称',
      );
      expectNoRecordingControl();
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
    },
  );

  testWidgets(
    'rename history during a different live recording refreshes list without changing current name',
    (tester) async {
      await showHome(tester);
      await tester.tap(find.text(tr('记录')).last);
      await tester.pumpAndSettle();
      await tester.tap(find.text('以前的录音'));
      await tester.pumpAndSettle();
      await tester.tap(find.byTooltip(tr('重命名')));
      await tester.pumpAndSettle();
      await tester.enterText(getNameField(), '新的历史录音名');
      await tester.tap(saveButton());
      await tester.pumpAndSettle();
      expect(find.text('新的历史录音名'), findsOneWidget);
      expect(renames.single['id'], '900');
      current['durationMs'] = 8000;
      await emit(tester);
      expect(find.text('新的历史录音名'), findsOneWidget);
      expect(find.textContaining('00:00:08'), findsOneWidget);
      await tester.tap(find.byType(BackButton));
      await tester.pumpAndSettle();
      expect(find.text('新的历史录音名'), findsOneWidget);
      expect(find.text('以前的录音'), findsNothing);
      expect(current['title'], '本次录音');
      expectNoRecordingControl();
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
    },
  );
}
