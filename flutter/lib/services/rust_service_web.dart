import 'app_service.dart';

class RustLib {
  static Future<void> init() async {}
}

class RustService extends AppService {
  @override
  StatusData? get status => null;
  @override
  ConfigData? get config => null;
  @override
  bool get connected => false;
  @override
  bool get isLoading => false;
  @override
  bool get isRunning => false;

  Future<void> initialize() async {}

  @override
  Future<void> fetchStatus() async {}
  @override
  Future<void> fetchConfig() async {}
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
  }) async => false;
  @override
  Future<bool> stopService() async => false;
  @override
  Future<List<LogEntry>> fetchLogs() async => [];
  @override
  Future<bool> clearLogs() async => false;
}