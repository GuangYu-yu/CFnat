import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter/foundation.dart';
import 'package:provider/provider.dart';
import 'package:file_picker/file_picker.dart';
import '../services/app_service.dart';
import '../theme.dart';

class ConfigPanel extends StatefulWidget {
  final AppService service;
  final bool compact;
  
  const ConfigPanel({super.key, required this.service, this.compact = false});

  @override
  State<ConfigPanel> createState() => _ConfigPanelState();
}

class _ConfigPanelState extends State<ConfigPanel> {
  final _speedTestFileController = TextEditingController();
  final _manualInputController = TextEditingController();
  final _dataCenterController = TextEditingController();
  final _delayLimitController = TextEditingController();
  final _tlrController = TextEditingController();
  final _maxConcurrencyController = TextEditingController();
  final _loadCountController = TextEditingController();
  final _testAddressController = TextEditingController();
  final _localServiceController = TextEditingController();
  final _tlsPortController = TextEditingController();
  final _httpPortController = TextEditingController();
  final _maxStickySlotsController = TextEditingController();
  
  bool _isRunning = false;
  bool _connected = false;
  bool _actionInProgress = false;
  bool _showManualInput = false;
  
  List<String>? _ipContent;
  List<String> _manualIps = [];
  int get _manualIpCount => _manualIps.length;
  ConfigData? _lastConfig;

  @override
  void initState() {
    super.initState();
    _initFromConfig();
  }

  @override
  void dispose() {
    _speedTestFileController.dispose();
    _manualInputController.dispose();
    _dataCenterController.dispose();
    _delayLimitController.dispose();
    _tlrController.dispose();
    _maxConcurrencyController.dispose();
    _loadCountController.dispose();
    _testAddressController.dispose();
    _localServiceController.dispose();
    _tlsPortController.dispose();
    _httpPortController.dispose();
    _maxStickySlotsController.dispose();
    super.dispose();
  }

  void _initFromConfig() {
    final config = widget.service.config;
    if (config != null && config != _lastConfig) {
      _speedTestFileController.text = config.ipFile;
      _dataCenterController.text = config.colo?.join(',') ?? '';
      _delayLimitController.text = config.delayLimit.toString();
      _tlrController.text = config.tlr.toString();
      _maxConcurrencyController.text = config.threads.toString();
      _loadCountController.text = config.ips.toString();
      _testAddressController.text = config.http;
      _localServiceController.text = config.addr;
      _tlsPortController.text = config.tlsPort.toString();
      _httpPortController.text = config.httpPort.toString();
      _maxStickySlotsController.text = config.maxStickySlots.toString();
      _lastConfig = config;
    }
  }

  @override
  void didUpdateWidget(ConfigPanel oldWidget) {
    super.didUpdateWidget(oldWidget);
    final config = widget.service.config;
    if (config != null && config != _lastConfig) {
      _initFromConfig();
    }
  }

  @override
  Widget build(BuildContext context) {
    return Selector<AppService, (bool, bool, ConfigData?)>(
      selector: (_, service) => (service.isRunning, service.connected, service.config),
      builder: (context, data, child) {
        final (isRunning, connected, config) = data;
        _isRunning = isRunning;
        _connected = connected;
        
        if (widget.compact) {
          return LayoutBuilder(
            builder: (context, constraints) => _buildCompactLayout(isRunning, connected, constraints),
          );
        }
        
        return LayoutBuilder(
          builder: (context, constraints) => _buildNormalLayout(isRunning, connected, constraints),
        );
      },
    );
  }

  Widget _buildNormalLayout(bool isRunning, bool connected, BoxConstraints constraints) {
    const itemMinWidth = 160.0;
    final canFitTwo = constraints.maxWidth >= itemMinWidth * 2 + 12;
    
    return CustomScrollView(
      slivers: [
        SliverPadding(
          padding: const EdgeInsets.all(16),
          sliver: SliverList(
            delegate: SliverChildListDelegate([
              ..._buildFormChildren(enabled: !isRunning, canFitTwo: canFitTwo),
              const SizedBox(height: 16),
              _buildActionButtons(),
              const SizedBox(height: 16),
              _buildStatusCard(),
            ]),
          ),
        ),
        SliverFillRemaining(
          hasScrollBody: false,
          child: ConstrainedBox(
            constraints: const BoxConstraints(minHeight: 250),
            child: _buildLogPanel(),
          ),
        ),
      ],
    );
  }

  Widget _buildTwoColumnRow(bool canFitTwo, Widget left, Widget right, {double gap = 12}) {
    if (canFitTwo) {
      return Row(
        children: [
          Expanded(child: left),
          SizedBox(width: gap),
          Expanded(child: right),
        ],
      );
    }
    return Column(
      children: [
        left,
        SizedBox(height: gap),
        right,
      ],
    );
  }

  Widget _buildCompactLayout(bool isRunning, bool connected, BoxConstraints constraints) {
    final width = constraints.maxWidth;
    final padding = (width * 0.03).clamp(8.0, 12.0);
    final spacing = (width * 0.02).clamp(6.0, 10.0);
    final fontSize = (width * 0.035).clamp(12.0, 14.0);
    
    return CustomScrollView(
      slivers: [
        SliverPadding(
          padding: EdgeInsets.all(padding),
          sliver: SliverList(
            delegate: SliverChildListDelegate([
              ..._buildFormChildren(enabled: !isRunning, canFitTwo: true, fontSize: fontSize, spacing: spacing),
              SizedBox(height: spacing),
              _buildActionButtons(),
              SizedBox(height: spacing),
              _buildStatusCard(),
            ]),
          ),
        ),
        SliverFillRemaining(
          hasScrollBody: false,
          child: ConstrainedBox(
            constraints: const BoxConstraints(minHeight: 200),
            child: _buildLogPanel(),
          ),
        ),
      ],
    );
  }

  // 表单字段规格：控制器 + 标签 + 类型，normal/compact 两套布局共用
  List<({TextEditingController controller, String label, bool number, bool decimal})> get _formFieldPairs => [
    (controller: _delayLimitController, label: '延迟上限 (ms)', number: true, decimal: false),
    (controller: _tlrController, label: '丢包率上限', number: false, decimal: true),
    (controller: _maxConcurrencyController, label: '测速并发', number: true, decimal: false),
    (controller: _dataCenterController, label: '数据中心', number: false, decimal: false),
    (controller: _loadCountController, label: '负载数量', number: true, decimal: false),
    (controller: _maxStickySlotsController, label: '最大负载槽数', number: true, decimal: false),
    (controller: _tlsPortController, label: 'TLS 端口', number: true, decimal: false),
    (controller: _httpPortController, label: 'HTTP 端口', number: true, decimal: false),
  ];

  List<Widget> _buildFormChildren({
    required bool enabled,
    required bool canFitTwo,
    double? fontSize,
    double spacing = 12,
  }) {
    final children = <Widget>[
      _buildFilePickerField(controller: _speedTestFileController, label: '测速文件', enabled: enabled, fontSize: fontSize),
    ];

    final fields = _formFieldPairs;
    for (var i = 0; i + 1 < fields.length; i += 2) {
      final left = fields[i];
      final right = fields[i + 1];
      children.add(SizedBox(height: spacing));
      children.add(_buildTwoColumnRow(
        canFitTwo,
        _buildTextField(controller: left.controller, label: left.label, enabled: enabled, isNumber: left.number, isDecimal: left.decimal, fontSize: fontSize),
        _buildTextField(controller: right.controller, label: right.label, enabled: enabled, isNumber: right.number, isDecimal: right.decimal, fontSize: fontSize),
        gap: spacing,
      ));
    }

    children
      ..add(SizedBox(height: spacing))
      ..add(_buildTextField(controller: _testAddressController, label: '测速地址', enabled: enabled, fontSize: fontSize))
      ..add(SizedBox(height: spacing))
      ..add(_buildTextField(controller: _localServiceController, label: '本地服务', enabled: enabled, fontSize: fontSize));

    return children;
  }

  Widget _buildTextField({
    required TextEditingController controller,
    required String label,
    bool enabled = true,
    bool isNumber = false,
    bool isDecimal = false,
    double? fontSize,
  }) {
    final size = fontSize ?? 14.0;
    return TextField(
      controller: controller,
      enabled: enabled,
      keyboardType: isDecimal
          ? const TextInputType.numberWithOptions(decimal: true)
          : isNumber
              ? TextInputType.number
              : TextInputType.text,
      style: fontSize != null ? TextStyle(fontSize: size) : null,
      decoration: InputDecoration(
        labelText: label,
        floatingLabelBehavior: FloatingLabelBehavior.always,
        border: const OutlineInputBorder(),
        contentPadding: fontSize != null 
            ? EdgeInsets.symmetric(horizontal: 10, vertical: size * 0.6)
            : const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
        labelStyle: fontSize != null ? TextStyle(fontSize: size - 1) : null,
      ),
    );
  }

  Widget _buildFilePickerField({
    required TextEditingController controller,
    required String label,
    bool enabled = true,
    double? fontSize,
  }) {
    final size = fontSize ?? 14.0;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Expanded(
              child: TextField(
                controller: controller,
                enabled: enabled,
                style: fontSize != null ? TextStyle(fontSize: size) : null,
                onChanged: (_) => setState(() => _ipContent = null),
                decoration: InputDecoration(
                  labelText: label,
                  floatingLabelBehavior: FloatingLabelBehavior.always,
                  border: const OutlineInputBorder(),
                  contentPadding: fontSize != null 
                      ? EdgeInsets.symmetric(horizontal: 10, vertical: size * 0.6)
                      : const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                  labelStyle: fontSize != null ? TextStyle(fontSize: size - 1) : null,
                  suffixText: _ipContent != null ? '已从文件加载' : null,
                  suffixStyle: TextStyle(
                    color: AppColors.ok,
                    fontSize: (size - 2).clamp(10.0, 12.0),
                  ),
                ),
              ),
            ),
            const SizedBox(width: 8),
            IconButton(
              onPressed: enabled ? () => _pickFile(controller) : null,
              icon: const Icon(Icons.folder_open),
              tooltip: '选择文件',
              style: IconButton.styleFrom(
                backgroundColor: Theme.of(context).colorScheme.surfaceContainerHighest,
              ),
            ),
          ],
        ),
            const SizedBox(height: 4),
            InkWell(
              onTap: () => setState(() => _showManualInput = !_showManualInput),
              child: Row(
                children: [
                  Icon(
                    Icons.edit,
                    size: 16,
                    color: _showManualInput 
                        ? Theme.of(context).colorScheme.primary
                        : AppColors.textSecondary,
                  ),
                  const SizedBox(width: 6),
                  Text(
                    '$_manualIpCount 行',
                    style: TextStyle(
                      fontSize: (size - 2).clamp(10.0, 12.0),
                      color: AppColors.textSecondary,
                    ),
                  ),
                  const Spacer(),
                  Icon(
                    _showManualInput ? Icons.keyboard_arrow_up : Icons.keyboard_arrow_down,
                    size: 18,
                    color: AppColors.textSecondary,
                  ),
                ],
              ),
            ),
            AnimatedSize(
              duration: const Duration(milliseconds: 200),
              curve: Curves.easeInOut,
              child: _showManualInput
                  ? Column(
                      children: [
                        const SizedBox(height: 8),
                        ClipRRect(
                          borderRadius: BorderRadius.circular(8),
                          child: TextField(
                            controller: _manualInputController,
                            readOnly: !enabled,
                            maxLines: 4,
                            style: fontSize != null ? TextStyle(fontSize: size) : null,
                            onChanged: (_) => _parseManualInput(),
                            scrollPadding: EdgeInsets.zero,
                            decoration: InputDecoration(
                              border: const OutlineInputBorder(),
                              contentPadding: const EdgeInsets.fromLTRB(8, 8, 4, 8),
                              filled: !enabled,
                              fillColor: enabled ? null : Theme.of(context).colorScheme.surfaceContainerHighest,
                            ),
                          ),
                        ),
                      ],
                    )
                  : const SizedBox.shrink(),
            ),
          ],
        );
  }

  void _parseManualInput() {
    final text = _manualInputController.text;
    setState(() {
      _manualIps = text.split('\n')
          .map((line) => line.trim())
          .where((line) => line.isNotEmpty)
          .toList();
    });
  }

  Future<void> _pickFile(TextEditingController controller) async {
    try {
      final file = await FilePicker.pickFile(
        type: FileType.custom,
        allowedExtensions: ['txt'],
        dialogTitle: '选择测速文件',
      );
      
      if (file != null) {
        final path = file.path;
        
        if (path != null) {
          setState(() {
            _ipContent = null;
          });
          controller.text = path;
        } else {
          final bytes = await file.readAsBytes();
          final content = String.fromCharCodes(bytes);
          setState(() {
            _ipContent = content.split('\n')
                .map((line) => line.trim())
                .where((line) => line.isNotEmpty)
                .toList();
          });
          controller.text = file.name;
        }
      }
    } catch (e) {
      debugPrint('文件选择失败: $e');
    }
  }

  Widget _buildActionButtons() {
    if (!_connected) {
      return Center(
        child: Text(
          '等待连接...',
          style: TextStyle(color: AppColors.textMuted, fontSize: 12),
        ),
      );
    }

    if (_isRunning) {
      return SizedBox(
        width: double.infinity,
        child: ElevatedButton.icon(
          onPressed: _actionInProgress
              ? null
              : () async {
                  await _runAction(() => widget.service.stopService(), '停止');
                },
          icon: _actionInProgress
              ? const SizedBox(
                  width: 18,
                  height: 18,
                  child: CircularProgressIndicator(strokeWidth: 2),
                )
              : const Icon(Icons.stop, size: 18),
          label: Text(_actionInProgress ? '停止中...' : '停止'),
          style: ElevatedButton.styleFrom(
            backgroundColor: AppColors.dangerStrong,
            foregroundColor: Colors.white,
            padding: const EdgeInsets.symmetric(vertical: 12),
          ),
        ),
      );
    }

    return SizedBox(
      width: double.infinity,
      child: ElevatedButton.icon(
        onPressed: _actionInProgress
            ? null
            : () async {
                String? ipFileToSend;
                List<String>? ipContentToSend;
                
                if (_manualIps.isNotEmpty) {
                  ipContentToSend = _manualIps;
                }
                
                if (!kIsWeb && _speedTestFileController.text.isNotEmpty) {
                  ipFileToSend = _speedTestFileController.text;
                } else if (_ipContent != null) {
                  ipContentToSend = [...ipContentToSend ?? [], ..._ipContent!];
                }
                
                await _runAction(
                  () => widget.service.startService(
                    ipFile: ipFileToSend,
                    ipContent: ipContentToSend,
                    http: _testAddressController.text.isNotEmpty 
                        ? _testAddressController.text : null,
                    delayLimit: int.tryParse(_delayLimitController.text),
                    tlr: double.tryParse(_tlrController.text),
                    ips: int.tryParse(_loadCountController.text),
                    threads: int.tryParse(_maxConcurrencyController.text),
                    tlsPort: int.tryParse(_tlsPortController.text),
                    httpPort: int.tryParse(_httpPortController.text),
                    colo: _dataCenterController.text.isNotEmpty
                        ? _dataCenterController.text.split(',').map((e) => e.trim()).toList()
                        : null,
                    addr: _localServiceController.text.isNotEmpty 
                        ? _localServiceController.text : null,
                    maxStickySlots: int.tryParse(_maxStickySlotsController.text),
                  ),
                  '启动',
                );
              },
        icon: _actionInProgress
            ? const SizedBox(
                width: 18,
                height: 18,
                child: CircularProgressIndicator(strokeWidth: 2),
              )
            : const Icon(Icons.play_arrow, size: 18),
        label: Text(_actionInProgress ? '启动中...' : '启动'),
        style: ElevatedButton.styleFrom(
          backgroundColor: AppColors.okStrong,
          foregroundColor: Colors.white,
          padding: const EdgeInsets.symmetric(vertical: 12),
        ),
      ),
    );
  }

  Widget _buildStatusCard() {
    return Selector<AppService, (StatusData?, bool)>(
      selector: (_, service) => (service.status, service.connected),
      builder: (context, data, child) {
        final (status, connected) = data;
        final isRunning = status?.running ?? false;
        final uptime = status?.uptimeSecs ?? 0;
        final primaryCount = status?.primaryCount ?? 0;
        final primaryTarget = status?.primaryTarget ?? 0;
        final backupCount = status?.backupCount ?? 0;
        final backupTarget = status?.backupTarget ?? 0;
        final stickyCount = status?.stickyIps.length ?? 0;
        
        return Card(
          elevation: 0,
          margin: EdgeInsets.zero,
          clipBehavior: Clip.antiAlias,
          child: Column(
            children: [
              _buildStatusBar(isRunning, connected, isRunning ? uptime : 0),
              Padding(
                padding: const EdgeInsets.all(12),
                child: Column(
                  children: [
                    _buildQueueRow(Icons.dns, primaryCount, primaryTarget, AppColors.ok),
                    const SizedBox(height: 8),
                    _buildQueueRow(Icons.backup, backupCount, backupTarget, AppColors.info),
                    const SizedBox(height: 8),
                    _buildQueueRow(Icons.push_pin, stickyCount, primaryTarget, AppColors.accent),
                  ],
                ),
              ),
            ],
          ),
        );
      },
    );
  }

  Widget _buildStatusBar(bool isRunning, bool connected, int uptime) {
    Color bgColor;
    Color borderColor;
    Color textColor;
    IconData icon;
    String text;

    if (isRunning) {
      bgColor = AppColors.ok.withValues(alpha: 0.15);
      borderColor = AppColors.okStrong;
      textColor = AppColors.ok;
      icon = Icons.play_circle;
      text = _formatUptime(uptime);
    } else if (connected) {
      bgColor = AppColors.info.withValues(alpha: 0.15);
      borderColor = AppColors.infoStrong;
      textColor = AppColors.info;
      icon = Icons.pause_circle;
      text = '已就绪';
    } else {
      bgColor = AppColors.warn.withValues(alpha: 0.15);
      borderColor = AppColors.warnStrong;
      textColor = AppColors.warn;
      icon = Icons.warning;
      text = '未就绪';
    }

    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
      decoration: BoxDecoration(
        color: bgColor,
        border: Border(bottom: BorderSide(color: borderColor)),
      ),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Icon(icon, size: 16, color: textColor),
          const SizedBox(width: 6),
          Text(
            text,
            style: TextStyle(
              fontSize: 12,
              color: textColor,
              fontFeatures: const [FontFeature.tabularFigures()],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildQueueRow(IconData icon, int count, int target, Color color) {
    final progress = target > 0 ? (count / target).clamp(0.0, 1.0) : 0.0;
    
    return Row(
      children: [
        Icon(icon, size: 18, color: color),
        const SizedBox(width: 8),
        Expanded(
          child: ClipRRect(
            borderRadius: BorderRadius.circular(8),
            child: LinearProgressIndicator(
              value: progress,
              backgroundColor: AppColors.border,
              valueColor: AlwaysStoppedAnimation(color),
              minHeight: 6,
            ),
          ),
        ),
        const SizedBox(width: 8),
        SizedBox(
          width: 50,
          child: Text(
            '$count/$target',
            style: TextStyle(
              fontSize: 12,
              color: AppColors.textPrimary,
              fontFeatures: const [FontFeature.tabularFigures()],
            ),
            textAlign: TextAlign.right,
          ),
        ),
      ],
    );
  }

  Widget _buildLogPanel() {
    return _LogPanel(service: widget.service);
  }

  String _formatUptime(int seconds) {
    final h = seconds ~/ 3600;
    final m = (seconds % 3600) ~/ 60;
    final s = seconds % 60;
    return '${h.toString().padLeft(2, '0')}:${m.toString().padLeft(2, '0')}:${s.toString().padLeft(2, '0')}';
  }

  Future<void> _runAction(Future<bool> Function() action, String label) async {
    if (_actionInProgress || !mounted) {
      return;
    }
    setState(() {
      _actionInProgress = true;
    });
    final success = await action();
    if (!mounted) {
      return;
    }
    setState(() {
      _actionInProgress = false;
    });
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(success ? '$label成功' : '$label失败')),
    );
  }
}

class _LogPanel extends StatefulWidget {
  final AppService service;
  
  const _LogPanel({required this.service});

  @override
  State<_LogPanel> createState() => _LogPanelState();
}

class _LogPanelState extends State<_LogPanel> {
  static const int _maxLogEntries = 500; // 与后端 MAX_LOG_ENTRIES 一致
  List<LogEntry> _logs = [];
  bool _loading = false;
  Timer? _refreshTimer;

  @override
  void initState() {
    super.initState();
    _fetchLogs();
    _refreshTimer = Timer.periodic(const Duration(seconds: 5), (_) => _fetchLogs());
  }

  @override
  void dispose() {
    _refreshTimer?.cancel();
    super.dispose();
  }

  Future<void> _fetchLogs() async {
    if (_loading) return;
    _loading = true;
    final logs = await widget.service.fetchLogs();
    if (mounted) {
      setState(() {
        _logs = logs;
      });
    }
    _loading = false;
  }

  @override
  Widget build(BuildContext context) {
    return Container(
      margin: const EdgeInsets.fromLTRB(16, 0, 16, 16),
      decoration: BoxDecoration(
        color: AppColors.panelBg,
        borderRadius: BorderRadius.circular(12),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
            child: Row(
              children: [
                Icon(Icons.article, size: 14, color: AppColors.textSecondary),
                const SizedBox(width: 6),
                Text(
                  '日志',
                  style: TextStyle(fontSize: 12, color: AppColors.textSecondary),
                ),
                const Spacer(),
                Text(
                  '${_logs.length}/$_maxLogEntries',
                  style: TextStyle(fontSize: 11, color: AppColors.textMuted),
                ),
                const SizedBox(width: 8),
                InkWell(
                  onTap: () async {
                    await widget.service.clearLogs();
                    await _fetchLogs();
                  },
                  borderRadius: BorderRadius.circular(8),
                  child: Padding(
                    padding: const EdgeInsets.all(4),
                    child: Icon(Icons.delete_outline, size: 16, color: AppColors.textMuted),
                  ),
                ),
              ],
            ),
          ),
          Divider(height: 1, color: AppColors.border),
          Expanded(
            child: ClipRRect(
              borderRadius: const BorderRadius.only(
                bottomLeft: Radius.circular(8),
                bottomRight: Radius.circular(8),
              ),
              child: _logs.isEmpty
                  ? const Padding(
                      padding: EdgeInsets.all(16),
                      child: Center(
                        child: Text('暂无日志'),
                      ),
                    )
                  : ListView.builder(
                      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                      itemCount: _logs.length,
                      itemExtent: 22,
                      itemBuilder: (context, index) {
                        return _buildLogItem(_logs[index]);
                      },
                    ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildLogItem(LogEntry log) {
    Color levelColor;
    switch (log.level) {
      case 'ERROR':
        levelColor = AppColors.danger;
        break;
      case 'WARN':
        levelColor = AppColors.warn;
        break;
      default:
        levelColor = AppColors.textMuted;
    }

    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 8),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.center,
        children: [
          Text(
            log.timestamp,
            style: TextStyle(
              fontSize: 10,
              color: AppColors.textMuted,
              fontFeatures: const [FontFeature.tabularFigures()],
            ),
          ),
          const SizedBox(width: 6),
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 4),
            decoration: BoxDecoration(
              color: levelColor.withValues(alpha: 0.15),
              borderRadius: BorderRadius.circular(4),
            ),
            child: Text(
              log.level.padRight(5),
              style: TextStyle(fontSize: 9, color: levelColor),
            ),
          ),
          const SizedBox(width: 6),
          Expanded(
            child: Text(
              log.message,
              style: const TextStyle(fontSize: 10),
              overflow: TextOverflow.ellipsis,
              maxLines: 1,
            ),
          ),
        ],
      ),
    );
  }
}