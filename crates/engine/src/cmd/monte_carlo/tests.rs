use super::*;

/// Builds a deterministic two-input workbook so simulation results can be checked analytically.
fn model() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range":"A1","values":[[10,20]]})).unwrap();
    s.execute("cell.set", json!({"cell":"C1","input":"=3*A1-B1"})).unwrap();
    s
}
/// Supplies reusable seeded uniform/triangular assumptions for the synthetic workbook.
fn params() -> Json {
    json!({"inputs":[{"cell":"A1","distribution":"uniform","min":0,"max":100},{"cell":"B1","distribution":"triangular","min":0,"mode":10,"max":20}],"outputs":["C1"],"iterations":1000,"seed":42})
}
/// Reads a known fixture cell without depending on the report's active-sheet selection.
fn value(s: &Session, sheet: usize, cell: &str) -> Value {
    s.doc().unwrap().wb.sheet(sheet).unwrap().value(CellRef::parse(cell).unwrap())
}

/// Repeated seeds must reproduce outcomes while report undo/redo and XLSX preserve source cells.
#[test]
fn monte_carlo_reproduces_samples_preserves_model_and_undoes_report() {
    let mut s = model();
    let original = s.doc().unwrap().wb.clone();
    let first = s.execute(CMD, params()).unwrap();
    assert_eq!(first["iterations"], 1000);
    assert_eq!(first["static"], true);
    assert_eq!(value(&s, 0, "A1"), Value::Number(10.0));
    assert_eq!(value(&s, 0, "C1"), Value::Number(10.0));
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 2);
    let report = s.doc().unwrap().wb.sheet(1).unwrap();
    assert_eq!(report.charts.len(), 2);
    let json_config: Json = serde_json::from_str(report.value(CellRef::new(3, 1)).as_text().unwrap()).unwrap();
    assert_eq!(json_config["inputs"][0]["cell"], "Sheet1!A1");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);
    assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap(), original.sheet(0).unwrap());
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 2);
    s.execute("sheet.activate", json!({"sheet":0})).unwrap();
    let second = s.execute(CMD, params()).unwrap();
    assert_eq!(first["outputs"], second["outputs"]);
    assert_eq!(first["tornado"], second["tornado"]);
    assert_eq!(first["tornado"][0]["input"], "Sheet1!A1");
    assert!(first["tornado"][0]["spearman"].as_f64().unwrap() > 0.9);
    let wb = &s.doc().unwrap().wb;
    let bytes = gridcraft_xlsx::write_xlsx(wb).unwrap();
    let (reopened, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
    assert_eq!(reopened.sheets.len(), 3);
    assert_eq!(reopened.sheet(1).unwrap().charts.len(), 2);
    assert_eq!(reopened.sheet(0).unwrap().value(CellRef::new(0, 0)), Value::Number(10.0));
}

/// Isolated trials must recalculate cross-sheet outputs even when the source uses manual mode.
#[test]
fn monte_carlo_manual_mode_cross_sheet_and_multiple_outputs() {
    let mut s = model();
    s.execute("home.insertSheet", json!({})).unwrap();
    s.execute("cell.set", json!({"cell":"D1","input":"=Sheet1!A1*2"})).unwrap();
    s.execute("formulas.calculationOptions", json!({"mode":"manual"})).unwrap();
    let p =
        json!({"inputs":[{"cell":"Sheet1!A1","distribution":"uniform","min":5,"max":5}],"outputs":["Sheet1!C1","D1"],"iterations":2,"samples":false});
    let r = s.execute(CMD, p).unwrap();
    assert_eq!(r["outputs"][0]["mean"], -5.0);
    assert_eq!(r["outputs"][1]["p50"], 10.0);
    assert_eq!(r["tornado"][0]["spearman"], Json::Null);
    assert_eq!(s.doc().unwrap().wb.calc.mode, gridcraft_model::CalcMode::Manual);
}

/// Invalid addresses, assumptions and resource requests must leave the original workbook intact.
#[test]
fn monte_carlo_rejects_invalid_params_without_changes() {
    for p in [
        json!({}),
        json!({"inputs":[],"outputs":["C1"]}),
        json!({"inputs":[{"cell":"A1","min":2,"max":1}],"outputs":["C1"]}),
        json!({"inputs":[{"cell":"C1","min":0,"max":1}],"outputs":["A1"]}),
        json!({"inputs":[{"cell":"A1","min":0,"max":1},{"cell":"$A$1","min":0,"max":1}],"outputs":["C1"]}),
        json!({"inputs":[{"cell":"A1","distribution":"normal","mean":0,"stdDev":-1}],"outputs":["C1"]}),
        json!({"inputs":[{"cell":"A1","distribution":"triangular","min":0,"mode":3,"max":1}],"outputs":["C1"]}),
        json!({"inputs":[{"cell":"A1","min":0,"max":1}],"outputs":["C1"],"iterations":0}),
        json!({"inputs":[{"cell":"A1","min":0,"max":1}],"outputs":["C1"],"iterations":100001}),
        json!({"inputs":[{"cell":"A1","min":0,"max":1}],"outputs":["C1"],"seed":-1}),
        json!({"inputs":[{"cell":"A1","min":0,"max":1}],"outputs":["C1"],"samples":"false"}),
        json!({"inputs":[{"cell":"A1","min":0,"max":1}],"outputs":["C1"],"newSheet":"bad[name]"}),
        json!({"inputs":[{"cell":"XFD1048577","min":0,"max":1}],"outputs":["C1"]}),
    ] {
        let mut s = model();
        let before = s.doc().unwrap().wb.clone();
        assert!(s.execute(CMD, p.clone()).is_err(), "{p}");
        assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);
        assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap(), before.sheet(0).unwrap());
    }
    let mut s = model();
    let mut p = params();
    p["inputs"] = json!((0..33).map(|_| json!({"cell":"A1","min":0,"max":1})).collect::<Vec<_>>());
    assert!(s.execute(CMD, p).is_err());
    let mut p = params();
    p["outputs"] = json!(vec!["C1"; 65]);
    assert!(s.execute(CMD, p).is_err());
    let mut p = params();
    p["inputs"] = json!((1..=16).map(|i| json!({"cell":format!("A{i}"),"min":0,"max":1})).collect::<Vec<_>>());
    p["iterations"] = json!(100000);
    assert!(s.execute(CMD, p).is_err());
}

/// A formula error during a trial must abort before publishing any report or source mutation.
#[test]
fn monte_carlo_erroring_trial_is_atomic() {
    let mut s = model();
    s.execute("cell.set", json!({"cell":"C1","input":"=1/A1"})).unwrap();
    let r = s.execute(CMD, json!({"inputs":[{"cell":"A1","min":0,"max":0}],"outputs":["C1"]}));
    assert!(r.is_err());
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);
    assert_eq!(value(&s, 0, "A1"), Value::Number(10.0));
}

/// Checks analytic marginal moments, interpolated percentiles and average-rank tie handling.
#[test]
fn monte_carlo_distribution_moments_and_ranks() {
    let mut rng = Rng(42);
    for (d, expected_mean, expected_var) in [
        (Distribution::Uniform(0., 1.), 0.5, 1. / 12.),
        (Distribution::Triangular(0., 0.5, 1.), 0.5, 1. / 24.),
        (Distribution::Normal(5., 2.), 5., 4.),
    ] {
        let xs = (0..50000).map(|_| d.sample(&mut rng, None).unwrap()).collect::<Vec<_>>();
        let mean = xs.iter().sum::<f64>() / xs.len() as f64;
        let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64;
        assert!((mean - expected_mean).abs() < 0.03);
        assert!((var - expected_var).abs() < 0.07);
    }
    assert_eq!(ranks(&[3., 1., 1., 2.]), vec![3., 0.5, 0.5, 2.]);
    assert_eq!(quantile(&[0., 10.], 0.1), 1.);
    assert_eq!(correlation(&[0., 0.], &[1., 2.]), None);
}

/// A 24-month formula forecast must retain distinct outputs over 10,000 shared input draws.
#[test]
fn monte_carlo_ten_thousand_trials_and_monthly_outputs() {
    let mut s = model();
    for month in 1..=24 {
        s.execute("cell.set", json!({"cell":format!("C{month}"),"input":format!("=A1*{month}-B1")})).unwrap();
    }
    let mut p = params();
    p["iterations"] = json!(10000);
    p["outputs"] = json!((1..=24).map(|m| format!("C{m}")).collect::<Vec<_>>());
    p["samples"] = json!(false);
    let result = s.execute(CMD, p).unwrap();
    assert_eq!(result["outputs"].as_array().unwrap().len(), 24);
    assert!((result["outputs"][0]["mean"].as_f64().unwrap() - 40.0).abs() < 2.0);
    assert!((result["outputs"][23]["mean"].as_f64().unwrap() - 1190.0).abs() < 30.0);
}

/// Correlated standard normals must reproduce the analytic variance of their sum and their seed.
#[test]
fn monte_carlo_gaussian_correlation_matches_analytic_covariance() {
    let mut s = model();
    s.execute("cell.set", json!({"cell":"C1","input":"=A1+B1"})).unwrap();
    let p = json!({"inputs":[{"cell":"A1","distribution":"normal","mean":0,"stdDev":1},{"cell":"B1","distribution":"normal","mean":0,"stdDev":1}],"outputs":["C1"],"iterations":20000,"seed":123,"samples":false,"correlation":[[1,0.75],[0.75,1]]});
    let first = s.execute(CMD, p.clone()).unwrap();
    assert!((first["outputs"][0]["stdDev"].as_f64().unwrap() - 3.5_f64.sqrt()).abs() < 0.04);
    assert_eq!(first["independentInputs"], false);
    s.execute("sheet.activate", json!({"sheet":0})).unwrap();
    let again = s.execute(CMD, p).unwrap();
    assert_eq!(first["outputs"], again["outputs"]);
    assert_eq!(first["accuracy"], again["accuracy"]);
    assert!(first["accuracy"][0]["meanCI95"].is_array());
}

/// Matrix validation must reject invalid/indefinite inputs while accepting singular ±1 dependence.
#[test]
fn monte_carlo_rejects_invalid_correlation_matrices_and_accepts_singular() {
    for matrix in [
        json!([[1]]),
        json!([[1, 0.5], [0.3, 1]]),
        json!([[0.9, 0], [0, 1]]),
        json!([[1, 1.1], [1.1, 1]]),
        json!([[1, "bad"], ["bad", 1]]),
        json!([[1, 0, 0], [0, 1, 0]]),
    ] {
        let mut s = model();
        let mut p = params();
        p["correlation"] = matrix;
        assert!(s.execute(CMD, p).is_err());
        assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);
    }
    assert!(sampling::Plan::parse(&json!({"correlation":[[1,0.9,0.9],[0.9,1,-0.9],[0.9,-0.9,1]]}), 3, 1000).is_err());
    for rho in [-1.0, 1.0] {
        let mut s = model();
        s.execute("cell.set", json!({"cell":"C1","input":"=A1+B1"})).unwrap();
        let p = json!({"inputs":[{"cell":"A1","distribution":"normal","mean":0,"stdDev":1},{"cell":"B1","distribution":"normal","mean":0,"stdDev":1}],"outputs":["C1"],"iterations":20000,"samples":false,"correlation":[[1,rho],[rho,1]]});
        let r = s.execute(CMD, p).unwrap();
        let sd = r["outputs"][0]["stdDev"].as_f64().unwrap();
        if rho < 0.0 {
            assert!(sd < 1e-10);
        } else {
            assert!((sd - 2.0).abs() < 0.05);
        }
    }
}

/// LHS must cover every driving stratum and infer error from replicas, including degenerate tails.
#[test]
fn monte_carlo_latin_hypercubes_preserve_strata_and_use_replica_error() {
    let plan = sampling::Plan::parse(&json!({"method":"latinHypercube","replicates":8}), 1, 1024).unwrap();
    let inputs = [Input { sheet: 0, cell: CellRef::new(0, 0), label: "A1".into(), distribution: Distribution::Uniform(0., 1.) }];
    let samples = plan.sample(&inputs, 42).unwrap();
    for replica in samples[0].chunks_exact(128) {
        let mut bins = replica.iter().map(|x| (x * 128.).floor() as usize).collect::<Vec<_>>();
        bins.sort();
        assert_eq!(bins, (0..128).collect::<Vec<_>>());
    }
    let mut s = model();
    s.execute("cell.set", json!({"cell":"C1","input":"=A1"})).unwrap();
    let p = json!({"inputs":[{"cell":"A1","min":0,"max":1}],"outputs":["C1"],"iterations":1024,"method":"latinHypercube","replicates":8,"seed":42,"samples":false});
    let r = s.execute(CMD, p).unwrap();
    assert!((r["outputs"][0]["mean"].as_f64().unwrap() - 0.5).abs() < 0.001);
    assert_eq!(r["accuracy"][0]["independentUnits"], 8);
    assert!(r["accuracy"][0]["meanMCSE"].as_f64().unwrap() < 0.001);
    assert!(r["accuracy"][0]["probabilityCI95"].is_null(), "a constant replica proportion is not a rare-event confidence guarantee");
    assert_eq!(r["convergence"].as_array().unwrap().last().unwrap()["samples"], 1024);
    for p in [
        json!({"method":"latinHypercube","replicates":1}),
        json!({"method":"latinHypercube","replicates":7}),
        json!({"replicates":0}),
        json!({"method":"bad"}),
    ] {
        assert!(sampling::Plan::parse(&p, 1, 1000).is_err());
    }
}

/// Compensated/scaled statistics must retain small offsets and mixed-sign cancellation residuals.
#[test]
fn monte_carlo_stable_moments_handle_large_offsets_and_extreme_ranges() {
    for values in [[1e16, -1e16, 1.0], [1.0, 1e16, -1e16], [-1e16, 1.0, 1e16]] {
        let (mean, _) = statistics::moments(&values).unwrap();
        assert!((mean - 1.0 / 3.0).abs() < 1e-15);
    }
    let (m, sd) = statistics::moments(&[1e15 + 1., 1e15 + 2., 1e15 + 3.]).unwrap();
    assert_eq!(m, 1e15 + 2.);
    assert!((sd - 1.).abs() < 1e-12);
    let (m, sd) = statistics::moments(&[-1e308, 1e308]).unwrap();
    assert_eq!(m, 0.);
    assert!((sd / 1e308 - 2_f64.sqrt()).abs() < 1e-14);
    let correlation = statistics::correl(&[1e15 + 1., 1e15 + 2., 1e15 + 3.], &[3., 2., 1.]).unwrap();
    assert!((correlation + 1.).abs() < 1e-12);
}

/// Combining LHS and a Gaussian copula must retain the expected covariance and seeded draws.
#[test]
fn monte_carlo_correlated_lhs_matches_normal_sum_variance_and_preserves_seed() {
    let inputs = [
        Input { sheet: 0, cell: CellRef::new(0, 0), label: "A1".into(), distribution: Distribution::Normal(0., 1.) },
        Input { sheet: 0, cell: CellRef::new(0, 1), label: "B1".into(), distribution: Distribution::Normal(0., 1.) },
    ];
    let p = json!({"method":"latinHypercube","replicates":8,"correlation":[[1,0.75],[0.75,1]]});
    let plan = sampling::Plan::parse(&p, 2, 8192).unwrap();
    let x = plan.sample(&inputs, 123).unwrap();
    let again = plan.sample(&inputs, 123).unwrap();
    assert_eq!(x, again);
    let correlation = statistics::correl(&x[0], &x[1]).unwrap();
    assert!((correlation - 0.75).abs() < 0.03);
    let total = x[0].iter().zip(&x[1]).map(|(a, b)| a + b).collect::<Vec<_>>();
    let (mean, sd) = statistics::moments(&total).unwrap();
    assert!(mean.abs() < 0.01);
    assert!((sd - 3.5_f64.sqrt()).abs() < 0.03);
}

/// Noise-only cells must enter the dirty closure even without a link to sampled assumptions.
/// Publication recalculates the ordinary guarded forecast and undo removes only the report.
#[test]
fn native_stochastic_formulas_recalculate_and_preserve_the_forecast() {
    let mut s = model();
    let formulas = [
        "=IF(MC.ACTIVE(),MC.POISSON(12,\"leads\"),12)",
        "=IF(MC.ACTIVE(),MC.BINOMIAL(C1,0.4,\"sales\"),C1*0.4)",
        "=IF(MC.ACTIVE(),MC.AR1.LOGNORMAL(\"cpl\",24,1,0.1,0.6),1)",
        "=C1+D1+E1",
    ];
    for (column, input) in ["C1", "D1", "E1", "F1"].into_iter().zip(formulas) {
        s.execute("cell.set", json!({"cell":column,"input":input})).unwrap();
    }
    let before = s.doc().unwrap().wb.sheet(0).unwrap().clone();
    let mut p = params();
    p["inputs"] = json!([{"cell":"A1","min":10,"max":10}]);
    p["outputs"] = json!(["C1", "D1", "E1", "F1"]);
    p["iterations"] = json!(512);
    let r = s.execute(CMD, p).unwrap();
    assert!((r["outputs"][0]["mean"].as_f64().unwrap() - 12.0).abs() < 0.7);
    assert!(r["outputs"][0]["stdDev"].as_f64().unwrap() > 2.5);
    assert!(r["outputs"][1]["stdDev"].as_f64().unwrap() > 1.0);
    assert!(r["outputs"][2]["stdDev"].as_f64().unwrap() > 0.07);
    assert!(r["stochastic"]["countRequests"].as_u64().unwrap() >= 1024);
    assert!(r["stochastic"]["ar1GeneratedSteps"].as_u64().unwrap() >= 512 * 24);
    assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap(), &before);
    assert!(gridcraft_calc::SimulationHook::current().is_none());
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);
    assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap(), &before);
}

/// Handled sampler errors and invalid arity cannot become apparently successful trial outputs.
#[test]
fn native_stochastic_invalid_requests_abort_atomically_despite_iferror() {
    for formula in [
        "=IF(MC.ACTIVE(),IFERROR(MC.BINOMIAL(2.5,0.5,\"s\"),7),1)",
        "=IF(MC.ACTIVE(),IFERROR(MC.POISSON(1),7),1)",
        "=IF(MC.ACTIVE(),IFERROR(MC.AR1.NORMAL(\"p\",1,0,1,1),7),1)",
        "=IF(MC.ACTIVE(),IFERROR(MC.POISSON({1,2},\"s\"),7),1)",
    ] {
        let s = model();
        // Imported formulas may bypass the interactive editor's strict arity validation.
        let mut wb = (*s.doc().unwrap().wb).clone();
        wb.sheet_mut(0).unwrap().set_cell(CellRef::new(0, 2), Cell::formula(gridcraft_model::Formula::new(formula)));
        let mut s = Session::new();
        s.add_document(crate::DocState::new(wb, None, "Imported fixture".into(), crate::Trust::FromFile));
        let before = s.doc().unwrap().wb.sheet(0).unwrap().clone();
        let mut p = params();
        p["iterations"] = json!(2);
        assert!(s.execute(CMD, p).is_err(), "{formula}");
        assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);
        assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap(), &before);
        assert!(gridcraft_calc::SimulationHook::current().is_none());
    }
}

/// Absolute trial keys make stepped runs identical to synchronous runs, including AR1 paths.
#[test]
fn native_stochastic_sync_and_incremental_outputs_match() {
    fn setup() -> Session {
        let mut s = model();
        s.execute("cell.set", json!({"cell":"C1","input":"=IF(MC.ACTIVE(),MC.POISSON(A1,\"n\")+MC.AR1.NORMAL(\"p\",12,0,1,-0.4),A1)"})).unwrap();
        s
    }
    let mut p = params();
    p["iterations"] = json!(64);
    p["inputs"] = json!([{"cell":"Sheet1!A1","min":5,"max":15}]);
    let expected = setup().execute(CMD, p.clone()).unwrap();
    let mut s = setup();
    let mut job = Job::prepare(&s, &p).unwrap();
    while !job.ready() {
        job.advance(1).unwrap();
    }
    let actual = job.publish(&mut s).unwrap();
    for key in ["outputs", "tornado", "stochastic", "accuracy", "convergence"] {
        assert_eq!(actual[key], expected[key], "{key}");
    }
}

/// New static marginals use the same bounded plan/correlation path and retain integral support.
#[test]
fn native_discrete_and_lognormal_marginals_are_reusable_and_validated() {
    for (distribution, low, high) in [
        (json!({"distribution":"poisson","mean":8}), 0.0, 100.0),
        (json!({"distribution":"binomial","trials":20,"probability":0.4}), 0.0, 20.0),
        (json!({"distribution":"hypergeometric","draws":10,"successes":15,"population":40}), 0.0, 10.0),
        (json!({"distribution":"lognormal","mean":1,"cv":0.2}), 0.0, 10.0),
    ] {
        let mut s = model();
        s.execute("cell.set", json!({"cell":"C1","input":"=A1"})).unwrap();
        let mut input = distribution.clone();
        input["cell"] = json!("A1");
        let p = json!({"inputs":[input],"outputs":["C1"],"iterations":128,"seed":42,"method":"latinHypercube","replicates":8,"correlation":[[1]]});
        let first = s.execute(CMD, p.clone()).unwrap();
        assert!(first["outputs"][0]["min"].as_f64().unwrap() >= low);
        assert!(first["outputs"][0]["max"].as_f64().unwrap() <= high);
        s.execute("sheet.activate", json!({"sheet":0})).unwrap();
        let again = s.execute(CMD, p).unwrap();
        assert_eq!(first["outputs"], again["outputs"]);
        assert_eq!(first["config"]["stochasticVersion"], 1);
    }
    for distribution in [
        json!({"distribution":"poisson","mean":50001}),
        json!({"distribution":"binomial","trials":2.5,"probability":0.5}),
        json!({"distribution":"hypergeometric","draws":11,"successes":2,"population":10}),
        json!({"distribution":"lognormal","mean":0,"cv":0.1}),
    ] {
        assert!(Distribution::parse(&distribution).is_err());
    }
}
