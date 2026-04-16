import 'dart:async';
import 'package:flutter/material.dart';
import '../bridge_generated.dart/lib.dart' as rust;
import 'app_service.dart';

export '../bridge_generated.dart/frb_generated.dart' show RustLib;

class RustService extends AppService {
  StatusData? _status;
  ConfigData? _config;
  bool _initialized = false;
  String? _error;
  Timer? _pollTimer;

  @override
  StatusData? get status => _status;
  @override
  ConfigData? get config => _config;
  bool get initialized => _initialized;
  String? get error => _error;
  @override
  bool get connected => _initialized;
  @override
  bool get isRunning => _status?.running ?? false;
  @override
  bool get isLoading => false;

  Future<void> initialize() async {
    try {
      await fetchConfig();
      _initialized = true;
      _startPolling();
      notifyListeners();
    } catch (e) {
      _error = e.toString();
      debugPrint('Rust init error: $_error');
      notifyListeners();
    }
  }

  void _startPolling() {
    _pollTimer?.cancel();
    _pollTimer = Timer.periodic(const Duration(seconds: 1), (_) async {
      if (_initialized) {
        await fetchStatus();
      }
    });
  }

  @override
  void dispose() {
    _pollTimer?.cancel();
    super.dispose();
  }

  @override
  Future<void> fetchStatus() async {
    try {
      final result = await rust.getStatus();
      _status = StatusData(
        running: result.running,
        uptimeSecs: result.uptimeSecs.toInt(),
        nextHealthCheck: result.nextHealthCheck.toInt(),
        healthCheckInterval: result.healthCheckInterval.toInt(),
        primaryCount: result.primaryCount,
        primaryTarget: result.primaryTarget,
        backupCount: result.backupCount,
        backupTarget: result.backupTarget,
        stickyIps: result.stickyIps,
        primaryIps: result.primaryIps.map((e) => IpInfo(
          ip: e.ip,
          colo: e.colo,
          delay: e.delay,
          loss: e.loss,
          samples: e.samples,
        )).toList(),
        backupIps: result.backupIps.map((e) => IpInfo(
          ip: e.ip,
          colo: e.colo,
          delay: e.delay,
          loss: e.loss,
          samples: e.samples,
        )).toList(),
      );
      notifyListeners();
    } catch (e) {
      debugPrint('获取状态失败: $e');
    }
  }

  @override
  Future<void> fetchConfig() async {
    try {
      final result = await rust.getConfig();
      _config = ConfigData(
        addr: result.addr,
        delayLimit: result.delayLimit.toInt(),
        tlr: result.tlr,
        ips: result.ips,
        threads: result.threads,
        tlsPort: result.tlsPort,
        httpPort: result.httpPort,
        http: result.http,
        ipFile: result.ipFile,
        maxStickySlots: result.maxStickySlots,
        colo: result.colo,
      );
      notifyListeners();
    } catch (e) {
      debugPrint('获取配置失败: $e');
    }
  }

  @override
  Future<bool> startService({
    String? ipFile,
    List<String>? ipContent,
    String? http,
    int? delayLimit,
    double? tlr,
    int? ips,
    int? threads,
    int? tlsPort,
    int? httpPort,
    List<String>? colo,
    String? addr,
    int? maxStickySlots,
  }) async {
    try {
      final success = await rust.startService(
        ipFile: ipFile,
        ipContent: ipContent,
        http: http,
        delayLimit: delayLimit != null ? BigInt.from(delayLimit) : null,
        tlr: tlr,
        ips: ips,
        threads: threads,
        tlsPort: tlsPort,
        httpPort: httpPort,
        maxStickySlots: maxStickySlots,
        addr: addr,
        colo: colo,
      );
      if (success) {
        await fetchConfig();
      }
      return success;
    } catch (e) {
      debugPrint('启动服务失败: $e');
      return false;
    }
  }

  @override
  Future<bool> stopService() async {
    try {
      final success = await rust.stopService();
      if (success) {
        _status = StatusData.stopped();
        notifyListeners();
      }
      return success;
    } catch (e) {
      debugPrint('停止服务失败: $e');
      return false;
    }
  }

  @override
  Future<List<LogEntry>> fetchLogs() async {
    try {
      final logs = await rust.getLogs();
      return logs.map((e) => LogEntry(
        timestamp: e.timestamp,
        level: e.level,
        message: e.message,
      )).toList();
    } catch (e) {
      debugPrint('获取日志失败: $e');
      return [];
    }
  }

  @override
  Future<bool> clearLogs() async {
    try {
      await rust.clearLogs();
      return true;
    } catch (e) {
      debugPrint('清空日志失败: $e');
      return false;
    }
  }
}