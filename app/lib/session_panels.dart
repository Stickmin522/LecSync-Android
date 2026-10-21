part of 'main.dart';

String digestText(dynamic value) {
  if (value is String) return value;
  if (value is Map) return digestText(value['text'] ?? value['content']);
  return '';
}

List<Map<String, dynamic>> readableSections(dynamic value) {
  if (value is! List) return [];
  return value
      .whereType<Map>()
      .map(
        (s) => {
          ...Map<String, dynamic>.from(s),
          'title': digestText(s['title']),
          'points': (s['points'] as List? ?? [])
              .map(digestText)
              .where((s) => s.trim().isNotEmpty)
              .toList(),
        },
      )
      .toList();
}

class SessionPanels extends StatefulWidget {
  const SessionPanels({
    super.key,
    required this.record,
    required this.settings,
    required this.onError,
    this.onModeChanged,
    this.onOverlay,
    this.saved = false,
  });
  final Map<String, dynamic> record, settings;
  final void Function(Object) onError;
  final void Function(String)? onModeChanged;
  final VoidCallback? onOverlay;
  final bool saved;
  @override
  State<SessionPanels> createState() => _SessionPanelsState();
}

class _SessionPanelsState extends State<SessionPanels> {
  int pane = 0;
  bool busy = false;
  Future<void> run(Future<void> Function() f) async {
    if (busy) return;
    setState(() => busy = true);
    try {
      await f();
    } catch (e) {
      widget.onError(e);
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  Widget recordingActions(bool compact) {
    final buttons = <Widget>[
      TextButton.icon(
        onPressed: busy
            ? null
            : () => run(() async {
                await PlatformDesk.call('play', {'id': widget.record['id']});
              }),
        icon: const Icon(Icons.play_circle_outline_rounded),
        label: Text(tr('回放')),
      ),
      TextButton(
        onPressed: () => run(() async {
          await PlatformDesk.call('stopPlayback');
        }),
        child: Text(tr('停止回放')),
      ),
      ...['txt', 'srt', 'json', 'wav'].map(
        (f) => TextButton(
          onPressed: busy
              ? null
              : () => run(() async {
                  await PlatformDesk.call('export', {
                    'id': widget.record['id'],
                    'format': f,
                  });
                }),
          child: Text(f == 'wav' ? tr('录音 WAV') : f.toUpperCase()),
        ),
      ),
    ];
    return compact
        ? SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: Row(children: buttons),
          )
        : Wrap(children: buttons);
  }

  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, constraints) => Card(
      margin: EdgeInsets.zero,
      child: Column(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 12, 6, 10),
            child: Row(
              children: [
                Expanded(
                  child: SegmentedButton<int>(
                    showSelectedIcon: false,
                    style: const ButtonStyle(
                      visualDensity: VisualDensity.compact,
                      padding: WidgetStatePropertyAll(
                        EdgeInsets.symmetric(horizontal: 8),
                      ),
                      textStyle: WidgetStatePropertyAll(
                        TextStyle(fontSize: 12),
                      ),
                    ),
                    segments: [
                      ButtonSegment(value: 0, label: Text(tr('双语字幕'))),
                      ButtonSegment(value: 1, label: Text(tr('实时纪要'))),
                    ],
                    selected: {pane},
                    onSelectionChanged: (v) => setState(() => pane = v.first),
                  ),
                ),
                if (widget.onOverlay != null)
                  IconButton(
                    tooltip: tr('悬浮字幕'),
                    onPressed: widget.onOverlay,
                    visualDensity: VisualDensity.compact,
                    icon: const Icon(
                      Icons.picture_in_picture_alt_rounded,
                      size: 20,
                    ),
                  ),
                if (widget.onModeChanged != null)
                  PopupMenuButton<String>(
                    tooltip: tr('字幕显示'),
                    onSelected: widget.onModeChanged,
                    itemBuilder: (_) => [
                      PopupMenuItem(value: 'both', child: Text(tr('原文与译文'))),
                      PopupMenuItem(value: 'source', child: Text(tr('仅原文'))),
                      PopupMenuItem(
                        value: 'translation',
                        child: Text(tr('仅译文')),
                      ),
                    ],
                  ),
              ],
            ),
          ),
          const Divider(height: 1),
          Expanded(
            child: IndexedStack(
              index: pane,
              children: [
                CaptionFeed(
                  key: ValueKey('captions-${widget.record['id']}'),
                  record: widget.record,
                  settings: widget.settings,
                ),
                DigestFeed(
                  key: ValueKey('digest-${widget.record['id']}'),
                  record: widget.record,
                  settings: widget.settings,
                  history: widget.saved,
                ),
              ],
            ),
          ),
          if (widget.saved)
            Padding(
              padding: const EdgeInsets.fromLTRB(8, 4, 8, 10),
              child: recordingActions(constraints.maxHeight < 260),
            ),
        ],
      ),
    ),
  );
}

class DigestFeed extends StatefulWidget {
  const DigestFeed({
    super.key,
    required this.record,
    required this.settings,
    this.history = false,
  });
  final Map<String, dynamic> record, settings;
  final bool history;
  @override
  State<DigestFeed> createState() => _DigestFeedState();
}

class _DigestFeedState extends State<DigestFeed> {
  Map<String, dynamic>? resolved;
  Timer? poll;
  bool querying = false;
  Future<void> refresh({bool retry = false}) async {
    final id = widget.record['id'] as String? ?? '';
    if (id.isEmpty || querying || !mounted) return;
    querying = true;
    try {
      final result = await PlatformDesk.call(retry ? 'digestRetry' : 'digest', {
        'id': id,
      });
      if (mounted && result is Map && result['id'] == id) {
        setState(() => resolved = Map<String, dynamic>.from(result));
      }
    } catch (_) {
    } finally {
      querying = false;
    }
  }

  @override
  void initState() {
    super.initState();
    refresh();
    if (widget.history && (widget.record['id'] as String? ?? '').isNotEmpty) {
      poll = Timer.periodic(const Duration(seconds: 2), (_) {
        if (resolved?['digestStatus'] == 'ready') {
          poll?.cancel();
        } else {
          refresh();
        }
      });
    }
  }

  @override
  void didUpdateWidget(DigestFeed old) {
    super.didUpdateWidget(old);
    if (!widget.history) resolved = null;
  }

  @override
  void dispose() {
    poll?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final record = resolved ?? widget.record;
    final original = readableSections(record['sections']);
    final chinese = readableSections(record['chineseSections']);
    final sections = chinese.isNotEmpty ? chinese : original;
    final status = record['digestStatus'] as String? ?? '';
    final error = record['digestError'] as String? ?? '';
    if (sections.isEmpty) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Text(
            tr('实时纪要将在这里整理\n字幕继续在“双语字幕”页更新'),
            textAlign: TextAlign.center,
            style: const TextStyle(height: 1.8),
          ),
        ),
      );
    }
    return ListView(
      padding: const EdgeInsets.all(20),
      children: [
        if (status == 'preparing' || status == 'translating')
          Padding(
            padding: const EdgeInsets.only(bottom: 16),
            child: Text(
              status == 'preparing'
                  ? tr('正在准备中文纪要语言包，录音继续进行…')
                  : tr('正在更新中文纪要…'),
              style: Theme.of(context).textTheme.bodySmall,
            ),
          ),
        if (error.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(bottom: 12),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(tr(error), style: Theme.of(context).textTheme.bodySmall),
                TextButton(
                  onPressed: () => refresh(retry: true),
                  child: Text(tr('准备并重试中文纪要')),
                ),
              ],
            ),
          ),
        ...sections.map(
          (s) => Padding(
            padding: const EdgeInsets.only(bottom: 24),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                if (s['startMs'] != null)
                  Text(
                    duration(s['startMs']),
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                const SizedBox(height: 8),
                SelectableText(
                  s['title'] as String,
                  textDirection: contentDirection(s['title'] as String),
                  style: TextStyle(
                    fontSize: (widget.settings['fontSize'] as num? ?? 20)
                        .toDouble(),
                    fontWeight: FontWeight.w700,
                    height: 1.6,
                  ),
                ),
                const SizedBox(height: 10),
                ...(s['points'] as List<String>).map(
                  (p) => Padding(
                    padding: const EdgeInsets.only(bottom: 10),
                    child: Row(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        const Text('•  ', style: TextStyle(height: 1.8)),
                        Expanded(
                          child: SelectableText(
                            p,
                            textDirection: contentDirection(p),
                            style: const TextStyle(height: 1.8),
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
        if (record['digestTranslated'] == true)
          Text(
            tr('中文翻译：Google Translate · 本机处理'),
            style: Theme.of(context).textTheme.bodySmall,
          ),
      ],
    );
  }
}

Future<Map<String, dynamic>?> editRecordingName(
  BuildContext context,
  Map<String, dynamic> record,
) => showDialog<Map<String, dynamic>>(
  context: context,
  barrierDismissible: false,
  builder: (_) => _RecordingNameEditor(record: record),
);

class _RecordingNameEditor extends StatefulWidget {
  const _RecordingNameEditor({required this.record});
  final Map<String, dynamic> record;
  @override
  State<_RecordingNameEditor> createState() => _RecordingNameEditorState();
}

class _RecordingNameEditorState extends State<_RecordingNameEditor> {
  late final name = TextEditingController(
    text: widget.record['title'] as String? ?? '',
  );
  bool saving = false;
  String? error;

  Future<void> save() async {
    if (saving) return;
    final value = name.text.trim();
    if (value.isEmpty) {
      setState(() => error = tr('记录名称不能为空'));
      return;
    }
    setState(() {
      saving = true;
      error = null;
    });
    try {
      final updated = await PlatformDesk.call('core', {
        'op': 'rename',
        'id': widget.record['id'],
        'title': value,
      });
      if (mounted) {
        Navigator.pop(context, Map<String, dynamic>.from(updated as Map));
      }
    } catch (e) {
      if (mounted) setState(() => error = _captionError(e));
    } finally {
      if (mounted) setState(() => saving = false);
    }
  }

  @override
  void dispose() {
    name.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => PopScope(
    canPop: !saving,
    child: AlertDialog(
      scrollable: true,
      title: Text(tr('重命名')),
      content: TextField(
        controller: name,
        autofocus: true,
        readOnly: saving,
        textDirection: contentDirection(name.text),
        textInputAction: TextInputAction.done,
        onSubmitted: (_) => save(),
        onChanged: (_) => setState(() => error = null),
        decoration: InputDecoration(
          labelText: tr('记录名称'),
          errorText: error,
          errorMaxLines: 4,
        ),
      ),
      actions: [
        TextButton(
          onPressed: saving ? null : () => Navigator.pop(context),
          child: Text(tr('取消')),
        ),
        FilledButton(
          onPressed: saving ? null : save,
          child: saving
              ? const SizedBox(
                  width: 16,
                  height: 16,
                  child: CircularProgressIndicator(strokeWidth: 2),
                )
              : Text(tr('保存')),
        ),
      ],
    ),
  );
}

class HistoryDetail extends StatefulWidget {
  const HistoryDetail({
    super.key,
    required this.record,
    required this.settings,
    required this.live,
  });
  final Map<String, dynamic> record, settings, live;
  @override
  State<HistoryDetail> createState() => _HistoryDetailState();
}

class _HistoryDetailState extends State<HistoryDetail> {
  late Map<String, dynamic> live = widget.live;
  late Map<String, dynamic> record = widget.record;
  StreamSubscription<dynamic>? sub;
  @override
  void initState() {
    super.initState();
    sub = PlatformDesk.snapshots.listen((e) {
      if (mounted) {
        setState(() {
          live = Map<String, dynamic>.from(jsonDecode(e as String) as Map);
          if (live['id'] == record['id']) record = live;
        });
      }
    });
  }

  @override
  void dispose() {
    sub?.cancel();
    PlatformDesk.call('stopPlayback');
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final active = !['idle', 'ended'].contains(live['phase']);
    return Scaffold(
      appBar: AppBar(
        title: Text(
          record['title'] as String? ?? tr('历史录音'),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
        ),
        actions: [
          IconButton(
            tooltip: tr('重命名'),
            icon: const Icon(Icons.edit_outlined),
            onPressed: () async {
              final updated = await editRecordingName(context, record);
              if (mounted && updated != null) {
                setState(() => record = {...record, 'title': updated['title']});
              }
            },
          ),
        ],
      ),
      body: DisplayFeatureSubScreen(
        anchorPoint: Offset(MediaQuery.sizeOf(context).width / 4, 0),
        child: SafeArea(
          child: Padding(
            padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
            child: Column(
              children: [
                if (active)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 12),
                    child: Container(
                      decoration: BoxDecoration(
                        color: Theme.of(context).colorScheme.primaryContainer,
                        borderRadius: BorderRadius.circular(12),
                      ),
                      padding: const EdgeInsets.symmetric(
                        horizontal: 12,
                        vertical: 4,
                      ),
                      child: Row(
                        children: [
                          Expanded(
                            child: Text(
                              '${live['phase'] == 'paused' ? tr('当前录音已暂停') : tr('后台录音与翻译继续')} · ${duration(live['durationMs'])}',
                              style: const TextStyle(fontSize: 12),
                            ),
                          ),
                          TextButton(
                            onPressed: () => Navigator.pop(context, true),
                            child: Text(tr('返回当前录音')),
                          ),
                        ],
                      ),
                    ),
                  ),
                Padding(
                  padding: const EdgeInsets.only(bottom: 12),
                  child: Align(
                    alignment: AlignmentDirectional.centerStart,
                    child: Text(
                      '${tr('历史录音')} · ${duration(record['durationMs'])}',
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                  ),
                ),
                Expanded(
                  child: SessionPanels(
                    record: record,
                    settings: widget.settings,
                    saved: true,
                    onError: (e) {
                      if (mounted) {
                        ScaffoldMessenger.of(context)
                            .showSnackBar(SnackBar(content: Text('$e')));
                      }
                    },
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
