import 'package:flutter/material.dart';

/// 全局配色常量（应用固定深色模式，用常量比 ThemeExtension 更简单）
abstract final class AppColors {
  // 面板基础色（与 main.dart 主题保持一致）
  static const Color panelBg = Color(0xFF161B22); // 卡片/日志面板背景
  static const Color rowBg = Color(0xFF212121); // 列表行背景
  static const Color border = Color(0xFF2A2F3A); // 分隔线/边框

  // 文本
  static const Color textPrimary = Color(0xFFE0E0E0);
  static const Color textSecondary = Color(0xFFBDBDBD);
  static const Color textMuted = Color(0xFF9E9E9E);

  // 语义色
  static const Color ok = Color(0xFF66BB6A); // 正常/主队列
  static const Color okStrong = Color(0xFF388E3C); // 启动按钮
  static const Color info = Color(0xFF42A5F5); // 备选队列
  static const Color infoStrong = Color(0xFF1976D2);
  static const Color warn = Color(0xFFFFA726); // 警告
  static const Color warnStrong = Color(0xFFF57C00);
  static const Color danger = Color(0xFFEF5350); // 危险
  static const Color dangerStrong = Color(0xFFD32F2F); // 停止按钮
  static const Color accent = Color(0xFFAB47BC); // sticky 强调
  static const Color accentLight = Color(0xFFBA68C8);
}