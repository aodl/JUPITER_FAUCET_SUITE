/// Returns true when a persistence batch is active for the supplied nesting depth.
pub fn is_active(depth: u32) -> bool {
    depth > 0
}

/// Increments a persistence-batch nesting depth without panicking on saturation.
pub fn begin_depth(depth: u32) -> u32 {
    depth.saturating_add(1)
}

/// Decrements a persistence-batch nesting depth and reports whether the outer
/// guard just closed over dirty state.
pub fn finish_depth(depth: u32, dirty: bool) -> (u32, bool) {
    assert!(depth > 0, "persistence batch depth underflow");
    let next_depth = depth - 1;
    (next_depth, next_depth == 0 && dirty)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_reflects_nonzero_depth() {
        assert!(!is_active(0));
        assert!(is_active(1));
    }

    #[test]
    fn finish_only_flushes_outer_dirty_batch() {
        assert_eq!(finish_depth(2, true), (1, false));
        assert_eq!(finish_depth(1, false), (0, false));
        assert_eq!(finish_depth(1, true), (0, true));
    }
}
