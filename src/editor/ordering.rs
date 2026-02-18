pub fn midpoint_or_append(current_order: f32, next_order: Option<f32>) -> f32 {
    if let Some(no) = next_order {
        (current_order + no) / 2.0
    } else {
        current_order + 1.0
    }
}
