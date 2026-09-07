import 'package:flutter/material.dart';

import '../theme/garra_theme.dart';
import '../theme/garra_tokens.dart';
import '../widgets/brand/wolf_mark.dart';

class SplashScreen extends StatelessWidget {
  const SplashScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: GarraColors.bg,
      body: Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const WolfMark(size: 112),
            const SizedBox(height: 18),
            Text(
              'Garra',
              style: garraText(
                size: 30,
                weight: FontWeight.w800,
                letterSpacing: 2,
              ),
            ),
            const SizedBox(height: 24),
            const SizedBox(
              width: 22,
              height: 22,
              child: CircularProgressIndicator(strokeWidth: 2),
            ),
          ],
        ),
      ),
    );
  }
}
