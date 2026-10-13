import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';

import 'package:flutter/rendering.dart' show ScrollDirection;
import 'package:flutter/services.dart';

import 'languages.dart';
export 'languages.dart';

part 'session_panels.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(const DeskApp());
}

@pragma('vm:entry-point')
void overlayMain() {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(const DeskApp(overlay: true));
}

class PlatformDesk {
  static const channel = MethodChannel('desk/methods');
  static const events = EventChannel('desk/events');
  static final snapshots = events.receiveBroadcastStream();
  static Future<dynamic> call(
    String method, [
    Map<String, dynamic> args = const {},
  ]) async {
    final raw = await channel.invokeMethod<String>(method, jsonEncode(args));
    final value = jsonDecode(raw ?? '{}');
    // Saved/live records carry an error status of their own. Only a failure
    // response without a record ID should fail the requested operation.
    if (value is Map && !value.containsKey('id')) {
      final error = value['error'];
      if (error != null && error.toString().isNotEmpty) {
        throw Exception(error);
      }
    }
    return value;
  }
}

String duration(dynamic value) {
  final n = ((value as num?)?.toInt() ?? 0) ~/ 1000;
  return '${(n ~/ 3600).toString().padLeft(2, '0')}:${(n ~/ 60 % 60).toString().padLeft(2, '0')}:${(n % 60).toString().padLeft(2, '0')}';
}

String phaseLabel(String phase) =>
    {
      'idle': tr('准备就绪'),
      'connecting': tr('正在连接'),
      'recording': tr('正在聆听'),
      'pausing': tr('正在暂停'),
      'paused': tr('录音已暂停'),
      'stopping': tr('正在保存'),
      'ended': tr('已保存'),
    }[phase] ??
    tr('准备就绪');
const defaults = <String, dynamic>{
  'hasKey': false,
  'keyHint': '',
  'pair': 'ja-zh',
  'uiLanguage': 'system',
  'source': 'mic',
  'speakers': true,
  'digest': true,
  'enhance': true,
  'overlay': false,
  'fontSize': 20,
  'captionMode': 'both',
  'theme': 'system',
  'device': 'default',
};

class DeskApp extends StatefulWidget {
  const DeskApp({super.key, this.overlay = false});
  final bool overlay;
  @override
  State<DeskApp> createState() => _DeskAppState();
}

class _DeskAppState extends State<DeskApp> with WidgetsBindingObserver {
  Map<String, dynamic> settings = {...defaults};
  int loadGeneration = 0;
  Future<void> load() async {
    try {
      final generation = ++loadGeneration;
      final value = await PlatformDesk.call('settings');
      if (generation != loadGeneration || !mounted) return;
      await AppStrings.load(value['effectiveLocale'] as String? ?? 'en');
      if (mounted) {
        setState(
          () => settings = {
            ...defaults,
            ...Map<String, dynamic>.from(value as Map),
          },
        );
      }
    } catch (_) {}
  }

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    load();
  }

  @override
  void didChangeLocales(List<Locale>? locales) {
    load();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) load();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  ThemeData theme(Brightness brightness) {
    final dark = brightness == Brightness.dark;
    final scheme =
        ColorScheme.fromSeed(
          seedColor: const Color(0xff245ce8),
          brightness: brightness,
        ).copyWith(
          primary: dark ? const Color(0xff9db9ff) : const Color(0xff245ce8),
          onPrimary: dark ? const Color(0xff102557) : Colors.white,
          primaryContainer: dark
              ? const Color(0xff213d76)
              : const Color(0xffe9effd),
          onPrimaryContainer: dark
              ? const Color(0xffdce6ff)
              : const Color(0xff204fca),
          secondary: dark ? const Color(0xff9db9ff) : const Color(0xff245ce8),
          secondaryContainer: dark
              ? const Color(0xff213d76)
              : const Color(0xffe9effd),
          onSecondaryContainer: dark
              ? const Color(0xffdce6ff)
              : const Color(0xff204fca),
          surface: dark ? const Color(0xff1a2435) : Colors.white,
        );
    return ThemeData(
      useMaterial3: true,
      colorScheme: scheme,
      scaffoldBackgroundColor: dark
          ? const Color(0xff101725)
          : const Color(0xfff3f5f9),
      textTheme:
          (dark
                  ? Typography.material2021().white
                  : Typography.material2021().black)
              .apply(
                bodyColor: dark
                    ? const Color(0xffe5eaf5)
                    : const Color(0xff172033),
                displayColor: dark
                    ? const Color(0xffe5eaf5)
                    : const Color(0xff172033),
              ),
      cardTheme: CardThemeData(
        elevation: 0,
        color: dark ? const Color(0xff1a2435) : Colors.white,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(24)),
      ),
      inputDecorationTheme: InputDecorationTheme(
        filled: true,
        fillColor: scheme.surfaceContainerLow,
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(16),
          borderSide: BorderSide.none,
        ),
      ),
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 16),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(16),
          ),
        ),
      ),
      navigationBarTheme: NavigationBarThemeData(
        backgroundColor: scheme.surface,
      ),
    );
  }

  @override
  Widget build(BuildContext context) => MaterialApp(
    title: tr('听译台'),
    locale: AppStrings.locale,
    supportedLocales: [AppStrings.locale],
    localizationsDelegates: GlobalMaterialLocalizations.delegates,
    builder: (context, child) => Directionality(
      textDirection: AppStrings.rtl ? TextDirection.rtl : TextDirection.ltr,
      child: child!,
    ),
    debugShowCheckedModeBanner: false,
    theme: theme(Brightness.light),
    darkTheme: theme(Brightness.dark),
    themeMode: switch (settings['theme']) {
      'light' => ThemeMode.light,
      'dark' => ThemeMode.dark,
      _ => ThemeMode.system,
    },
    home: widget.overlay
        ? FloatingDesk(settings: settings)
        : DeskHome(settings: settings, reload: load),
  );
}

class DeskHome extends StatefulWidget {
  const DeskHome({super.key, required this.settings, required this.reload});
  final Map<String, dynamic> settings;
  final Future<void> Function() reload;
  @override
  State<DeskHome> createState() => _DeskHomeState();
}

class _DeskHomeState extends State<DeskHome> with WidgetsBindingObserver {
  Map<String, dynamic> state = {
    'phase': 'idle',
    'segments': [],
    'sections': [],
  };
  List<dynamic> history = [];
  StreamSubscription<dynamic>? subscription;
  final title = TextEditingController();
  int page = 0;
  bool actionBusy = false;
  String lastError = '';
  StateSetter? sheetRefresh;
  String get phase => state['phase'] as String? ?? 'idle';
  bool get active => !['idle', 'ended'].contains(phase);
  Map<String, dynamic> get record => state;
  Future<void> refreshHistory() async {
    try {
      final result = await PlatformDesk.call('core', {'op': 'list'});
      if (mounted) setState(() => history = result as List);
    } catch (e) {
      showError(e);
    }
  }

  Future<void> action(Future<void> Function() task) async {
    if (actionBusy) return;
    setState(() => actionBusy = true);
    try {
      await task();
    } catch (e) {
      showError(e);
    } finally {
      if (mounted) setState(() => actionBusy = false);
    }
  }

  void showError(Object e) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text(
          tr(
            e is PlatformException
                ? (e.message ?? tr('操作失败'))
                : e.toString().replaceFirst('Exception: ', ''),
          ),
        ),
      ),
    );
  }

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    subscription = PlatformDesk.snapshots.listen((event) {
      final value = Map<String, dynamic>.from(
        jsonDecode(event as String) as Map,
      );
      if (!mounted) return;
      final ended = value['phase'] == 'ended' && state['phase'] != 'ended';
      setState(() => state = value);
      if (ended) refreshHistory();
      final error = value['error'] as String? ?? '';
      if (error.isNotEmpty && error != lastError) {
        lastError = error;
        showError(error);
      }
    }, onError: (Object e) => showError(e));
    refreshHistory();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState s) {
    if (s == AppLifecycleState.resumed) {
      widget.reload();
      refreshHistory();
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    subscription?.cancel();
    title.dispose();
    super.dispose();
  }

  Future<void> save(Map<String, dynamic> values) async {
    await PlatformDesk.call('saveSettings', values);
    await widget.reload();
    await WidgetsBinding.instance.endOfFrame;
    sheetRefresh?.call(() {});
  }

  Widget iconTitle(IconData icon, String text) => Row(
    children: [
      Icon(icon, size: 20),
      const SizedBox(width: 10),
      Expanded(
        child: Text(
          text,
          style: const TextStyle(fontWeight: FontWeight.w700, fontSize: 17),
        ),
      ),
    ],
  );
  Widget controls() => Card(
    margin: EdgeInsets.zero,
    child: Padding(
      padding: const EdgeInsets.all(20),
      child: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            iconTitle(Icons.tune_rounded, tr('录音设置')),
            const SizedBox(height: 20),
            TextField(
              controller: title,
              enabled: !active,
              decoration: InputDecoration(
                labelText: tr('记录名称'),
                hintText: tr('我的听译记录'),
                prefixIcon: const Icon(Icons.edit_note_rounded),
              ),
            ),
            const SizedBox(height: 16),
            Row(
              children: [
                Expanded(
                  child: LanguageField(
                    key: const ValueKey('language-left'),
                    value: AppStrings.pair(widget.settings['pair'])[0],
                    label: tr('语言一'),
                    exclude: AppStrings.pair(widget.settings['pair'])[1],
                    onChanged: active
                        ? null
                        : (v) => action(
                            () => save({
                              'pair':
                                  '$v-${AppStrings.pair(widget.settings['pair'])[1]}',
                            }),
                          ),
                  ),
                ),
                const Padding(
                  padding: EdgeInsets.symmetric(horizontal: 8),
                  child: Icon(Icons.sync_alt_rounded, size: 20),
                ),
                Expanded(
                  child: LanguageField(
                    key: const ValueKey('language-right'),
                    value: AppStrings.pair(widget.settings['pair'])[1],
                    label: tr('语言二'),
                    exclude: AppStrings.pair(widget.settings['pair'])[0],
                    onChanged: active
                        ? null
                        : (v) => action(
                            () => save({
                              'pair':
                                  '${AppStrings.pair(widget.settings['pair'])[0]}-$v',
                            }),
                          ),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 8),
            Text(tr('自动双向翻译'), style: Theme.of(context).textTheme.bodySmall),
            const SizedBox(height: 18),
            Text(
              tr('采集音源'),
              style: const TextStyle(fontWeight: FontWeight.w600),
            ),
            const SizedBox(height: 8),
            ...[
              ('mic', Icons.mic_rounded, tr('麦克风'), tr('录制你身边的声音')),
              (
                'system',
                Icons.volume_up_rounded,
                tr('系统内部音频'),
                tr('捕获允许录制的媒体声音'),
              ),
              (
                'both',
                Icons.multitrack_audio_rounded,
                tr('麦克风 + 系统'),
                tr('同时录制，并自动混音'),
              ),
            ].map(
              (x) => Padding(
                padding: const EdgeInsets.only(bottom: 8),
                child: InkWell(
                  borderRadius: BorderRadius.circular(16),
                  onTap: active
                      ? null
                      : () => action(() => save({'source': x.$1})),
                  child: AnimatedContainer(
                    duration: const Duration(milliseconds: 180),
                    padding: const EdgeInsets.all(12),
                    decoration: BoxDecoration(
                      color: widget.settings['source'] == x.$1
                          ? Theme.of(context).colorScheme.primaryContainer
                          : Theme.of(context).colorScheme.surfaceContainerLow,
                      borderRadius: BorderRadius.circular(16),
                    ),
                    child: Row(
                      children: [
                        Icon(x.$2),
                        const SizedBox(width: 12),
                        Expanded(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text(
                                x.$3,
                                style: const TextStyle(
                                  fontWeight: FontWeight.w600,
                                ),
                              ),
                              Text(
                                x.$4,
                                style: Theme.of(context).textTheme.bodySmall,
                              ),
                            ],
                          ),
                        ),
                        if (widget.settings['source'] == x.$1)
                          const Icon(Icons.check_circle_rounded, size: 20),
                      ],
                    ),
                  ),
                ),
              ),
            ),
            const SizedBox(height: 8),
            SwitchListTile.adaptive(
              contentPadding: EdgeInsets.zero,
              title: Text(tr('悬浮字幕')),
              subtitle: Text(tr('记住开启状态；关闭浮窗不停止录音')),
              value: widget.settings['overlay'] == true,
              onChanged: (v) => action(() async {
                await PlatformDesk.call('overlay', {'enabled': v});
                await widget.reload();
                await WidgetsBinding.instance.endOfFrame;
                sheetRefresh?.call(() {});
              }),
            ),
            if (widget.settings['source'] != 'mic')
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text(
                  tr('内录需系统授权。通话及禁止音频捕获的应用可能无声。'),
                  style: const TextStyle(fontSize: 12, height: 1.6),
                ),
              ),
          ],
        ),
      ),
    ),
  );
  Widget toolbar() {
    final p = phase;
    final busy =
        actionBusy || ['connecting', 'pausing', 'stopping'].contains(p);
    return Wrap(
      spacing: 10,
      runSpacing: 10,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        if (!active)
          FilledButton.icon(
            onPressed: busy
                ? null
                : () => action(() async {
                    await PlatformDesk.call('start', {
                      'title': title.text.trim().isEmpty
                          ? tr('我的听译记录')
                          : title.text,
                    });
                  }),
            icon: const Icon(Icons.mic_rounded),
            label: Text(tr('开始录音')),
          ),
        if (p == 'recording')
          FilledButton.icon(
            onPressed: busy
                ? null
                : () => action(() async {
                    await PlatformDesk.call('pause');
                  }),
            icon: const Icon(Icons.pause_rounded),
            label: Text(tr('暂停录音')),
          ),
        if (p == 'paused')
          FilledButton.icon(
            onPressed: busy
                ? null
                : () => action(() async {
                    await PlatformDesk.call('resume');
                  }),
            icon: const Icon(Icons.play_arrow_rounded),
            label: Text(tr('继续录音')),
          ),
        if (active)
          OutlinedButton.icon(
            onPressed: actionBusy || p == 'stopping'
                ? null
                : () => action(() async {
                    await PlatformDesk.call('stop');
                  }),
            icon: const Icon(Icons.stop_rounded),
            label: Text(tr('停止并保存')),
          ),
        if (busy)
          const SizedBox(
            width: 24,
            height: 24,
            child: CircularProgressIndicator(strokeWidth: 2),
          ),
        if (!active)
          TextButton.icon(
            onPressed: () => setState(() => page = 2),
            icon: const Icon(Icons.key_rounded, size: 18),
            label: Text(
              widget.settings['hasKey'] == true
                  ? widget.settings['keyHint'] as String
                  : tr('连接设置'),
            ),
          ),
      ],
    );
  }

  Widget subtitles() => SessionPanels(
    key: const ValueKey('current-session'),
    record: record,
    settings: widget.settings,
    onModeChanged: (mode) => action(() => save({'captionMode': mode})),
    onOverlay: () => action(() async {
      await PlatformDesk.call('overlay', {'enabled': true});
      await widget.reload();
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text(tr('悬浮字幕已记住，返回桌面后自动显示'))));
      }
    }),
    onError: showError,
    saved: !active && (record['id'] as String? ?? '').isNotEmpty,
  );

  Widget capture() => LayoutBuilder(
    builder: (context, c) {
      final hinges = MediaQuery.of(context).displayFeatures
          .where((f) => f.bounds.width > 0 && f.bounds.height > f.bounds.width)
          .toList();
      final foldedLeft = hinges.isEmpty
          ? 0.0
          : hinges.first.bounds.left -
                MediaQuery.paddingOf(context).left -
                104 -
                8;
      final folded =
          hinges.isNotEmpty &&
          foldedLeft >= 220 &&
          c.maxWidth - foldedLeft - hinges.first.bounds.width - 16 >= 260;
      final side = c.maxWidth >= 850 || folded;
      final gap = hinges.isEmpty
          ? 20.0
          : math.max(20.0, hinges.first.bounds.width + 16);
      final captionPane = Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Wrap(
            crossAxisAlignment: WrapCrossAlignment.center,
            spacing: 12,
            runSpacing: 4,
            children: [
              Container(
                padding: const EdgeInsets.symmetric(
                  horizontal: 12,
                  vertical: 7,
                ),
                decoration: BoxDecoration(
                  color: Theme.of(context).colorScheme.primaryContainer,
                  borderRadius: BorderRadius.circular(30),
                ),
                child: Text(
                  phaseLabel(phase),
                  style: const TextStyle(fontWeight: FontWeight.w600),
                ),
              ),
              Text(
                duration(record['durationMs']),
                style: const TextStyle(
                  fontSize: 24,
                  fontWeight: FontWeight.w600,
                  fontFeatures: [FontFeature.tabularFigures()],
                ),
              ),
            ],
          ),
          const SizedBox(height: 12),
          toolbar(),
          const SizedBox(height: 14),
          if ((record['warning'] as String? ?? '').isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(bottom: 10),
              child: Text(
                tr(record['warning'] as String),
                style: TextStyle(
                  fontSize: 12,
                  color: Theme.of(context).colorScheme.tertiary,
                ),
              ),
            ),
          if (phase == 'recording')
            Padding(
              padding: const EdgeInsets.only(bottom: 12),
              child: LinearProgressIndicator(
                value: ((state['level'] as num?)?.toDouble() ?? 0).clamp(0, 1),
                minHeight: 5,
                borderRadius: BorderRadius.circular(5),
              ),
            ),
          Expanded(child: subtitles()),
        ],
      );
      if (side) {
        return Row(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            SizedBox(
              width: folded
                  ? foldedLeft
                  : math.min(320, (c.maxWidth - gap) * .36),
              child: controls(),
            ),
            SizedBox(width: gap),
            Expanded(child: captionPane),
          ],
        );
      }
      return Column(
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  tr('让声音，成为文字。'),
                  style: const TextStyle(
                    fontSize: 22,
                    fontWeight: FontWeight.w700,
                  ),
                ),
              ),
              IconButton(
                tooltip: tr('录音设置'),
                onPressed: () => showModalBottomSheet<void>(
                  context: context,
                  isScrollControlled: true,
                  useSafeArea: true,
                  builder: (_) => StatefulBuilder(
                    builder: (context, refresh) {
                      sheetRefresh = refresh;
                      return SizedBox(
                        height: MediaQuery.sizeOf(context).height * .8,
                        child: controls(),
                      );
                    },
                  ),
                ).whenComplete(() => sheetRefresh = null),
                icon: const Icon(Icons.tune_rounded),
              ),
            ],
          ),
          Padding(
            padding: const EdgeInsets.only(top: 6, bottom: 12),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    '${AppStrings.pair(widget.settings['pair']).map(AppStrings.name).join(' ⇄ ')} · ${{'mic': tr('麦克风'), 'system': tr('系统内部音频'), 'both': tr('麦克风 + 系统')}[widget.settings['source']]}',
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                ),
                if (widget.settings['overlay'] == true)
                  const Icon(Icons.picture_in_picture_alt_rounded, size: 18),
              ],
            ),
          ),
          Expanded(child: captionPane),
        ],
      );
    },
  );
  Widget historyPage() => Column(
    crossAxisAlignment: CrossAxisAlignment.start,
    children: [
      Row(
        children: [
          Expanded(
            child: Text(
              tr('本机记录'),
              style: const TextStyle(fontSize: 26, fontWeight: FontWeight.w700),
            ),
          ),
          IconButton(
            onPressed: refreshHistory,
            tooltip: tr('刷新'),
            icon: const Icon(Icons.refresh_rounded),
          ),
        ],
      ),
      const SizedBox(height: 8),
      Text(tr('录音与字幕保存在此设备，导出后可自行备份。')),
      const SizedBox(height: 20),
      Expanded(
        child: history.isEmpty
            ? Center(child: Text(tr('还没有录音记录')))
            : ListView.separated(
                itemCount: history.length,
                separatorBuilder: (_, _) => const SizedBox(height: 10),
                itemBuilder: (context, i) {
                  final r = Map<String, dynamic>.from(history[i] as Map);
                  final date = DateTime.fromMillisecondsSinceEpoch(
                    (r['createdAt'] as num).toInt(),
                  );
                  return Card(
                    margin: EdgeInsets.zero,
                    child: ListTile(
                      contentPadding: const EdgeInsets.symmetric(
                        horizontal: 20,
                        vertical: 8,
                      ),
                      leading: CircleAvatar(
                        backgroundColor: Theme.of(context)
                            .colorScheme
                            .primaryContainer,
                        child: const Icon(Icons.graphic_eq_rounded),
                      ),
                      title: Text(
                        r['title'] as String,
                        style: const TextStyle(fontWeight: FontWeight.w600),
                      ),
                      subtitle: Text(
                        '${date.month}/${date.day} ${date.hour.toString().padLeft(2, '0')}:${date.minute.toString().padLeft(2, '0')} · ${duration(r['durationMs'])} · ${(r['segments'] as List).length} ${tr('段字幕')}',
                      ),
                      onTap: () async {
                        final current = await Navigator.of(context).push<bool>(
                          MaterialPageRoute(
                            builder: (_) => HistoryDetail(
                              record: r,
                              settings: widget.settings,
                              live: state,
                            ),
                          ),
                        );
                        if (mounted && current == true) {
                          setState(() => page = 0);
                        }
                      },
                      trailing: IconButton(
                        tooltip: tr('删除记录'),
                        icon: const Icon(Icons.delete_outline_rounded),
                        onPressed: () => action(() async {
                          final yes = await showDialog<bool>(
                            context: context,
                            builder: (context) => AlertDialog(
                              title: Text(tr('删除这条记录？')),
                              content: Text(tr('本机录音与字幕会一并删除。')),
                              actions: [
                                TextButton(
                                  onPressed: () =>
                                      Navigator.pop(context, false),
                                  child: Text(tr('取消')),
                                ),
                                FilledButton(
                                  onPressed: () => Navigator.pop(context, true),
                                  child: Text(tr('删除')),
                                ),
                              ],
                            ),
                          );
                          if (yes == true) {
                            await PlatformDesk.call('core', {
                              'op': 'delete',
                              'id': r['id'],
                            });
                            await refreshHistory();
                          }
                        }),
                      ),
                    ),
                  );
                },
              ),
      ),
    ],
  );
  @override
  Widget build(BuildContext context) => Scaffold(
    body: SafeArea(
      child: LayoutBuilder(
        builder: (context, c) {
          final wide = c.maxWidth >= 720;
          final content = Padding(
            padding: EdgeInsets.all(wide ? 24 : 16),
            child: switch (page) {
              1 => historyPage(),
              2 => SettingsPage(
                settings: widget.settings,
                active: active,
                save: save,
                onError: showError,
              ),
              _ =>
                MediaQuery.of(context).displayFeatures.any(
                      (f) =>
                          f.bounds.height > 0 &&
                          f.bounds.width > f.bounds.height,
                    )
                    ? DisplayFeatureSubScreen(
                        anchorPoint: Offset(
                          MediaQuery.sizeOf(context).width / 2,
                          0,
                        ),
                        child: capture(),
                      )
                    : capture(),
            },
          );
          return Row(
            children: [
              if (wide)
                NavigationRail(
                  scrollable: true,
                  selectedIndex: page,
                  onDestinationSelected: (v) => setState(() => page = v),
                  labelType: NavigationRailLabelType.all,
                  leading: const Padding(
                    padding: EdgeInsets.symmetric(vertical: 24),
                    child: Icon(Icons.graphic_eq_rounded, size: 34),
                  ),
                  destinations: [
                    NavigationRailDestination(
                      icon: const Icon(Icons.subtitles_outlined),
                      selectedIcon: const Icon(Icons.subtitles_rounded),
                      label: Text(tr('听译')),
                    ),
                    NavigationRailDestination(
                      icon: const Icon(Icons.history_rounded),
                      label: Text(tr('记录')),
                    ),
                    NavigationRailDestination(
                      icon: const Icon(Icons.tune_rounded),
                      label: Text(tr('设置')),
                    ),
                  ],
                ),
              Expanded(child: content),
            ],
          );
        },
      ),
    ),
    bottomNavigationBar: MediaQuery.sizeOf(context).width < 720
        ? NavigationBar(
            selectedIndex: page,
            onDestinationSelected: (v) => setState(() => page = v),
            destinations: [
              NavigationDestination(
                icon: const Icon(Icons.subtitles_outlined),
                selectedIcon: const Icon(Icons.subtitles_rounded),
                label: tr('听译'),
              ),
              NavigationDestination(
                icon: const Icon(Icons.history_rounded),
                label: tr('记录'),
              ),
              NavigationDestination(
                icon: const Icon(Icons.tune_rounded),
                label: tr('设置'),
              ),
            ],
          )
        : null,
  );
}

class SettingsPage extends StatefulWidget {
  const SettingsPage({
    super.key,
    required this.settings,
    required this.active,
    required this.save,
    required this.onError,
  });
  final Map<String, dynamic> settings;
  final bool active;
  final Future<void> Function(Map<String, dynamic>) save;
  final void Function(Object) onError;
  @override
  State<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends State<SettingsPage> {
  final keyField = TextEditingController();
  bool hidden = true, busy = false;
  String result = '';
  List<dynamic> devices = [];
  @override
  void dispose() {
    keyField.dispose();
    super.dispose();
  }

  Future<void> task(Future<void> Function() work) async {
    if (busy) return;
    setState(() => busy = true);
    try {
      await work();
    } catch (e) {
      widget.onError(e);
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final s = widget.settings;
    return Center(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 720),
        child: ListView(
          children: [
            Text(
              tr('连接与显示'),
              style: const TextStyle(fontSize: 26, fontWeight: FontWeight.w700),
            ),
            const SizedBox(height: 8),
            Text(tr('使用你的 LecSync API Key。音频按服务商实际用量计费。')),
            const SizedBox(height: 24),
            Card(
              margin: EdgeInsets.zero,
              child: Padding(
                padding: const EdgeInsets.all(20),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      tr('LecSync 连接'),
                      style: const TextStyle(
                        fontSize: 18,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                    TextButton.icon(
                      onPressed: () => task(() async {
                        await PlatformDesk.call('openLecSyncConsole');
                      }),
                      icon: const Icon(Icons.open_in_new_rounded, size: 18),
                      label: Text(tr('LecSync 控制台')),
                    ),
                    const SizedBox(height: 16),
                    TextField(
                      controller: keyField,
                      enabled: !widget.active && !busy,
                      obscureText: hidden,
                      autocorrect: false,
                      enableSuggestions: false,
                      decoration: InputDecoration(
                        labelText: 'API Key',
                        hintText: s['hasKey'] == true
                            ? '${tr('已保存')} ${s['keyHint']} · ${tr('输入新密钥替换')}'
                            : tr('粘贴你的密钥'),
                        prefixIcon: const Icon(Icons.key_rounded),
                        suffixIcon: IconButton(
                          tooltip: hidden ? tr('显示密钥') : tr('隐藏密钥'),
                          onPressed: () => setState(() => hidden = !hidden),
                          icon: Icon(
                            hidden
                                ? Icons.visibility_outlined
                                : Icons.visibility_off_outlined,
                          ),
                        ),
                      ),
                    ),
                    const SizedBox(height: 14),
                    Wrap(
                      spacing: 8,
                      runSpacing: 8,
                      children: [
                        FilledButton(
                          onPressed: widget.active || busy
                              ? null
                              : () => task(() async {
                                  if (keyField.text.trim().isEmpty) {
                                    throw Exception(tr('请填入新密钥'));
                                  }
                                  await widget.save({'apiKey': keyField.text});
                                  keyField.clear();
                                  setState(() => result = tr('密钥已在手机本地加密保存'));
                                }),
                          child: Text(tr('保存密钥')),
                        ),
                        OutlinedButton(
                          onPressed: widget.active || busy
                              ? null
                              : () => task(() async {
                                  if (keyField.text.trim().isNotEmpty) {
                                    await widget.save({
                                      'apiKey': keyField.text,
                                    });
                                    keyField.clear();
                                  }
                                  final r = await PlatformDesk.call('core', {
                                    'op': 'test',
                                  });
                                  if (mounted) {
                                    setState(
                                      () => result = r['message'] as String,
                                    );
                                  }
                                }),
                          child: Text(tr('测试连接')),
                        ),
                        TextButton(
                          onPressed: widget.active || busy
                              ? null
                              : () => task(() async {
                                  await widget.save({'apiKey': ''});
                                  setState(() => result = tr('已删除本机密钥'));
                                }),
                          child: Text(tr('删除密钥')),
                        ),
                      ],
                    ),
                    if (busy)
                      const Padding(
                        padding: EdgeInsets.only(top: 12),
                        child: LinearProgressIndicator(),
                      ),
                    if (result.isNotEmpty)
                      Padding(
                        padding: const EdgeInsets.only(top: 12),
                        child: Text(tr(result)),
                      ),
                    const SizedBox(height: 14),
                    Text(
                      tr(
                        '密钥使用 Android Keystore 加密。本机录音不自动同步到网页账号；测试连接不采集或发送音频。',
                      ),
                      style: const TextStyle(fontSize: 12, height: 1.6),
                    ),
                    const Divider(height: 32),
                    for (final item in [
                      ('speakers', tr('区分说话人'), tr('标注不同说话人的字幕')),
                      ('digest', tr('实时纪要'), tr('整理话题与要点')),
                      ('enhance', tr('译文精修'), tr('根据上下文更新译文')),
                    ])
                      SwitchListTile.adaptive(
                        contentPadding: EdgeInsets.zero,
                        title: Text(item.$2),
                        subtitle: Text(item.$3),
                        value: s[item.$1] == true,
                        onChanged: widget.active || busy
                            ? null
                            : (v) => task(() => widget.save({item.$1: v})),
                      ),
                    TextButton.icon(
                      onPressed: () => task(() async {
                        final r = await PlatformDesk.call('devices');
                        if (mounted) setState(() => devices = r as List);
                      }),
                      icon: const Icon(Icons.headset_mic_rounded),
                      label: Text(tr('刷新输入设备')),
                    ),
                    if (devices.isNotEmpty)
                      DropdownButtonFormField<String>(
                        isExpanded: true,
                        initialValue: devices.any((d) => d['id'] == s['device'])
                            ? s['device'] as String
                            : 'default',
                        decoration: InputDecoration(labelText: tr('麦克风设备')),
                        items: [
                          DropdownMenuItem(
                            value: 'default',
                            child: Text(tr('系统默认麦克风')),
                          ),
                          ...devices.map(
                            (d) => DropdownMenuItem(
                              value: d['id'] as String,
                              child: Text(
                                d['name'] as String,
                                overflow: TextOverflow.ellipsis,
                              ),
                            ),
                          ),
                        ],
                        onChanged: widget.active
                            ? null
                            : (v) => task(() => widget.save({'device': v})),
                      ),
                  ],
                ),
              ),
            ),
            const SizedBox(height: 16),
            Card(
              margin: EdgeInsets.zero,
              child: Padding(
                padding: const EdgeInsets.all(20),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      tr('阅读体验'),
                      style: const TextStyle(
                        fontSize: 18,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                    const SizedBox(height: 16),
                    LanguageField(
                      value: s['uiLanguage'] as String? ?? 'system',
                      label: tr('界面语言'),
                      followSystem: true,
                      onChanged: (v) =>
                          task(() => widget.save({'uiLanguage': v})),
                    ),
                    if (s['uiLanguage'] != null &&
                        s['uiLanguage'] != 'system' &&
                        s['effectiveLocale'] == 'en' &&
                        s['uiLanguage'] != 'en')
                      Padding(
                        padding: const EdgeInsets.only(top: 8),
                        child: Text(
                          tr('系统不支持此界面语言，将显示英语。'),
                          style: Theme.of(context).textTheme.bodySmall,
                        ),
                      ),
                    const SizedBox(height: 16),
                    DropdownButtonFormField<String>(
                      isExpanded: true,
                      initialValue: s['theme'] as String,
                      decoration: InputDecoration(labelText: tr('外观')),
                      items: [
                        DropdownMenuItem(
                          value: 'system',
                          child: Text(tr('跟随系统')),
                        ),
                        DropdownMenuItem(value: 'light', child: Text(tr('浅色'))),
                        DropdownMenuItem(value: 'dark', child: Text(tr('深色'))),
                      ],
                      onChanged: (v) => task(() => widget.save({'theme': v})),
                    ),
                    const SizedBox(height: 16),
                    Text('${tr('字幕字号')} · ${s['fontSize']}'),
                    Slider(
                      value: (s['fontSize'] as num).toDouble(),
                      min: 14,
                      max: 32,
                      divisions: 9,
                      onChanged: (v) =>
                          task(() => widget.save({'fontSize': v.round()})),
                    ),
                    Text(
                      tr('接下来说明这部分内容。'),
                      style: TextStyle(
                        fontSize: (s['fontSize'] as num).toDouble(),
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                    const SizedBox(height: 14),
                    Text(
                      tr('主界面与悬浮窗各自控制跟随。向上翻阅后，点击“回到底部并跟随”恢复更新。'),
                      style: const TextStyle(fontSize: 12, height: 1.6),
                    ),
                  ],
                ),
              ),
            ),
            const SizedBox(height: 24),
            const Text(
              'LingoDesk 1.2.2 · Android 10–17 · ARM64',
              style: TextStyle(fontSize: 12, height: 1.8),
            ),
          ],
        ),
      ),
    );
  }
}

class CaptionFeed extends StatefulWidget {
  const CaptionFeed({
    super.key,
    required this.record,
    required this.settings,
    this.floating = false,
  });
  final Map<String, dynamic> record, settings;
  final bool floating;
  @override
  State<CaptionFeed> createState() => _CaptionFeedState();
}

class _CaptionFeedState extends State<CaptionFeed> {
  final scroll = ScrollController();
  bool follow = true;
  bool actionsOpen = false;
  Map<String, dynamic>? editedRecord;

  Future<void> textActions(
    Map row,
    String rowType,
    String field,
    String text,
  ) async {
    if (actionsOpen) return;
    actionsOpen = true;
    final recordId = (editedRecord ?? widget.record)['id'] as String? ?? '';
    final rowId = row['id'] as String? ?? '';
    final canEdit = recordId.isNotEmpty && rowId.isNotEmpty;
    try {
      if (widget.floating) {
        await PlatformDesk.call('overlayTextInput', {'enabled': true});
      }
      if (!mounted) return;
      final action = await showDialog<String>(
        context: context,
        useSafeArea: !widget.floating,
        builder: (context) => Dialog(
          constraints: const BoxConstraints(minWidth: 0, maxWidth: 360),
          insetPadding: EdgeInsets.all(widget.floating ? 4 : 24),
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(4),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                TextButton.icon(
                  onPressed: () => Navigator.pop(context, 'copy'),
                  icon: const Icon(Icons.copy_rounded, size: 18),
                  label: Text(tr('复制')),
                ),
                if (canEdit)
                  TextButton.icon(
                    onPressed: () => Navigator.pop(context, 'edit'),
                    icon: const Icon(Icons.edit_outlined, size: 18),
                    label: Text(tr('编辑')),
                  ),
              ],
            ),
          ),
        ),
      );
      if (action == 'copy') {
        await Clipboard.setData(ClipboardData(text: text));
      } else if (action == 'edit' && mounted) {
        await showDialog<void>(
          context: context,
          useSafeArea: !widget.floating,
          barrierDismissible: false,
          builder: (context) => _CaptionEditor(
            title: tr(field == 'text' ? '编辑原文' : '编辑译文'),
            text: text,
            floating: widget.floating,
            save: (value) async {
              final updated = await PlatformDesk.call('core', {
                'op': 'editCaption',
                'id': recordId,
                'rowId': rowId,
                'rowType': rowType,
                'field': field,
                'value': value,
              });
              if (mounted) {
                setState(
                  () =>
                      editedRecord = Map<String, dynamic>.from(updated as Map),
                );
              }
            },
          ),
        );
      }
    } catch (error) {
      if (mounted) {
        await showDialog<void>(
          context: context,
          useSafeArea: !widget.floating,
          builder: (context) => Dialog(
            constraints: const BoxConstraints(minWidth: 0, maxWidth: 360),
            insetPadding: EdgeInsets.all(widget.floating ? 4 : 24),
            child: SingleChildScrollView(
              padding: const EdgeInsets.all(8),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text(_captionError(error)),
                  TextButton(
                    onPressed: () => Navigator.pop(context),
                    child: Text(tr('取消')),
                  ),
                ],
              ),
            ),
          ),
        );
      }
    } finally {
      if (widget.floating) {
        await PlatformDesk.call('overlayTextInput', {
          'enabled': false,
        }).catchError((_) => null);
      }
      actionsOpen = false;
    }
  }

  void bottom() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && follow && !actionsOpen && scroll.hasClients) {
        scroll.jumpTo(scroll.position.maxScrollExtent);
      }
    });
  }

  @override
  void didUpdateWidget(CaptionFeed old) {
    super.didUpdateWidget(old);
    if (!identical(old.record, widget.record)) editedRecord = null;
    if (follow) bottom();
  }

  @override
  void dispose() {
    scroll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final record = editedRecord ?? widget.record;
    final captions = record['captions'] as List?;
    final list = captions?.isNotEmpty == true
        ? captions!
        : record['segments'] as List? ?? [];
    final displayRows = captions?.isNotEmpty == true;
    final preview = displayRows ? '' : record['preview'] as String? ?? '';
    final mode = widget.settings['captionMode'];
    final requestedFont = (widget.settings['fontSize'] as num? ?? 20)
        .toDouble();
    final size = MediaQuery.sizeOf(context);
    final font = widget.floating
        ? (requestedFont *
                  (size.width / 380).clamp(.6, 1) *
                  (size.height / 260).clamp(.8, 1))
              .clamp(12, 32)
              .toDouble()
        : requestedFont;
    if (list.isEmpty && preview.isEmpty) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: FittedBox(
            fit: BoxFit.scaleDown,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(
                  Icons.graphic_eq_rounded,
                  size: widget.floating ? 36 : 64,
                  color: Theme.of(context).colorScheme.primary,
                ),
                const SizedBox(height: 16),
                Text(
                  record['phase'] == 'recording'
                      ? tr('正在聆听…')
                      : tr('听到的内容，在这里出现'),
                  textAlign: TextAlign.center,
                  style: TextStyle(
                    fontSize: widget.floating ? 16 : 20,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                if (!widget.floating)
                  Padding(
                    padding: const EdgeInsets.only(top: 10),
                    child: Text(
                      tr('填写 API Key，选择音源，然后开始录音。'),
                      textAlign: TextAlign.center,
                      style: const TextStyle(height: 1.6),
                    ),
                  ),
              ],
            ),
          ),
        ),
      );
    }
    return Column(
      children: [
        Expanded(
          child: NotificationListener<ScrollNotification>(
            onNotification: (n) {
              if (n is ScrollUpdateNotification &&
                  n.dragDetails != null &&
                  follow) {
                setState(() => follow = false);
              }
              if (n is UserScrollNotification &&
                  n.direction != ScrollDirection.idle &&
                  follow) {
                setState(() => follow = false);
              }
              if (widget.floating &&
                  n is ScrollEndNotification &&
                  scroll.hasClients &&
                  scroll.position.extentAfter < 24 &&
                  !follow) {
                setState(() => follow = true);
                bottom();
              }
              return false;
            },
            child: ListView.builder(
              controller: scroll,
              padding: EdgeInsets.all(widget.floating ? 14 : 20),
              itemCount: list.length + (preview.isNotEmpty ? 1 : 0),
              itemBuilder: (context, i) {
                final partial =
                    i == list.length ||
                    (displayRows && (list[i] as Map)['provisional'] == true);
                final s = i == list.length
                    ? {
                        'text': preview,
                        'translation': record['previewTranslation'],
                        'speaker': '',
                        'startMs': record['durationMs'],
                      }
                    : list[i] as Map;
                return Padding(
                  key: ValueKey(s['id'] ?? 'preview'),
                  padding: EdgeInsets.only(bottom: widget.floating ? 10 : 20),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        partial
                            ? tr('识别中…')
                            : '${duration(s['startMs'])}${(s['speaker'] as String? ?? '').isNotEmpty ? ' · ${tr('说话人')} ${s['speaker']}' : ''}${s['enhanced'] == true ? ' · ${tr('已精修')}' : ''}',
                        style: TextStyle(
                          fontSize: 11,
                          color: Theme.of(context).colorScheme.outline,
                        ),
                      ),
                      const SizedBox(height: 7),
                      if (mode != 'translation' &&
                          (s['text'] as String? ?? '').isNotEmpty)
                        SelectableText(
                          s['text'] as String,
                          onTap: () => textActions(
                            s,
                            displayRows ? 'caption' : 'segment',
                            'text',
                            s['text'] as String,
                          ),
                          textDirection: contentDirection(s['text'] as String),
                          style: TextStyle(
                            fontSize: font - 2,
                            height: 1.6,
                            color: Theme.of(context)
                                .colorScheme
                                .onSurfaceVariant,
                          ),
                        ),
                      if (mode != 'source' &&
                          (s['translation'] as String? ?? '').isNotEmpty)
                        SelectableText(
                          s['translation'] as String,
                          onTap: () => textActions(
                            s,
                            displayRows ? 'caption' : 'segment',
                            'translation',
                            s['translation'] as String,
                          ),
                          textDirection: contentDirection(
                            s['translation'] as String,
                          ),
                          style: TextStyle(
                            fontSize: font,
                            height: 1.6,
                            fontWeight: FontWeight.w600,
                            color: Theme.of(context).colorScheme.primary,
                          ),
                        ),
                    ],
                  ),
                );
              },
            ),
          ),
        ),
        if (!follow && !widget.floating)
          TextButton.icon(
            onPressed: () {
              setState(() => follow = true);
              bottom();
            },
            icon: const Icon(Icons.south_rounded, size: 16),
            label: Text(tr('回到底部并跟随')),
          ),
      ],
    );
  }
}

String _captionError(Object error) => tr(
  error is PlatformException
      ? (error.message ?? '操作失败')
      : error.toString().replaceFirst('Exception: ', ''),
);

class _CaptionEditor extends StatefulWidget {
  const _CaptionEditor({
    required this.title,
    required this.text,
    required this.floating,
    required this.save,
  });
  final String title, text;
  final bool floating;
  final Future<void> Function(String) save;
  @override
  State<_CaptionEditor> createState() => _CaptionEditorState();
}

class _CaptionEditorState extends State<_CaptionEditor> {
  late final controller = TextEditingController(text: widget.text);
  bool saving = false;
  String? error;

  Future<void> save() async {
    if (saving) return;
    setState(() {
      saving = true;
      error = null;
    });
    try {
      await widget.save(controller.text);
      if (mounted) Navigator.pop(context);
    } catch (e) {
      if (mounted) setState(() => error = _captionError(e));
    } finally {
      if (mounted) setState(() => saving = false);
    }
  }

  @override
  void dispose() {
    controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => PopScope(
    canPop: !saving,
    child: Dialog(
      constraints: const BoxConstraints(minWidth: 0, maxWidth: 560),
      insetPadding: EdgeInsets.all(widget.floating ? 4 : 24),
      child: Padding(
        padding: EdgeInsets.all(widget.floating ? 4 : 16),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Flexible(
              child: SingleChildScrollView(
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    TextField(
                      controller: controller,
                      autofocus: true,
                      readOnly: saving,
                      minLines: 1,
                      maxLines: widget.floating ? 3 : 8,
                      decoration: InputDecoration(labelText: widget.title),
                    ),
                    if (error != null)
                      Text(
                        error!,
                        style: TextStyle(
                          color: Theme.of(context).colorScheme.error,
                        ),
                      ),
                  ],
                ),
              ),
            ),
            Row(
              children: [
                Expanded(
                  child: TextButton(
                    onPressed: saving ? null : () => Navigator.pop(context),
                    child: Text(
                      tr('取消'),
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                ),
                Expanded(
                  child: FilledButton(
                    onPressed: saving ? null : save,
                    child: saving
                        ? const SizedBox(
                            width: 16,
                            height: 16,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : Text(
                            tr('保存'),
                            maxLines: 2,
                            overflow: TextOverflow.ellipsis,
                          ),
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    ),
  );
}

class FloatingDesk extends StatefulWidget {
  const FloatingDesk({super.key, required this.settings});
  static const controlsChannel = MethodChannel('desk/overlay');
  final Map<String, dynamic> settings;
  @override
  State<FloatingDesk> createState() => _FloatingDeskState();
}

class _FloatingDeskState extends State<FloatingDesk>
    with WidgetsBindingObserver {
  Map<String, dynamic> record = {'phase': 'idle', 'segments': []};
  StreamSubscription<dynamic>? sub;
  Timer? hideControls;
  bool controlsVisible = true;
  bool foreground = true;
  bool nativeTouch = false;
  final pointers = <int>{};

  void syncControls() {
    unawaited(
      PlatformDesk.call('overlayControls', {
        'visible': controlsVisible,
      }).catchError((_) => null),
    );
  }

  void showControls() {
    if (!mounted || !foreground) return;
    hideControls?.cancel();
    if (!controlsVisible) {
      setState(() => controlsVisible = true);
      syncControls();
    }
    if (!nativeTouch && pointers.isEmpty) {
      hideControls = Timer(const Duration(seconds: 3), () {
        if (!mounted || !foreground) return;
        setState(() => controlsVisible = false);
        syncControls();
      });
    }
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    foreground = state == AppLifecycleState.resumed;
    hideControls?.cancel();
    nativeTouch = false;
    pointers.clear();
    if (foreground) showControls();
  }

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    FloatingDesk.controlsChannel.setMethodCallHandler((call) async {
      if (call.method == 'shown') {
        foreground = true;
        nativeTouch = false;
        pointers.clear();
        showControls();
      } else if (call.method == 'interaction') {
        nativeTouch = call.arguments == true;
        showControls();
      }
    });
    syncControls();
    showControls();
    sub = PlatformDesk.snapshots.listen((v) {
      if (mounted) {
        setState(
          () => record = Map<String, dynamic>.from(
            jsonDecode(v as String) as Map,
          ),
        );
      }
    });
  }

  @override
  void dispose() {
    hideControls?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    FloatingDesk.controlsChannel.setMethodCallHandler(null);
    sub?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final phase = record['phase'] as String? ?? 'idle';
    return Directionality(
      textDirection: TextDirection.ltr,
      child: Listener(
        behavior: HitTestBehavior.translucent,
        onPointerDown: (event) {
          pointers.add(event.pointer);
          showControls();
        },
        onPointerUp: (event) {
          pointers.remove(event.pointer);
          showControls();
        },
        onPointerCancel: (event) {
          pointers.remove(event.pointer);
          showControls();
        },
        child: Material(
          color: Colors.transparent,
          child: Container(
            decoration: BoxDecoration(
              color: Theme.of(context).colorScheme.surface,
              borderRadius: BorderRadius.circular(22),
              border: Border.all(
                color: Theme.of(context).colorScheme.outlineVariant,
              ),
            ),
            clipBehavior: Clip.antiAlias,
            child: Column(
              children: [
                ClipRect(
                  child: AnimatedSize(
                    duration: const Duration(milliseconds: 180),
                    alignment: Alignment.topCenter,
                    child: controlsVisible
                        ? Column(
                            key: const ValueKey('floating-controls'),
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              Padding(
                                padding: const EdgeInsets.only(
                                  left: 12,
                                  right: 2,
                                  top: 2,
                                  bottom: 2,
                                ),
                                child: Row(
                                  children: [
                                    const Icon(
                                      Icons.drag_indicator_rounded,
                                      size: 16,
                                    ),
                                    const SizedBox(width: 6),
                                    Expanded(
                                      child: Text(
                                        '${tr('听译台')} · ${phaseLabel(phase)}',
                                        maxLines: 1,
                                        overflow: TextOverflow.ellipsis,
                                        style: const TextStyle(
                                          fontSize: 11,
                                          fontWeight: FontWeight.w700,
                                        ),
                                      ),
                                    ),
                                    IconButton(
                                      visualDensity: VisualDensity.compact,
                                      tooltip: tr('回到应用'),
                                      onPressed: () =>
                                          PlatformDesk.call('open'),
                                      icon: const Icon(
                                        Icons.open_in_new_rounded,
                                        size: 17,
                                      ),
                                    ),
                                    IconButton(
                                      visualDensity: VisualDensity.compact,
                                      tooltip: tr('关闭悬浮字幕'),
                                      onPressed: () =>
                                          PlatformDesk.call('closeOverlay'),
                                      icon: const Icon(
                                        Icons.close_rounded,
                                        size: 18,
                                      ),
                                    ),
                                  ],
                                ),
                              ),
                              const Divider(height: 1),
                            ],
                          )
                        : const SizedBox(width: double.infinity, height: 0),
                  ),
                ),
                Expanded(
                  child: CaptionFeed(
                    record: record,
                    settings: widget.settings,
                    floating: true,
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.only(left: 12, right: 5, bottom: 3),
                  child: Row(
                    children: [
                      Text(
                        duration(record['durationMs']),
                        style: const TextStyle(fontSize: 10),
                      ),
                      const Spacer(),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
