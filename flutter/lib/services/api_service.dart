import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:http/http.dart';
import 'app_service.dart';

class ApiService extends AppService {
  StatusData? _status;
  ConfigData? _config;
  bool _connected = false;
  StreamSubscription<String>? _streamSubscription;
  Client? _streamClient;
  int _streamGeneration = 0;
  Timer? _reconnectTimer;

  @override
  StatusData? get status => _status;
  @override
  ConfigData? get config => _config;
  @override
  bool get connected => _connected;
  @override
  bool get isRunning => _status?.running ?? false;

  ApiService() {
    _startStreaming();
  }

  void _handleDisconnect() {
    _connected = false;
    _status = null;
    notifyListeners();
  }

  void _scheduleReconnect() {
    _reconnectTimer?.cancel();
    _reconnectTimer = Timer(const Duration(seconds: 1), _startStreaming);
  }

  void _startStreaming() async {
    final generation = ++_streamGeneration;
    try {
      final client = Client();
      _streamClient = client;
      final request = Request('GET', Uri.parse('/api/stream'));
      
      final response = await client.send(request);
      if (generation != _streamGeneration) {
        client.close();
        return;
      }
      
      _streamSubscription = response.stream
          .transform(utf8.decoder)
          .transform(const LineSplitter())
          .listen(
            (line) {
              if (generation != _streamGeneration) {
                return;
              }
              if (line.isNotEmpty && line.startsWith('data: ')) {
                try {
                  final jsonStr = line.substring(6);
                  final data = json.decode(jsonStr);
                  
                  if (data['status'] != null) {
                    _status = StatusData.fromJson(data['status']);
                  }
                  if (data['config'] != null) {
                    _config = ConfigData.fromJson(data['config']);
                  }
                  _connected = true;
                  notifyListeners();
                } catch (e) {
                  debugPrint('解析SSE数据失败: $e');
                }
              }
            },
            onError: (e) {
              if (generation != _streamGeneration) {
                return;
              }
              debugPrint('SSE连接错误: $e');
              _handleDisconnect();
              _scheduleReconnect();
            },
            onDone: () {
              if (generation != _streamGeneration) {
                return;
              }
              debugPrint('SSE连接关闭');
              _handleDisconnect();
              _scheduleReconnect();
            },
          );
    } catch (e) {
      debugPrint('启动SSE失败: $e');
      _handleDisconnect();
      _scheduleReconnect();
    }
  }

  @override
  void dispose() {
    _reconnectTimer?.cancel();
    _streamSubscription?.cancel();
    _streamClient?.close();
    super.dispose();
  }

  Future<void> _restartStreaming() async {
    _streamGeneration++;
    _reconnectTimer?.cancel();
    await _streamSubscription?.cancel();
    _streamClient?.close();
    _streamSubscription = null;
    _streamClient = null;
    _startStreaming();
  }

  @override
  Future<void> fetchStatus() async {
    try {
      final response = await get(Uri.parse('/api/status'));
      if (response.statusCode == 200) {
        _status = StatusData.fromJson(json.decode(response.body));
        _connected = true;
      }
    } catch (e) {
      debugPrint('获取状态失败: $e');
      _handleDisconnect();
    }
    notifyListeners();
  }

  @override
  Future<void> fetchConfig() async {
    try {
      final response = await get(Uri.parse('/api/config'));
      if (response.statusCode == 200) {
        _config = ConfigData.fromJson(json.decode(response.body));
        _connected = true;
      }
    } catch (e) {
      debugPrint('获取配置失败: $e');
    }
    notifyListeners();
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
      final body = <String, dynamic>{
        'ip_file': ipFile,
        'ip_content': ipContent,
        'http': http,
        'delay_limit': delayLimit,
        'tlr': tlr,
        'ips': ips,
        'threads': threads,
        'tls_port': tlsPort,
        'http_port': httpPort,
        'colo': colo,
        'addr': addr,
        'max_sticky_slots': maxStickySlots,
      }..removeWhere((key, value) => value == null);

      final response = await post(
        Uri.parse('/api/start'),
        headers: {'Content-Type': 'application/json'},
        body: json.encode(body),
      );

      if (response.statusCode == 200) {
        final result = json.decode(response.body);
        final success = result['success'] == true;
        if (success) {
          await _restartStreaming();
          await fetchConfig();
        }
        return success;
      }
      return false;
    } catch (e) {
      debugPrint('启动服务失败: $e');
      return false;
    }
  }

  @override
  Future<bool> stopService() async {
    try {
      final response = await post(Uri.parse('/api/stop'));
      if (response.statusCode == 200) {
        final result = json.decode(response.body);
        final success = result['success'] == true;
        if (success) {
          _status = StatusData.stopped();
          notifyListeners();
          await _restartStreaming();
        }
        return success;
      }
      return false;
    } catch (e) {
      debugPrint('停止服务失败: $e');
      return false;
    }
  }

  @override
  Future<List<LogEntry>> fetchLogs() async {
    try {
      final response = await get(Uri.parse('/api/logs'));
      if (response.statusCode == 200) {
        final List<dynamic> data = json.decode(response.body);
        return data.map((e) => LogEntry.fromJson(e)).toList();
      }
      return [];
    } catch (e) {
      debugPrint('获取日志失败: $e');
      return [];
    }
  }

  @override
  Future<bool> clearLogs() async {
    try {
      final response = await post(Uri.parse('/api/logs/clear'));
      if (response.statusCode == 200) {
        final result = json.decode(response.body);
        return result['success'] == true;
      }
      return false;
    } catch (e) {
      debugPrint('清空日志失败: $e');
      return false;
    }
  }
}