import 'api_service.dart';

class RustLib {
  static Future<void> init() async {}
}

/// Web 端无 FFI 桥接：RustService 即基于 HTTP/SSE 的 ApiService 实现
class RustService extends ApiService {
  /// Web 端无需额外初始化（ApiService 构造即启动 SSE 连接）
  void initialize() {}
}