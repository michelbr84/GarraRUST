import 'package:flutter/material.dart';

class SplashScreen extends StatelessWidget {
  const SplashScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return const Scaffold(
      body: Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            _GarraLogo(),
            SizedBox(height: 24),
            CircularProgressIndicator(),
          ],
        ),
      ),
    );
  }
}

class _GarraLogo extends StatelessWidget {
  const _GarraLogo();

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        // Placeholder — replace with Rive mascot when .riv is available
        Container(
          width: 96,
          height: 96,
          decoration: BoxDecoration(
            color: Theme.of(context).colorScheme.primaryContainer,
            shape: BoxShape.circle,
          ),
          child: const Icon(Icons.smart_toy_rounded, size: 56),
        ),
        const SizedBox(height: 16),
        Text(
          'Garra',
          style: Theme.of(context).textTheme.headlineLarge?.copyWith(
                fontWeight: FontWeight.bold,
                letterSpacing: 2,
              ),
        ),
      ],
    );
  }
}
