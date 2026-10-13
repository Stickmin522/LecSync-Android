import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:listening_desk/main.dart';

Map<String, dynamic> captionRecord({String rowType = 'caption'}) => {
  'id': '1001',
  'title': '测试录音',
  'phase': 'ended',
  'durationMs': 2000,
  rowType == 'caption' ? 'captions' : 'segments': [
    {
      'id': 'first',
      'text': '原文一',
      'translation': '译文一',
      'speaker': '1',
      'startMs': 0,
      'endMs': 1000,
    },
    {
      'id': 'second',
      'text': '原文二',
      'translation': '译文二',
      'speaker': '2',
      'startMs': 1000,
      'endMs': 2000,
    },
  ],
};

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  final nativeCalls = <MethodCall>[];
  final edits = <Map<String, dynamic>>[];
  String? clipboardText;
  bool failSave = false;
  Map<String, dynamic> stored = captionRecord();

  setUp(() async {
    await AppStrings.load('zh');
    nativeCalls.clear();
    edits.clear();
    clipboardText = null;
    failSave = false;
    stored = captionRecord();
    messenger.setMockMethodCallHandler(PlatformDesk.channel, (call) async {
      nativeCalls.add(call);
      if (call.method == 'core') {
        final request = Map<String, dynamic>.from(
          jsonDecode(call.arguments as String) as Map,
        );
        if (request['op'] == 'editCaption') {
          edits.add(request);
          if (failSave) return jsonEncode({'error': '字幕保存失败'});
          final key = request['rowType'] == 'caption' ? 'captions' : 'segments';
          stored = Map<String, dynamic>.from(jsonDecode(jsonEncode(stored)));
          final row = (stored[key] as List).cast<Map>().singleWhere(
            (row) => row['id'] == request['rowId'],
          );
          row[request['field']] = request['value'];
          return jsonEncode(stored);
        }
        return jsonEncode(stored);
      }
      return '{}';
    });
    messenger.setMockMethodCallHandler(SystemChannels.platform, (call) async {
      if (call.method == 'Clipboard.setData') {
        clipboardText = (call.arguments as Map)['text'] as String;
      }
      if (call.method == 'Clipboard.getData') return {'text': clipboardText};
      return null;
    });
    messenger.setMockMethodCallHandler(
      const MethodChannel('desk/events'),
      (_) async => null,
    );
  });

  tearDown(() {
    messenger.setMockMethodCallHandler(PlatformDesk.channel, null);
    messenger.setMockMethodCallHandler(SystemChannels.platform, null);
    messenger.setMockMethodCallHandler(
      const MethodChannel('desk/events'),
      null,
    );
  });

  Future<void> showFeed(
    WidgetTester tester, {
    String rowType = 'caption',
    Map<String, dynamic>? record,
    bool floating = false,
    TextDirection direction = TextDirection.ltr,
    double textScale = 1,
  }) async {
    stored = record ?? captionRecord(rowType: rowType);
    await tester.pumpWidget(
      MaterialApp(
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(context)
              .copyWith(textScaler: TextScaler.linear(textScale)),
          child: Directionality(textDirection: direction, child: child!),
        ),
        home: Scaffold(
          body: CaptionFeed(
            record: stored,
            settings: {...defaults, 'fontSize': 32},
            floating: floating,
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
  }

  Finder button(String key) => key == '保存'
      ? find.widgetWithText(FilledButton, tr(key))
      : find.widgetWithText(TextButton, tr(key));

  Future<void> openActions(WidgetTester tester, String text) async {
    await tester.ensureVisible(find.text(text));
    await tester.tap(find.text(text));
    await tester.pumpAndSettle();
    expect(button('复制'), findsOneWidget);
  }

  Future<void> openEditor(WidgetTester tester, String text) async {
    await openActions(tester, text);
    await tester.tap(button('编辑'));
    await tester.pumpAndSettle();
    expect(find.byType(TextField), findsOneWidget);
    expect(
      tester.widget<TextField>(find.byType(TextField)).controller!.text,
      text,
    );
  }

  List<bool> nativeInputStates() => nativeCalls
      .where((call) => call.method == 'overlayTextInput')
      .map(
        (call) =>
            (jsonDecode(call.arguments as String) as Map)['enabled'] as bool,
      )
      .toList();

  for (final field in ['text', 'translation']) {
    final original = field == 'text' ? '原文一' : '译文一';
    testWidgets('tapping $field copies that paragraph only', (tester) async {
      await showFeed(tester);
      await openActions(tester, original);
      await tester.tap(button('复制'));
      await tester.pumpAndSettle();
      expect(clipboardText, original);
      expect(edits, isEmpty);
      expect(find.byType(Dialog), findsNothing);
      expect(find.text('原文二'), findsOneWidget);
      expect(find.text('译文二'), findsOneWidget);
      await tester.pumpWidget(const SizedBox());
    });
    for (final rowType in ['caption', 'segment']) {
      testWidgets('editing $rowType $field changes only the selected field', (
        tester,
      ) async {
        await showFeed(tester, rowType: rowType);
        await openEditor(tester, original);
        const replacement = '修正后的内容';
        await tester.enterText(find.byType(TextField), replacement);
        await tester.tap(button('保存'));
        await tester.pumpAndSettle();
        expect(edits, [
          {
            'op': 'editCaption',
            'id': '1001',
            'rowId': 'first',
            'rowType': rowType,
            'field': field,
            'value': replacement,
          },
        ]);
        expect(find.text(replacement), findsOneWidget);
        expect(find.text(original), findsNothing);
        expect(find.text(field == 'text' ? '译文一' : '原文一'), findsOneWidget);
        expect(find.text('原文二'), findsOneWidget);
        expect(find.text('译文二'), findsOneWidget);
        expect(find.byType(TextField), findsNothing);
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox());
      });
    }
  }

  for (final recordError in ['', '记录里以前的连接错误']) {
    testWidgets(
      'saved View error=$recordError is record state, not edit failure',
      (tester) async {
        await showFeed(
          tester,
          record: {...captionRecord(), 'error': recordError},
        );
        await openEditor(tester, '原文一');
        await tester.enterText(find.byType(TextField), '保存的新原文');
        await tester.tap(button('保存'));
        await tester.pumpAndSettle();
        expect(edits, hasLength(1));
        expect(stored['error'], recordError);
        expect(find.byType(TextField), findsNothing);
        expect(find.byType(Dialog), findsNothing);
        expect(find.text('保存的新原文'), findsOneWidget);
        expect(find.text('原文一'), findsNothing);
        expect(find.text('译文一'), findsOneWidget);
        expect(find.text('原文二'), findsOneWidget);
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox());
      },
    );
  }

  testWidgets('preview without a row identifier can be copied', (tester) async {
    await showFeed(
      tester,
      record: {
        'phase': 'recording',
        'segments': [],
        'preview': '识别中的原文',
        'previewTranslation': '识别中的译文',
      },
    );
    await openActions(tester, '识别中的译文');
    final edit = button('编辑');
    if (edit.evaluate().isNotEmpty) {
      expect(tester.widget<TextButton>(edit).onPressed, isNull);
    }
    await tester.tap(button('复制'));
    await tester.pumpAndSettle();
    expect(clipboardText, '识别中的译文');
    expect(edits, isEmpty);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('cancel preserves the original paragraph and sends no edit', (
    tester,
  ) async {
    await showFeed(tester);
    await openEditor(tester, '原文一');
    await tester.enterText(find.byType(TextField), '取消的修改');
    await tester.tap(button('取消'));
    await tester.pumpAndSettle();
    expect(edits, isEmpty);
    expect(find.text('原文一'), findsOneWidget);
    expect(find.text('取消的修改'), findsNothing);
    expect(find.byType(TextField), findsNothing);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('save failure retains draft and can retry without retyping', (
    tester,
  ) async {
    await showFeed(tester, floating: true);
    await openEditor(tester, '译文一');
    expect(nativeInputStates(), [true]);
    await tester.enterText(find.byType(TextField), '保留待重试译文');
    failSave = true;
    await tester.tap(button('保存'));
    await tester.pumpAndSettle();
    expect(find.textContaining('字幕保存失败'), findsOneWidget);
    expect(find.byType(TextField), findsOneWidget);
    expect(
      tester.widget<TextField>(find.byType(TextField)).controller!.text,
      '保留待重试译文',
    );
    expect(find.text('译文一'), findsOneWidget);
    expect(nativeInputStates(), [true]);
    failSave = false;
    await tester.tap(button('保存'));
    await tester.pumpAndSettle();
    expect(edits.length, 2);
    expect(edits[1], edits[0]);
    expect(find.byType(TextField), findsNothing);
    expect(find.text('保留待重试译文'), findsOneWidget);
    expect(find.text('原文一'), findsOneWidget);
    expect(nativeInputStates(), [true, false]);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('floating copy restores native text interaction after closing', (
    tester,
  ) async {
    await showFeed(tester, floating: true);
    await openActions(tester, '原文一');
    expect(nativeInputStates(), [true]);
    await tester.tap(button('复制'));
    await tester.pumpAndSettle();
    expect(nativeInputStates(), [true, false]);
    expect(clipboardText, '原文一');
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('dismissing floating action menu restores native interaction', (
    tester,
  ) async {
    await showFeed(tester, floating: true);
    await openActions(tester, '原文一');
    expect(nativeInputStates(), [true]);
    await tester.tapAt(const Offset(1, 1));
    await tester.pumpAndSettle();
    expect(find.byType(Dialog), findsNothing);
    expect(nativeInputStates(), [true, false]);
    expect(edits, isEmpty);
    await tester.pumpWidget(const SizedBox());
  });

  for (final direction in [TextDirection.ltr, TextDirection.rtl]) {
    testWidgets('floating menu and editor fit 200x120 in $direction', (
      tester,
    ) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(200, 120);
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      if (direction == TextDirection.rtl) await AppStrings.load('ar');
      await showFeed(
        tester,
        floating: true,
        direction: direction,
        textScale: 1.8,
        record: {
          'id': '1001',
          'phase': 'recording',
          'captions': [
            {'id': 'first', 'text': '原文一', 'translation': '译文一'},
          ],
        },
      );
      await openActions(tester, '原文一');
      expect(nativeInputStates(), [true]);
      expect(tester.takeException(), isNull);
      await tester.ensureVisible(button('编辑'));
      await tester.tap(button('编辑'));
      await tester.pumpAndSettle();
      expect(find.byType(TextField), findsOneWidget);
      expect(tester.takeException(), isNull);
      await tester.ensureVisible(button('取消'));
      await tester.tap(button('取消'));
      await tester.pumpAndSettle();
      expect(nativeInputStates(), [true, false]);
      expect(edits, isEmpty);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
    });
  }
}
