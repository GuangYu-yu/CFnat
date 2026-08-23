import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../screens/main_screen.dart' show LayoutConstants;
import '../services/app_service.dart';
import '../theme.dart';

class InfoPanel extends StatefulWidget {
  final AppService service;
  final bool forceVertical;
  
  const InfoPanel({super.key, required this.service, this.forceVertical = false});

  @override
  State<InfoPanel> createState() => _InfoPanelState();
}

class _InfoPanelState extends State<InfoPanel> {
  @override
  Widget build(BuildContext context) {
    return Selector<AppService, (StatusData?, bool)>(
      selector: (_, service) => (service.status, service.connected),
      builder: (context, data, child) {
        final (status, connected) = data;
        if (!connected) {
          return _buildDisconnectedState();
        }

        if (status == null) {
          return const Center(child: CircularProgressIndicator());
        }

        if (!status.running) {
          return _buildIdleState();
        }

        return LayoutBuilder(
          builder: (context, constraints) {
            return Padding(
              padding: const EdgeInsets.all(12),
              child: Column(
                children: [
                  Expanded(child: _buildContentArea(status, constraints)),
                ],
              ),
            );
          },
        );
      },
    );
  }

  Widget _buildContentArea(StatusData status, BoxConstraints constraints) {
    final canFitTwo = constraints.maxWidth >= LayoutConstants.listSideBySideThreshold && !widget.forceVertical;
    if (canFitTwo) {
      return _buildSideBySide(status, constraints);
    }
    if (constraints.maxHeight >= LayoutConstants.verticalSplitMinHeight) {
      return _buildVerticalSplit(status, constraints);
    }
    return _buildScrollable(status, constraints);
  }

  Widget _buildSideBySide(StatusData status, BoxConstraints constraints) => Row(
        children: [
          Expanded(child: _buildPrimaryList(status, constraints)),
          const SizedBox(width: 10),
          Expanded(child: _buildBackupList(status, constraints)),
        ],
      );

  Widget _buildVerticalSplit(StatusData status, BoxConstraints constraints) => Column(
        children: [
          Expanded(child: _buildPrimaryList(status, constraints)),
          const SizedBox(height: 10),
          Expanded(child: _buildBackupList(status, constraints)),
        ],
      );

  Widget _buildScrollable(StatusData status, BoxConstraints constraints) => ListView(
        children: [
          SizedBox(height: 360, child: _buildPrimaryList(status, constraints)),
          const SizedBox(height: 10),
          SizedBox(height: 360, child: _buildBackupList(status, constraints)),
        ],
      );

  Widget _buildPrimaryList(StatusData status, BoxConstraints constraints) => _buildIpList(
        '负载均衡',
        status.primaryIps,
        status.primaryCount,
        status.primaryTarget,
        AppColors.ok,
        status.stickyIps,
        constraints,
      );

  Widget _buildBackupList(StatusData status, BoxConstraints constraints) => _buildIpList(
        '备选列表',
        status.backupIps,
        status.backupCount,
        status.backupTarget,
        AppColors.info,
        status.stickyIps,
        constraints,
      );

  Widget _buildDisconnectedState() {
    return Center(
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Icon(Icons.cloud_off, size: 64, color: AppColors.textMuted),
          const SizedBox(height: 16),
          Text(
            '后端已断开',
            style: TextStyle(fontSize: 16, color: AppColors.textMuted),
          ),
          const SizedBox(height: 8),
          Text(
            '正在自动重连...',
            style: TextStyle(fontSize: 12, color: AppColors.textMuted),
          ),
        ],
      ),
    );
  }

  Widget _buildIdleState() {
    return Center(
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Icon(Icons.play_circle_outline, size: 64, color: AppColors.textMuted),
          const SizedBox(height: 16),
          Text(
            '等待启动',
            style: TextStyle(fontSize: 16, color: AppColors.textMuted),
          ),
          const SizedBox(height: 8),
          Text(
            '点击"启动"来运行',
            style: TextStyle(fontSize: 12, color: AppColors.textMuted),
          ),
        ],
      ),
    );
  }

  Widget _buildIpList(
    String title,
    List<IpInfo> ips,
    int count,
    int target,
    Color color,
    List<String> stickyIps,
    BoxConstraints constraints,
  ) {
    final padding = constraints.maxWidth > 600 ? 12.0 : 8.0;
    final titleSize = constraints.maxWidth > 400 ? 14.0 : 13.0;
    final ipSize = constraints.maxWidth > 400 ? 13.0 : 12.0;
    final headerSize = constraints.maxWidth > 400 ? 11.0 : 10.0;
    
    return Card(
      elevation: 0,
      clipBehavior: Clip.antiAlias,
      child: Column(
        children: [
        Container(
            padding: EdgeInsets.symmetric(horizontal: padding, vertical: padding * 0.7),
            decoration: BoxDecoration(
              color: color.withValues(alpha: 0.15),
              border: Border(bottom: BorderSide(color: AppColors.border)),
            ),
            child: Center(
              child: Text(
                title,
                style: TextStyle(
                  fontSize: titleSize,
                  fontWeight: FontWeight.bold,
                  color: color,
                ),
              ),
            ),
          ),
          Container(
            padding: EdgeInsets.symmetric(horizontal: padding, vertical: padding * 0.5),
            decoration: BoxDecoration(
              color: AppColors.rowBg,
              border: Border(bottom: BorderSide(color: AppColors.border)),
            ),
            child: Row(
              children: [
                Expanded(
                  flex: 3,
                  child: Text('IP', style: TextStyle(fontSize: headerSize, color: AppColors.textSecondary)),
                ),
                Expanded(
                  flex: 1,
                  child: Text('延迟', style: TextStyle(fontSize: headerSize, color: AppColors.textSecondary), textAlign: TextAlign.right),
                ),
                Expanded(
                  flex: 1,
                  child: Text('丢包', style: TextStyle(fontSize: headerSize, color: AppColors.textSecondary), textAlign: TextAlign.right),
                ),
                Expanded(
                  flex: 1,
                  child: Text('采样', style: TextStyle(fontSize: headerSize, color: AppColors.textSecondary), textAlign: TextAlign.right),
                ),
              ],
            ),
          ),
          Expanded(
            child: ips.isEmpty
                ? Center(
                    child: Text('暂无数据', style: TextStyle(color: AppColors.textMuted, fontSize: ipSize)),
                  )
              : ListView.builder(
                  itemCount: ips.length,
                  itemBuilder: (context, index) {
                    return _buildIpRow(ips[index], stickyIps, padding, ipSize);
                  },
                ),
        ),
        ],
      ),
    );
  }

  Widget _buildIpRow(IpInfo ip, List<String> stickyIps, double padding, double fontSize) {
    final delayColor = _getDelayColor(ip.delay);
    final lossColor = _getLossColor(ip.loss);
    final isSticky = stickyIps.contains(ip.ip);

    return Container(
      padding: EdgeInsets.symmetric(horizontal: padding, vertical: padding * 0.7),
      decoration: BoxDecoration(
        color: isSticky ? AppColors.accent.withValues(alpha: 0.15) : null,
        border: Border(
          bottom: BorderSide(color: AppColors.border),
          left: isSticky 
              ? const BorderSide(color: AppColors.accent, width: 3)
              : BorderSide.none,
        ),
      ),
      child: Row(
        children: [
          Expanded(
            flex: 3,
            child: Row(
              children: [
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          if (isSticky) ...[
                            Icon(Icons.bolt, size: fontSize, color: AppColors.accentLight),
                            SizedBox(width: padding * 0.3),
                          ],
                          Expanded(
                            child: Text(
                              ip.ip,
                              style: TextStyle(
                                fontSize: fontSize,
                                fontWeight: isSticky ? FontWeight.w600 : FontWeight.w500,
                                color: isSticky ? AppColors.accentLight : null,
                              ),
                            ),
                          ),
                        ],
                      ),
                      if (ip.colo != null && ip.colo!.isNotEmpty)
                        Padding(
                          padding: EdgeInsets.only(left: isSticky ? fontSize + padding * 0.3 : 0),
                          child: Text(
                            ip.colo!,
                            style: TextStyle(fontSize: fontSize - 2, color: AppColors.textSecondary),
                          ),
                        ),
                    ],
                  ),
                ),
              ],
            ),
          ),
          Expanded(
            flex: 1,
            child: Text(
              ip.delay > 0 ? '${ip.delay.toStringAsFixed(0)}ms' : '-',
              style: TextStyle(fontSize: fontSize - 1, color: delayColor, fontWeight: FontWeight.w500),
              textAlign: TextAlign.right,
            ),
          ),
          Expanded(
            flex: 1,
            child: Text(
              '${(ip.loss * 100).toStringAsFixed(1)}%',
              style: TextStyle(fontSize: fontSize - 1, color: lossColor, fontWeight: FontWeight.w500),
              textAlign: TextAlign.right,
            ),
          ),
          Expanded(
            flex: 1,
            child: Text(
              '${ip.samples}',
              style: TextStyle(fontSize: fontSize - 1),
              textAlign: TextAlign.right,
            ),
          ),
        ],
      ),
    );
  }

  Color _getDelayColor(double delay) {
    if (delay <= 0) return AppColors.textMuted;
    if (delay < 100) return AppColors.ok;
    if (delay < 300) return AppColors.warn;
    return AppColors.danger;
  }

  Color _getLossColor(double loss) {
    if (loss < 0.01) return AppColors.ok;
    if (loss < 0.05) return AppColors.warn;
    return AppColors.danger;
  }
}