override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
    super.configureFlutterEngine(flutterEngine)
    methodChannel = MethodChannel(
        flutterEngine.dartExecutor.binaryMessenger,
        "com.example.vpn/VpnChannel",
    )
    methodChannel.setMethodCallHandler { call, result ->
        when (call.method) {
            "prepare" -> handlePrepare(result)
            "getInstalledApps" -> result.success(fetchInstalledApps())
            "elapsedRealtime" -> result.success(SystemClock.elapsedRealtime())
            else -> result.notImplemented()
        }
    }
}
