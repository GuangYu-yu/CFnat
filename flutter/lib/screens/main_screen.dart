import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/app_service.dart';
import '../widgets/config_panel.dart';
import '../widgets/info_panel.dart';

class LayoutConstants {
  static const double configBaseWidth = 280;
  static const double configMaxWidth = 650;
  static const double listMinWidth = 700;
  static const double listGap = 10;
  static const double dividerWidth = 1;
  
  static double get listSideBySideThreshold => listMinWidth * 2 + listGap;
  static double get narrowThreshold => configBaseWidth + listMinWidth + dividerWidth;
  static double get verticalSplitMinHeight => 400;
}

class MainScreen extends StatefulWidget {
  const MainScreen({super.key});

  @override
  State<MainScreen> createState() => _MainScreenState();
}

class _MainScreenState extends State<MainScreen> {
  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('CFnat'),
        centerTitle: true,
      ),
      body: Consumer<AppService>(
        builder: (context, service, child) {
          return LayoutBuilder(
            builder: (context, constraints) {
              final width = constraints.maxWidth;
              final isNarrow = width < LayoutConstants.narrowThreshold;

              if (isNarrow) {
                return _buildNarrowLayout(service);
              }

              return _buildFlexibleLayout(service);
            },
          );
        },
      ),
    );
  }

  Widget _buildNarrowLayout(AppService service) {
    final colorScheme = Theme.of(context).colorScheme;
    final dividerColor = Theme.of(context).dividerColor;

    return DefaultTabController(
      length: 2,
      child: Column(
        children: [
          Container(
            decoration: BoxDecoration(
              color: colorScheme.surface.withValues(alpha: 0.35),
              border: Border(bottom: BorderSide(color: dividerColor)),
            ),
            child: const TabBar(
              tabs: [
                Tab(text: '配置'),
                Tab(text: '列表'),
              ],
            ),
          ),
          Expanded(
            child: TabBarView(
              children: [
                ConfigPanel(service: service, compact: true),
                InfoPanel(service: service, forceVertical: true),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildFlexibleLayout(AppService service) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final totalWidth = constraints.maxWidth;
        final minWidthForSideBySide = LayoutConstants.configBaseWidth + 
            LayoutConstants.listSideBySideThreshold + 
            LayoutConstants.dividerWidth;
        
        double configWidth;
        if (totalWidth >= minWidthForSideBySide) {
          final availableForConfig = totalWidth - 
              LayoutConstants.listSideBySideThreshold - 
              LayoutConstants.dividerWidth;
          configWidth = availableForConfig.clamp(
            LayoutConstants.configBaseWidth,
            LayoutConstants.configMaxWidth,
          );
        } else {
          final availableForConfig = totalWidth - 
              LayoutConstants.listMinWidth - 
              LayoutConstants.dividerWidth;
          configWidth = availableForConfig.clamp(
            LayoutConstants.configBaseWidth,
            LayoutConstants.configMaxWidth,
          );
        }
        
        return Row(
          children: [
            SizedBox(
              width: configWidth,
              child: ConfigPanel(service: service),
            ),
            const VerticalDivider(width: LayoutConstants.dividerWidth),
            Expanded(
              child: InfoPanel(service: service),
            ),
          ],
        );
      },
    );
  }
}