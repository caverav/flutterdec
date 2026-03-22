import 'package:flutter/services.dart';

class InstalledAppsService {
  InstalledAppsService({MethodChannel? channel})
      : _channel = channel ?? const MethodChannel('com.example.vpn/VpnChannel');

  final MethodChannel _channel;

  Future<List<InstalledAppInfo>> fetchInstalledApps() async {
    final result = await _channel.invokeMethod<List<dynamic>>('getInstalledApps');
    if (result == null) {
      return const [];
    }
    return result
        .map((e) => Map<String, dynamic>.from(e as Map))
        .map(
          (e) => InstalledAppInfo(
            packageName: e['package'] as String,
            appName: e['name'] as String,
          ),
        )
        .toList();
  }
}
