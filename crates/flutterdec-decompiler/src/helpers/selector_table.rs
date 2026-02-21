fn classify_standard_selector(raw: &str) -> Option<String> {
    let flutter = [
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
        (
            "keyedsubtree",
            "framework:flutter.widgets.KeyedSubtree.new",
        ),
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
            "didhavememorypressure",
            "framework:flutter.widgets.WidgetsBindingObserver.didHaveMemoryPressure",
        ),
        (
            "addobserver",
            "framework:flutter.widgets.WidgetsBinding.addObserver",
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
        ("pushnamed", "framework:flutter.widgets.Navigator.pushNamed"),
        ("popuntil", "framework:flutter.widgets.Navigator.popUntil"),
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
    let dart_async = [
        ("then", "stdlib:dart.async.Future.then"),
        ("catcherror", "stdlib:dart.async.Future.catchError"),
        ("whencomplete", "stdlib:dart.async.Future.whenComplete"),
        ("listen", "stdlib:dart.async.Stream.listen"),
        ("streamiterator", "stdlib:dart.async.StreamIterator.new"),
        ("wait", "stdlib:dart.async.Future.wait"),
        ("delayed", "stdlib:dart.async.Future.delayed"),
        ("timeout", "stdlib:dart.async.Future.timeout"),
        ("asstream", "stdlib:dart.async.Future.asStream"),
        ("transform", "stdlib:dart.async.Stream.transform"),
        ("distinct", "stdlib:dart.async.Stream.distinct"),
        ("takewhile", "stdlib:dart.async.Stream.takeWhile"),
        ("skipwhile", "stdlib:dart.async.Stream.skipWhile"),
    ];
    let dart_core = [
        ("print", "stdlib:dart.core.print"),
        ("compiletimeerror", "stdlib:dart.core._CompileTimeError.new"),
        ("tostring", "stdlib:dart.core.toString"),
        ("hashcode", "stdlib:dart.core.hashCode"),
        ("compareto", "stdlib:dart.core.compareTo"),
        ("contains", "stdlib:dart.core.contains"),
        ("containskey", "stdlib:dart.core.Map.containsKey"),
        ("putifabsent", "stdlib:dart.core.Map.putIfAbsent"),
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
    let dart_io = [
        ("supportsansiescapes", "stdlib:dart.io.Stdout.supportsAnsiEscapes"),
        ("websocketimpl", "stdlib:dart.io.WebSocketImpl.new"),
        ("nativesocket", "stdlib:dart.io._NativeSocket.new"),
    ];
    let dart_vm_runtime = [
        ("yieldstariterable", "runtime:dart_vm.yieldStarIterable"),
        ("closure", "runtime:dart_vm.Closure.new"),
        ("typeparameter", "runtime:dart_vm.TypeParameter.new"),
    ];
    let dart_typed_data = [
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
        ("setfloat64", "stdlib:dart.typed_data.ByteData.setFloat64"),
        ("setint32", "stdlib:dart.typed_data.ByteData.setInt32"),
        ("setuint32", "stdlib:dart.typed_data.ByteData.setUint32"),
        ("getfloat32", "stdlib:dart.typed_data.ByteData.getFloat32"),
        ("getfloat64", "stdlib:dart.typed_data.ByteData.getFloat64"),
        ("getint32", "stdlib:dart.typed_data.ByteData.getInt32"),
        ("getuint32", "stdlib:dart.typed_data.ByteData.getUint32"),
    ];

    for candidate in selector_candidates(raw) {
        for (needle, tag) in flutter {
            if candidate == needle {
                return Some(format!("{} [selector]", tag));
            }
        }
        for (needle, tag) in dart_async {
            if candidate == needle {
                return Some(format!("{} [selector]", tag));
            }
        }
        for (needle, tag) in dart_core {
            if candidate == needle {
                return Some(format!("{} [selector]", tag));
            }
        }
        for (needle, tag) in dart_io {
            if candidate == needle {
                return Some(format!("{} [selector]", tag));
            }
        }
        for (needle, tag) in dart_vm_runtime {
            if candidate == needle {
                return Some(format!("{} [selector]", tag));
            }
        }
        for (needle, tag) in dart_typed_data {
            if candidate == needle {
                return Some(format!("{} [selector]", tag));
            }
        }
    }

    None
}

fn selector_candidates(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let normalized = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    push_unique(&mut out, normalized);

    let mut token = String::new();
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            token.push(c.to_ascii_lowercase());
        } else if !token.is_empty() {
            push_unique(&mut out, token.clone());
            token.clear();
        }
    }
    if !token.is_empty() {
        push_unique(&mut out, token);
    }

    let derived = out.clone();
    for t in derived {
        if let Some(rest) = t.strip_prefix("init") {
            push_unique(&mut out, rest.to_string());
        }
        if let Some(rest) = t.strip_prefix("get") {
            push_unique(&mut out, rest.to_string());
        }
        if let Some(rest) = t.strip_prefix("set") {
            push_unique(&mut out, format!("set{}", rest));
            push_unique(&mut out, rest.to_string());
        }
        if let Some(rest) = t.strip_prefix("native") {
            push_unique(&mut out, rest.to_string());
        }
        if let Some(rest) = t.strip_prefix('_') {
            push_unique(&mut out, rest.to_string());
        }
    }

    out
}

fn push_unique(out: &mut Vec<String>, s: String) {
    if !s.is_empty() && !out.contains(&s) {
        out.push(s);
    }
}
