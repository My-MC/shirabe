use shirabe_ui::vu_dial::deflection;

#[test]
fn zero_vu_is_at_70_8_percent_of_the_sweep() {
    assert!((deflection(0.0) - 0.70795).abs() < 0.0001);
}

#[test]
fn needle_can_rest_below_the_first_label() {
    assert_eq!(deflection(f32::NEG_INFINITY), 0.0);
    assert!(deflection(-30.0) < deflection(-20.0));
}

#[test]
fn needle_stops_at_full_scale_and_handles_invalid_readings() {
    assert_eq!(deflection(3.0), 1.0);
    assert_eq!(deflection(12.0), 1.0);
    assert_eq!(deflection(f32::NAN), 0.0);
}
