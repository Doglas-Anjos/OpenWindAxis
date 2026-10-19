//! Flag por dispersão: the steps a shape holds on the scatter of two columns,
//! flagged and unflagged as one undoable edit.
use wind_core::error::CoreError;
use wind_core::filter::FilterView;
use wind_core::history::History;
use wind_core::import::{import_text, ImportConfig};
use wind_core::model::{Dataset, Segment};
use wind_core::ops::Op;
use wind_core::region::steps_inside;

const INVALID_FLAG: usize = 0;
const ICING_FLAG: usize = 2;

/// Two anemometers that agree except at steps 2 and 5, where B reads far low.
fn pair() -> Dataset {
    let csv = "t,A,B,T\n\
               2018-01-01 00:00,5,5.1,10\n2018-01-01 00:10,6,6.0,10\n2018-01-01 00:20,7,2.0,10\n\
               2018-01-01 00:30,8,8.1,10\n2018-01-01 00:40,9,9.0,10\n2018-01-01 00:50,10,3.0,10\n\
               2018-01-01 01:00,11,11.1,10\n";
    import_text("par", csv, &ImportConfig::default()).unwrap().0
}

/// A triangle below the line y = x, where B reads low: from (4, 0) to (12, 0) to (12, 8).
fn below_the_line() -> Vec<[f64; 2]> {
    vec![[4.0, 0.0], [12.0, 0.0], [12.0, 8.0]]
}

#[test]
fn a_shape_holds_the_steps_whose_points_fall_inside() {
    let dataset = pair();
    let runs = steps_inside(&dataset, &FilterView::new(&dataset), 0, 1, &[below_the_line()]).unwrap();
    assert_eq!(runs, [2..3, 5..6], "only the two low readings of B");
    let both = steps_inside(&dataset, &FilterView::new(&dataset), 0, 1, &[below_the_line(), vec![[4.5, 4.5], [5.5, 4.5], [5.5, 5.5], [4.5, 5.5]]]).unwrap();
    assert_eq!(both, [0..1, 2..3, 5..6], "shapes add up");
    assert!(matches!(steps_inside(&dataset, &FilterView::new(&dataset), 0, 1, &[vec![[0.0, 0.0], [1.0, 1.0]]]), Err(CoreError::ShapeTooFewVertices { vertices: 2 })));
}

#[test]
fn hidden_points_are_not_selected() {
    let mut dataset = pair();
    dataset.segments = vec![Segment { column: 1, flag: INVALID_FLAG, start: 5, end: 6 }];
    let runs = steps_inside(&dataset, &FilterView::new(&dataset), 0, 1, &[below_the_line()]).unwrap();
    assert_eq!(runs, [2..3], "step 5 is excluded by its flag, so the plot does not show it");
}

#[test]
fn the_selection_is_flagged_and_unflagged_as_one_edit() {
    let mut dataset = pair();
    let mut history = History::default();
    let runs = steps_inside(&dataset, &FilterView::new(&dataset), 0, 1, &[below_the_line()]).unwrap();
    history.apply(&mut dataset, Op::FlagSteps { columns: vec![1], flag: ICING_FLAG, runs: runs.clone() }).unwrap();
    let flagged: Vec<(usize, usize)> = dataset.segments.iter().map(|segment| (segment.start, segment.end)).collect();
    assert_eq!(flagged, [(2, 3), (5, 6)]);
    history.apply(&mut dataset, Op::UnflagSteps { columns: vec![1], flag: None, runs: vec![5..6] }).unwrap();
    assert_eq!(dataset.segments.len(), 1, "step 5 cleared, step 2 kept");
    history.undo(&mut dataset).unwrap();
    history.undo(&mut dataset).unwrap();
    assert!(dataset.segments.is_empty(), "two undos take the selection back");
}
