override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    handleIntentAction(intent)
}

override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
    super.configureFlutterEngine(flutterEngine)
    methodChannel = MethodChannel(
        flutterEngine.dartExecutor.binaryMessenger,
        "com.example.vpn/VpnChannel",
    )
}
