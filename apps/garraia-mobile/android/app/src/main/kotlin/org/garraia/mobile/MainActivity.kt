package org.garraia.mobile

import io.flutter.embedding.android.FlutterFragmentActivity

// FlutterFragmentActivity (not FlutterActivity): `local_auth` hosts its
// biometric prompt in a FragmentActivity and throws at runtime otherwise.
class MainActivity : FlutterFragmentActivity()
