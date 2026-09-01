//! VALIDATOR PROBE (not product code). Prints the containment the CLI actually
//! emits, through both JSON surfaces, so `image_integrity` is read off a real
//! report rather than inferred from the struct definition.

mod support;

use support::*;

#[test]
fn zv_the_cli_json_carries_the_image_integrity_state() {
    let prefix = Prefix::answering();
    let libapp = prefix.root().join("libapp.so");
    std::fs::write(&libapp, synthetic_libapp(HASH, FEATURES)).expect("write libapp");
    let input = libapp.to_str().expect("path").to_string();

    let install = prefix.install();
    assert_eq!(code(&install), 0, "{}", stderr(&install));

    let info = prefix.run(&["info", &input, "--json"]);
    assert_eq!(code(&info), 0, "{}", stderr(&info));
    let report = json(&info);
    println!(
        "ZV-CLI-INFO {}",
        serde_json::to_string_pretty(&report["adapter_containment"]).expect("serialize")
    );

    let out = prefix.root().join("out");
    let out_arg = out.to_str().expect("path").to_string();
    let _ = prefix.run(&[
        "decompile",
        &input,
        "-o",
        &out_arg,
        "--function-scope",
        "all",
    ]);
    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("report.json")).expect("read report"))
            .expect("report JSON");
    println!(
        "ZV-CLI-REPORT {}",
        serde_json::to_string_pretty(&summary["adapter_selection"]["provider"]["containment"])
            .expect("serialize")
    );

    for surface in [
        &report["adapter_containment"],
        &summary["adapter_selection"]["provider"]["containment"],
    ] {
        assert!(
            surface["image_integrity"].is_object(),
            "the CLI JSON does not carry image_integrity: {surface}"
        );
    }
}
