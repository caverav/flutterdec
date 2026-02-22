fn classify_standard_selector(raw: &str) -> Option<String> {
    for candidate in selector_candidates(raw) {
        if let Some(tag) = match_selector_candidate(&candidate) {
            return Some(format!("{} [selector]", tag));
        }
    }
    None
}

fn match_selector_candidate(candidate: &str) -> Option<&'static str> {
    for table in [
        FLUTTER_SELECTORS,
        DART_ASYNC_SELECTORS,
        DART_CORE_SELECTORS,
        DART_IO_SELECTORS,
        DART_VM_RUNTIME_SELECTORS,
        DART_TYPED_DATA_SELECTORS,
    ] {
        if let Some(tag) = lookup_selector(candidate, table) {
            return Some(tag);
        }
    }
    None
}

fn lookup_selector<'a>(candidate: &str, table: &'a [(&str, &'a str)]) -> Option<&'a str> {
    for (needle, tag) in table {
        if candidate == *needle {
            return Some(*tag);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::classify_standard_selector;

    #[test]
    fn classifies_flutter_observer_route_selectors() {
        assert_eq!(
            classify_standard_selector("didPushRouteInformation"),
            Some(
                "framework:flutter.widgets.WidgetsBindingObserver.didPushRouteInformation [selector]"
                    .to_string()
            )
        );
        assert_eq!(
            classify_standard_selector("handleCommitBackGesture"),
            Some(
                "framework:flutter.widgets.WidgetsBindingObserver.handleCommitBackGesture [selector]"
                    .to_string()
            )
        );
    }

    #[test]
    fn classifies_flutter_scheduler_and_navigator_selectors() {
        assert_eq!(
            classify_standard_selector("scheduleWarmUpFrame"),
            Some(
                "framework:flutter.scheduler.SchedulerBinding.scheduleWarmUpFrame [selector]"
                    .to_string()
            )
        );
        assert_eq!(
            classify_standard_selector("restorablePushNamed"),
            Some("framework:flutter.widgets.Navigator.restorablePushNamed [selector]".to_string())
        );
    }

    #[test]
    fn classifies_dart_async_and_core_selectors() {
        assert_eq!(
            classify_standard_selector("scheduleMicrotask"),
            Some("stdlib:dart.async.scheduleMicrotask [selector]".to_string())
        );
        assert_eq!(
            classify_standard_selector("runtimeType"),
            Some("stdlib:dart.core.Object.runtimeType [selector]".to_string())
        );
    }

    #[test]
    fn classifies_dart_typed_data_native_prefixed_selectors() {
        assert_eq!(
            classify_standard_selector("_nativeSetInt64"),
            Some("stdlib:dart.typed_data.ByteData.setInt64 [selector]".to_string())
        );
        assert_eq!(
            classify_standard_selector("_nativeGetFloat64x2"),
            Some("stdlib:dart.typed_data.ByteData.getFloat64x2 [selector]".to_string())
        );
    }

    #[test]
    fn does_not_classify_unknown_selectors() {
        assert_eq!(classify_standard_selector("veryCustomProjectSelector"), None);
    }
}
