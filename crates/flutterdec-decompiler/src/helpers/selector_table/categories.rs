const FLUTTER_SELECTORS: &[(&str, &str)] = &[
    ("setstate", "framework:flutter.widgets.State.setState"),
    (
        "createstate",
        "framework:flutter.widgets.StatefulWidget.createState",
    ),
    ("build", "framework:flutter.widgets.Widget.build"),
    ("initstate", "framework:flutter.widgets.State.initState"),
    ("dispose", "framework:flutter.widgets.State.dispose"),
    ("activate", "framework:flutter.widgets.State.activate"),
    ("deactivate", "framework:flutter.widgets.State.deactivate"),
    ("reassemble", "framework:flutter.widgets.State.reassemble"),
    (
        "didupdatewidget",
        "framework:flutter.widgets.State.didUpdateWidget",
    ),
    (
        "didchangedependencies",
        "framework:flutter.widgets.State.didChangeDependencies",
    ),
    (
        "addlistener",
        "framework:flutter.foundation.ChangeNotifier.addListener",
    ),
    (
        "removelistener",
        "framework:flutter.foundation.ChangeNotifier.removeListener",
    ),
    (
        "notifylisteners",
        "framework:flutter.foundation.ChangeNotifier.notifyListeners",
    ),
    (
        "addpostframecallback",
        "framework:flutter.scheduler.SchedulerBinding.addPostFrameCallback",
    ),
    (
        "markneedsbuild",
        "framework:flutter.widgets.Element.markNeedsBuild",
    ),
    (
        "findrenderobject",
        "framework:flutter.widgets.BuildContext.findRenderObject",
    ),
    (
        "createrenderobject",
        "framework:flutter.rendering.RenderObjectWidget.createRenderObject",
    ),
    (
        "updaterenderobject",
        "framework:flutter.rendering.RenderObjectWidget.updateRenderObject",
    ),
    ("keyedsubtree", "framework:flutter.widgets.KeyedSubtree.new"),
    (
        "parentdatawidget",
        "framework:flutter.widgets.ParentDataWidget.new",
    ),
    (
        "slivergridparentdata",
        "framework:flutter.rendering.SliverGridParentData.new",
    ),
    (
        "didchangeapplifecyclestate",
        "framework:flutter.widgets.WidgetsBindingObserver.didChangeAppLifecycleState",
    ),
    (
        "didchangemetrics",
        "framework:flutter.widgets.WidgetsBindingObserver.didChangeMetrics",
    ),
    (
        "didchangelocales",
        "framework:flutter.widgets.WidgetsBindingObserver.didChangeLocales",
    ),
    (
        "didchangeplatformbrightness",
        "framework:flutter.widgets.WidgetsBindingObserver.didChangePlatformBrightness",
    ),
    (
        "didchangetextscalefactor",
        "framework:flutter.widgets.WidgetsBindingObserver.didChangeTextScaleFactor",
    ),
    (
        "didchangeaccessibilityfeatures",
        "framework:flutter.widgets.WidgetsBindingObserver.didChangeAccessibilityFeatures",
    ),
    (
        "didpushroute",
        "framework:flutter.widgets.WidgetsBindingObserver.didPushRoute",
    ),
    (
        "didpoproute",
        "framework:flutter.widgets.WidgetsBindingObserver.didPopRoute",
    ),
    (
        "didpushrouteinformation",
        "framework:flutter.widgets.WidgetsBindingObserver.didPushRouteInformation",
    ),
    (
        "didrequestappexit",
        "framework:flutter.widgets.WidgetsBindingObserver.didRequestAppExit",
    ),
    (
        "didchangeviewfocus",
        "framework:flutter.widgets.WidgetsBindingObserver.didChangeViewFocus",
    ),
    (
        "handlestartbackgesture",
        "framework:flutter.widgets.WidgetsBindingObserver.handleStartBackGesture",
    ),
    (
        "handleupdatebackgestureprogress",
        "framework:flutter.widgets.WidgetsBindingObserver.handleUpdateBackGestureProgress",
    ),
    (
        "handlecommitbackgesture",
        "framework:flutter.widgets.WidgetsBindingObserver.handleCommitBackGesture",
    ),
    (
        "handlecancelbackgesture",
        "framework:flutter.widgets.WidgetsBindingObserver.handleCancelBackGesture",
    ),
    (
        "didhavememorypressure",
        "framework:flutter.widgets.WidgetsBindingObserver.didHaveMemoryPressure",
    ),
    (
        "addobserver",
        "framework:flutter.widgets.WidgetsBinding.addObserver",
    ),
    (
        "addpersistentframecallback",
        "framework:flutter.scheduler.SchedulerBinding.addPersistentFrameCallback",
    ),
    (
        "addtimingscallback",
        "framework:flutter.scheduler.SchedulerBinding.addTimingsCallback",
    ),
    (
        "removetimingscallback",
        "framework:flutter.scheduler.SchedulerBinding.removeTimingsCallback",
    ),
    (
        "scheduleframe",
        "framework:flutter.scheduler.SchedulerBinding.scheduleFrame",
    ),
    (
        "schedulewarmupframe",
        "framework:flutter.scheduler.SchedulerBinding.scheduleWarmUpFrame",
    ),
    (
        "ensurevisualupdate",
        "framework:flutter.scheduler.SchedulerBinding.ensureVisualUpdate",
    ),
    (
        "removeobserver",
        "framework:flutter.widgets.WidgetsBinding.removeObserver",
    ),
    (
        "pushnamedandremoveuntil",
        "framework:flutter.widgets.Navigator.pushNamedAndRemoveUntil",
    ),
    (
        "pushreplacementnamed",
        "framework:flutter.widgets.Navigator.pushReplacementNamed",
    ),
    (
        "pushreplacement",
        "framework:flutter.widgets.Navigator.pushReplacement",
    ),
    ("pushnamed", "framework:flutter.widgets.Navigator.pushNamed"),
    (
        "pushandremoveuntil",
        "framework:flutter.widgets.Navigator.pushAndRemoveUntil",
    ),
    (
        "popandpushnamed",
        "framework:flutter.widgets.Navigator.popAndPushNamed",
    ),
    ("maybepop", "framework:flutter.widgets.Navigator.maybePop"),
    ("popuntil", "framework:flutter.widgets.Navigator.popUntil"),
    (
        "restorablepush",
        "framework:flutter.widgets.Navigator.restorablePush",
    ),
    (
        "restorablepushnamed",
        "framework:flutter.widgets.Navigator.restorablePushNamed",
    ),
    ("push", "framework:flutter.widgets.Navigator.push"),
    ("pop", "framework:flutter.widgets.Navigator.pop"),
    (
        "showsnackbar",
        "framework:flutter.material.ScaffoldMessengerState.showSnackBar",
    ),
    (
        "hidecurrentsnackbar",
        "framework:flutter.material.ScaffoldMessengerState.hideCurrentSnackBar",
    ),
    (
        "removecurrentsnackbar",
        "framework:flutter.material.ScaffoldMessengerState.removeCurrentSnackBar",
    ),
];

const DART_ASYNC_SELECTORS: &[(&str, &str)] = &[
    ("then", "stdlib:dart.async.Future.then"),
    ("catcherror", "stdlib:dart.async.Future.catchError"),
    ("whencomplete", "stdlib:dart.async.Future.whenComplete"),
    ("listen", "stdlib:dart.async.Stream.listen"),
    ("streamiterator", "stdlib:dart.async.StreamIterator.new"),
    ("wait", "stdlib:dart.async.Future.wait"),
    ("delayed", "stdlib:dart.async.Future.delayed"),
    ("timeout", "stdlib:dart.async.Future.timeout"),
    ("asstream", "stdlib:dart.async.Future.asStream"),
    ("futurevalue", "stdlib:dart.async.Future.value"),
    ("futureerror", "stdlib:dart.async.Future.error"),
    ("futuresync", "stdlib:dart.async.Future.sync"),
    ("futuremicrotask", "stdlib:dart.async.Future.microtask"),
    ("completer", "stdlib:dart.async.Completer.new"),
    ("timer", "stdlib:dart.async.Timer.new"),
    ("periodic", "stdlib:dart.async.Timer.periodic"),
    ("schedulemicrotask", "stdlib:dart.async.scheduleMicrotask"),
    ("streamcontroller", "stdlib:dart.async.StreamController.new"),
    ("transform", "stdlib:dart.async.Stream.transform"),
    ("distinct", "stdlib:dart.async.Stream.distinct"),
    ("takewhile", "stdlib:dart.async.Stream.takeWhile"),
    ("skipwhile", "stdlib:dart.async.Stream.skipWhile"),
];

const DART_CORE_SELECTORS: &[(&str, &str)] = &[
    ("print", "stdlib:dart.core.print"),
    ("compiletimeerror", "stdlib:dart.core._CompileTimeError.new"),
    ("tostring", "stdlib:dart.core.toString"),
    ("hashcode", "stdlib:dart.core.hashCode"),
    ("compareto", "stdlib:dart.core.compareTo"),
    ("contains", "stdlib:dart.core.contains"),
    ("containskey", "stdlib:dart.core.Map.containsKey"),
    ("putifabsent", "stdlib:dart.core.Map.putIfAbsent"),
    ("runtimetype", "stdlib:dart.core.Object.runtimeType"),
    ("nosuchmethod", "stdlib:dart.core.Object.noSuchMethod"),
    ("isempty", "stdlib:dart.core.isEmpty"),
    ("isnotempty", "stdlib:dart.core.isNotEmpty"),
    ("wheretype", "stdlib:dart.core.Iterable.whereType"),
    ("expand", "stdlib:dart.core.Iterable.expand"),
    ("fold", "stdlib:dart.core.Iterable.fold"),
    ("reduce", "stdlib:dart.core.Iterable.reduce"),
    ("firstwhere", "stdlib:dart.core.Iterable.firstWhere"),
    ("singlewhere", "stdlib:dart.core.Iterable.singleWhere"),
    ("map", "stdlib:dart.core.map"),
    ("where", "stdlib:dart.core.where"),
    ("join", "stdlib:dart.core.String.join"),
    ("split", "stdlib:dart.core.String.split"),
    ("substring", "stdlib:dart.core.String.substring"),
    ("startswith", "stdlib:dart.core.String.startsWith"),
    ("endswith", "stdlib:dart.core.String.endsWith"),
    ("replaceall", "stdlib:dart.core.String.replaceAll"),
    ("tolowercase", "stdlib:dart.core.String.toLowerCase"),
    ("touppercase", "stdlib:dart.core.String.toUpperCase"),
    ("removeat", "stdlib:dart.core.List.removeAt"),
    ("removewhere", "stdlib:dart.core.List.removeWhere"),
    ("addall", "stdlib:dart.core.List.addAll"),
    ("putall", "stdlib:dart.core.Map.addAll"),
    ("tolist", "stdlib:dart.core.Iterable.toList"),
    ("toset", "stdlib:dart.core.Iterable.toSet"),
    ("foreach", "stdlib:dart.core.Iterable.forEach"),
    ("indexof", "stdlib:dart.core.String.indexOf"),
    ("lastindexof", "stdlib:dart.core.String.lastIndexOf"),
    ("trimleft", "stdlib:dart.core.String.trimLeft"),
    ("trimright", "stdlib:dart.core.String.trimRight"),
    ("trim", "stdlib:dart.core.String.trim"),
    ("codeunitat", "stdlib:dart.core.String.codeUnitAt"),
    ("matchendindex", "stdlib:dart.core.Match.end"),
];

const DART_IO_SELECTORS: &[(&str, &str)] = &[
    ("supportsansiescapes", "stdlib:dart.io.Stdout.supportsAnsiEscapes"),
    ("websocketimpl", "stdlib:dart.io.WebSocketImpl.new"),
    ("nativesocket", "stdlib:dart.io._NativeSocket.new"),
];

const DART_VM_RUNTIME_SELECTORS: &[(&str, &str)] = &[
    ("yieldstariterable", "runtime:dart_vm.yieldStarIterable"),
    ("closure", "runtime:dart_vm.Closure.new"),
    ("typeparameter", "runtime:dart_vm.TypeParameter.new"),
    ("prependtypearguments", "runtime:dart_vm.prependTypeArguments"),
];

const DART_TYPED_DATA_SELECTORS: &[(&str, &str)] = &[
    ("float32x4list", "stdlib:dart.typed_data.Float32x4List.new"),
    ("int64list", "stdlib:dart.typed_data.Int64List.new"),
    ("offsetinbytes", "stdlib:dart.typed_data.TypedData.offsetInBytes"),
    (
        "lengthinbytes",
        "stdlib:dart.typed_data.TypedData.lengthInBytes",
    ),
    (
        "elementsizeinbytes",
        "stdlib:dart.typed_data.TypedData.elementSizeInBytes",
    ),
    ("setfloat32", "stdlib:dart.typed_data.ByteData.setFloat32"),
    ("setfloat32x4", "stdlib:dart.typed_data.ByteData.setFloat32x4"),
    ("setfloat64", "stdlib:dart.typed_data.ByteData.setFloat64"),
    ("setfloat64x2", "stdlib:dart.typed_data.ByteData.setFloat64x2"),
    ("setint8", "stdlib:dart.typed_data.ByteData.setInt8"),
    ("setuint8", "stdlib:dart.typed_data.ByteData.setUint8"),
    ("setint16", "stdlib:dart.typed_data.ByteData.setInt16"),
    ("setuint16", "stdlib:dart.typed_data.ByteData.setUint16"),
    ("setint32", "stdlib:dart.typed_data.ByteData.setInt32"),
    ("setuint32", "stdlib:dart.typed_data.ByteData.setUint32"),
    ("setint64", "stdlib:dart.typed_data.ByteData.setInt64"),
    ("setuint64", "stdlib:dart.typed_data.ByteData.setUint64"),
    ("getint8", "stdlib:dart.typed_data.ByteData.getInt8"),
    ("getuint8", "stdlib:dart.typed_data.ByteData.getUint8"),
    ("getint16", "stdlib:dart.typed_data.ByteData.getInt16"),
    ("getuint16", "stdlib:dart.typed_data.ByteData.getUint16"),
    ("getfloat32", "stdlib:dart.typed_data.ByteData.getFloat32"),
    ("getfloat32x4", "stdlib:dart.typed_data.ByteData.getFloat32x4"),
    ("getfloat64", "stdlib:dart.typed_data.ByteData.getFloat64"),
    ("getfloat64x2", "stdlib:dart.typed_data.ByteData.getFloat64x2"),
    ("getint32", "stdlib:dart.typed_data.ByteData.getInt32"),
    ("getuint32", "stdlib:dart.typed_data.ByteData.getUint32"),
    ("getint64", "stdlib:dart.typed_data.ByteData.getInt64"),
    ("getuint64", "stdlib:dart.typed_data.ByteData.getUint64"),
    (
        "unmodifiableuint8arrayview",
        "stdlib:dart.typed_data._UnmodifiableUint8ArrayView.new",
    ),
    ("int32arrayview", "stdlib:dart.typed_data._Int32ArrayView.new"),
];
