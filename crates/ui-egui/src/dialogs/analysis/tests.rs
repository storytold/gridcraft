use super::*;

#[test]
fn regression_models_use_upstream_references_and_independent_options() {
    let defaults = toolpak_defaults("A1:A5", "regression");
    let fields = toolpak_fields("regression", &defaults);
    assert_eq!(defaults["model"], "linear");
    assert_eq!(defaults["inputY"], "A1:A5");
    assert!(matches!(fields.first(), Some(Field::Ref { key: "inputY", .. })));
    assert!(matches!(fields.get(1), Some(Field::Ref { key: "inputX", .. })));
    assert!(matches!(fields.get(2), Some(Field::Check { key: "labels", .. })));
    assert!(matches!(fields.get(3), Some(Field::Choice { key: "model", options, .. })
        if options.iter().map(|(id, _)| *id).collect::<Vec<_>>() == ["linear", "exponential", "logarithmic", "power"]));
    assert!(matches!(fields.get(4), Some(Field::Check { key: "constantZero", .. })));

    let mut session = gridcraft_engine::Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    app.open_dialog("analysisToolPak", json!({"tool":"regression"}));
    let d = app.dialog.as_mut().unwrap();
    d.set_value("labels", json!(true));
    d.set_value("constantZero", json!(true));
    d.set_value("model", json!("power"));
    let params = build_params(d);
    assert_eq!(params["model"], "power");
    assert_eq!(params["labels"], true);
    assert_eq!(params["constantZero"], true);
}

#[test]
fn regression_model_keeps_a_non_string_value_until_an_option_is_selected() {
    let mut defaults = toolpak_defaults("A1:A5", "regression");
    defaults["model"] = Json::Null;
    let fields = toolpak_fields("regression", &defaults);
    assert!(matches!(fields.get(3), Some(Field::Choice { key: "model", .. })));

    let mut session = gridcraft_engine::Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    app.open_dialog("analysisToolPak", json!({"tool":"regression", "model":null}));
    let d = app.dialog.as_mut().unwrap();
    d.set_value("model", Json::Null);
    assert_eq!(d.values.get("model"), Some(&Json::Null));
    assert_eq!(build_params(d)["model"], Json::Null);
}
