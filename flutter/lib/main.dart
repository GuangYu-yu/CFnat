import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'screens/main_screen.dart';
import 'services/rust_service.dart'
    if (dart.library.js_interop) 'services/rust_service_web.dart';
import 'services/app_service.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();

  if (!kIsWeb) {
    await RustLib.init();
  }

  runApp(const CFnatApp());
}

class CFnatApp extends StatelessWidget {
  const CFnatApp({super.key});

  @override
  Widget build(BuildContext context) {
    return ChangeNotifierProvider<AppService>(
      create: (_) => RustService()..initialize(),
      child: MaterialApp(
        title: 'CFnat',
        debugShowCheckedModeBanner: false,
        theme: ThemeData(
          colorScheme: ColorScheme.fromSeed(
            seedColor: const Color(0xFF2D7DFF),
            brightness: Brightness.dark,
          ),
          fontFamily: 'MiSans',
          scaffoldBackgroundColor: const Color(0xFF0E1117),
          cardTheme: CardThemeData(
            color: const Color(0xFF161B22),
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(12),
              side: const BorderSide(color: Color(0xFF2A2F3A)),
            ),
          ),
          dividerColor: const Color(0xFF2A2F3A),
          appBarTheme: const AppBarTheme(
            backgroundColor: Color(0xFF121721),
            surfaceTintColor: Colors.transparent,
            elevation: 0,
          ),
        ),
        themeMode: ThemeMode.dark,
        home: const MainScreen(),
      ),
    );
  }
}