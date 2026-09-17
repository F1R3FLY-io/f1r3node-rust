//! Zipper wrapper types for integrating PathMap zippers with Rholang Par types.
//!
//! This module provides wrapper types that bridge PathMap's zipper API with Rholang's process-oriented
//! data model. Operations work on Par values as the unit of operation rather than raw bytes.

use pathmap::zipper::{ReadZipperUntracked, WriteZipperUntracked, ZipperHead};

use super::pathmap_integration::{par_to_path, RholangPathMap};
use crate::rhoapi::{EPathMap, Par};

/// Wrapper for PathMap ReadZipper that maintains Rholang context
pub struct RholangReadZipper<'a, 'path> {
    pub(crate) zipper: ReadZipperUntracked<'a, 'path, Par>,
    pub(crate) connective_used: bool,
    pub(crate) locally_free: Vec<u8>,
}

impl<'a, 'path> RholangReadZipper<'a, 'path> {
    /// Create a new read zipper from a PathMap at root
    pub fn new(map: &'a RholangPathMap, connective_used: bool, locally_free: Vec<u8>) -> Self {
        RholangReadZipper {
            zipper: map.read_zipper(),
            connective_used,
            locally_free,
        }
    }

    /// Create a new read zipper at a specific path
    pub fn new_at_path(
        map: &'a RholangPathMap,
        path: &Par,
        connective_used: bool,
        locally_free: Vec<u8>,
    ) -> Result<RholangReadZipper<'a, 'static>, String> {
        let segments = par_to_path(path);
        let key = flatten_segments(&segments);
        // Use the owned version since we can't return a reference to local key
        Ok(RholangReadZipper {
            zipper: map.read_zipper_at_path(key),
            connective_used,
            locally_free,
        })
    }

    /// Descend to a path specified as a Par (list of segments)
    pub fn descend_to(&mut self, path: &Par) -> Result<(), String> {
        use pathmap::zipper::ZipperMoving;

        let segments = par_to_path(path);
        let key = flatten_segments(&segments);
        self.zipper.descend_to(&key);
        Ok(())
    }

    /// Get the value at the current position
    pub fn get_val(&self) -> Option<&Par> {
        use pathmap::zipper::ZipperValues;
        self.zipper.val()
    }

    /// Check if there's a value at current position
    pub fn has_val(&self) -> bool {
        use pathmap::zipper::Zipper;
        self.zipper.is_val()
    }

    /// Check if the current path exists
    pub fn path_exists(&self) -> bool {
        use pathmap::zipper::Zipper;
        self.zipper.path_exists()
    }

    /// Convert zipper to Par representation
    /// This creates a special Par that represents the zipper state
    pub fn to_par(&self) -> Par {
        // For now, we'll represent the zipper as a special PathMap
        // In a full implementation, we'd need a custom Expr type for zippers
        // We'll create an empty PathMap as a placeholder since we can't easily
        // extract the underlying PathMap from the zipper
        let empty_pathmap = EPathMap {
            ps: vec![],
            locally_free: self.locally_free.clone(),
            connective_used: self.connective_used,
            remainder: None,
        };

        // Create a special Par that represents a read zipper
        // We'll use a special marker to identify it as a zipper
        Par::default().with_exprs(vec![crate::rhoapi::Expr {
            expr_instance: Some(crate::rhoapi::expr::ExprInstance::EPathmapBody(
                empty_pathmap,
            )),
        }])
    }
}

/// Wrapper for PathMap WriteZipper that maintains Rholang context
pub struct RholangWriteZipper<'a, 'path> {
    pub(crate) zipper: WriteZipperUntracked<'a, 'path, Par>,
    #[allow(dead_code)]
    pub(crate) connective_used: bool,
    #[allow(dead_code)]
    pub(crate) locally_free: Vec<u8>,
}

impl<'a, 'path> RholangWriteZipper<'a, 'path> {
    /// Create a new write zipper from a PathMap at root
    pub fn new(map: &'a mut RholangPathMap, connective_used: bool, locally_free: Vec<u8>) -> Self {
        RholangWriteZipper {
            zipper: map.write_zipper(),
            connective_used,
            locally_free,
        }
    }

    /// Create a new write zipper at a specific path
    pub fn new_at_path(
        map: &'a mut RholangPathMap,
        path: &Par,
        connective_used: bool,
        locally_free: Vec<u8>,
    ) -> Result<Self, String> {
        use pathmap::zipper::ZipperMoving;

        let segments = par_to_path(path);
        let key = flatten_segments(&segments);
        // Create a write zipper at the constructed path
        let mut zipper = map.write_zipper();
        zipper.descend_to(&key);
        Ok(RholangWriteZipper {
            zipper,
            connective_used,
            locally_free,
        })
    }

    /// Descend to a path specified as a Par (list of segments)
    pub fn descend_to(&mut self, path: &Par) -> Result<(), String> {
        use pathmap::zipper::ZipperMoving;

        let segments = par_to_path(path);
        let key = flatten_segments(&segments);
        self.zipper.descend_to(&key);
        Ok(())
    }

    /// Set the value at the current position
    pub fn set_val(&mut self, value: Par) -> Option<Par> {
        use pathmap::zipper::ZipperWriting;
        self.zipper.set_val(value)
    }

    /// Get the value at the current position
    pub fn get_val(&self) -> Option<&Par> {
        use pathmap::zipper::ZipperValues;
        self.zipper.val()
    }

    /// Remove the value at the current position
    pub fn remove_val(&mut self) -> Option<Par> {
        use pathmap::zipper::ZipperWriting;
        self.zipper.remove_val(true)
    }

    /// Remove all branches below the current position
    pub fn remove_branches(&mut self) {
        use pathmap::zipper::ZipperWriting;
        self.zipper.remove_branches(true);
    }

    /// Check if there's a value at current position
    pub fn has_val(&self) -> bool {
        use pathmap::zipper::Zipper;
        self.zipper.is_val()
    }

    /// Check if the current path exists
    pub fn path_exists(&self) -> bool {
        use pathmap::zipper::Zipper;
        self.zipper.path_exists()
    }

    /// Graft a subtrie from a read zipper
    pub fn graft<'b, 'bpath>(&mut self, read_zipper: &RholangReadZipper<'b, 'bpath>) {
        use pathmap::zipper::ZipperWriting;
        self.zipper.graft(&read_zipper.zipper);
    }

    /// Join (union) a subtrie from a read zipper
    pub fn join_into<'b, 'bpath>(&mut self, read_zipper: &RholangReadZipper<'b, 'bpath>) {
        use pathmap::zipper::ZipperWriting;
        self.zipper.join_into(&read_zipper.zipper);
    }

    /// Reset zipper to root
    pub fn reset(&mut self) {
        use pathmap::zipper::ZipperMoving;
        self.zipper.reset();
    }
}

/// Wrapper for PathMap ZipperHead that maintains Rholang context
pub struct RholangZipperHead<'a> {
    #[allow(dead_code)]
    pub(crate) zipper_head: ZipperHead<'a, 'a, Par>,
    #[allow(dead_code)]
    pub(crate) connective_used: bool,
    #[allow(dead_code)]
    pub(crate) locally_free: Vec<u8>,
}

impl<'a> RholangZipperHead<'a> {
    /// Create a new zipper head from a PathMap
    pub fn new(map: &'a mut RholangPathMap, connective_used: bool, locally_free: Vec<u8>) -> Self {
        RholangZipperHead {
            zipper_head: map.zipper_head(),
            connective_used,
            locally_free,
        }
    }
}

/// Helper function to flatten path segments with 0xFF separator
pub(crate) fn flatten_segments(segments: &[Vec<u8>]) -> Vec<u8> {
    segments
        .iter()
        .flat_map(|seg| {
            let mut v = seg.clone();
            v.push(0xFF); // separator
            v
        })
        .collect()
}

/// Helper function to unflatten path segments (split by 0xFF separator)
#[allow(dead_code)]
pub(crate) fn unflatten_segments(flattened: &[u8]) -> Vec<Vec<u8>> {
    flattened
        .split(|&b| b == 0xFF)
        .filter(|seg| !seg.is_empty())
        .map(|seg| seg.to_vec())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rhoapi::expr::ExprInstance;
    use crate::rust::pathmap_integration::create_pathmap_from_elements;
    use crate::rust::utils::new_gint_par;

    fn gint(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    #[test]
    fn flatten_segments_appends_separator_per_segment() {
        let segments = vec![vec![1u8, 2, 3], vec![4, 5]];
        assert_eq!(flatten_segments(&segments), vec![1, 2, 3, 0xFF, 4, 5, 0xFF]);
    }

    #[test]
    fn unflatten_segments_inverts_flatten() {
        let segments = vec![vec![1u8, 2, 3], vec![4, 5]];
        assert_eq!(unflatten_segments(&flatten_segments(&segments)), segments);
    }

    #[test]
    fn unflatten_segments_drops_empty_segments() {
        assert_eq!(
            unflatten_segments(&[0xFF, 1, 0xFF, 0xFF, 2, 3, 0xFF]),
            vec![vec![1u8], vec![2, 3]]
        );
        assert!(unflatten_segments(&[]).is_empty());
    }

    #[test]
    fn read_zipper_finds_inserted_value_by_par_path() {
        let elements = vec![gint(1), gint(2)];
        let created = create_pathmap_from_elements(&elements, None);

        let mut zipper = RholangReadZipper::new(&created.map, false, Vec::new());
        assert!(!zipper.has_val());
        zipper.descend_to(&gint(1)).unwrap();
        assert!(zipper.path_exists());
        assert!(zipper.has_val());
        assert_eq!(zipper.get_val(), Some(&gint(1)));
    }

    #[test]
    fn read_zipper_reports_missing_path() {
        let created = create_pathmap_from_elements(&[gint(1)], None);
        let mut zipper = RholangReadZipper::new(&created.map, false, Vec::new());
        zipper.descend_to(&gint(99)).unwrap();
        assert!(!zipper.has_val());
        assert_eq!(zipper.get_val(), None);
    }

    #[test]
    fn read_zipper_new_at_path_starts_at_value() {
        let created = create_pathmap_from_elements(&[gint(7)], None);
        let zipper = RholangReadZipper::new_at_path(&created.map, &gint(7), true, vec![1]).unwrap();
        assert!(zipper.has_val());
        assert_eq!(zipper.get_val(), Some(&gint(7)));
    }

    #[test]
    fn read_zipper_to_par_carries_metadata_into_pathmap_expr() {
        let created = create_pathmap_from_elements(&[gint(1)], None);
        let zipper = RholangReadZipper::new(&created.map, true, vec![3]);
        let par = zipper.to_par();
        assert_eq!(par.exprs.len(), 1);
        match &par.exprs[0].expr_instance {
            Some(ExprInstance::EPathmapBody(pathmap)) => {
                assert!(pathmap.ps.is_empty());
                assert!(pathmap.connective_used);
                assert_eq!(pathmap.locally_free, vec![3]);
            }
            other => panic!("expected EPathmapBody, got {:?}", other),
        }
    }

    #[test]
    fn write_zipper_sets_gets_and_removes_values() {
        let mut map = RholangPathMap::new();
        let mut zipper = RholangWriteZipper::new(&mut map, false, Vec::new());
        zipper.descend_to(&gint(7)).unwrap();
        assert!(!zipper.has_val());
        assert_eq!(zipper.set_val(gint(70)), None);
        assert!(zipper.has_val());
        assert!(zipper.path_exists());
        assert_eq!(zipper.get_val(), Some(&gint(70)));
        assert_eq!(zipper.set_val(gint(71)), Some(gint(70)));
        assert_eq!(zipper.remove_val(), Some(gint(71)));
        assert!(!zipper.has_val());
    }

    #[test]
    fn write_zipper_reset_returns_to_root() {
        let mut map = RholangPathMap::new();
        let mut zipper = RholangWriteZipper::new(&mut map, false, Vec::new());
        zipper.descend_to(&gint(1)).unwrap();
        zipper.set_val(gint(10));
        zipper.reset();
        assert!(!zipper.has_val());
        zipper.descend_to(&gint(1)).unwrap();
        assert_eq!(zipper.get_val(), Some(&gint(10)));
    }

    #[test]
    fn write_zipper_new_at_path_lands_on_requested_path() {
        let mut map = RholangPathMap::new();
        {
            let mut zipper =
                RholangWriteZipper::new_at_path(&mut map, &gint(5), false, Vec::new()).unwrap();
            zipper.set_val(gint(50));
        }
        let mut reader = RholangReadZipper::new(&map, false, Vec::new());
        reader.descend_to(&gint(5)).unwrap();
        assert_eq!(reader.get_val(), Some(&gint(50)));
    }

    #[test]
    fn write_zipper_remove_branches_clears_subtrie() {
        let mut map = RholangPathMap::new();
        {
            let mut zipper = RholangWriteZipper::new(&mut map, false, Vec::new());
            zipper.descend_to(&gint(1)).unwrap();
            zipper.set_val(gint(10));
            zipper.reset();
            zipper.descend_to(&gint(2)).unwrap();
            zipper.set_val(gint(20));
            zipper.reset();
            zipper.remove_branches();
        }
        let mut reader = RholangReadZipper::new(&map, false, Vec::new());
        reader.descend_to(&gint(1)).unwrap();
        assert!(!reader.has_val());
    }

    #[test]
    fn graft_copies_source_subtrie_under_current_position() {
        let source = create_pathmap_from_elements(&[gint(1)], None);
        let source_zipper = RholangReadZipper::new(&source.map, false, Vec::new());

        let mut dest = RholangPathMap::new();
        {
            let mut writer = RholangWriteZipper::new(&mut dest, false, Vec::new());
            writer.descend_to(&gint(9)).unwrap();
            writer.graft(&source_zipper);
        }

        let mut reader = RholangReadZipper::new(&dest, false, Vec::new());
        reader.descend_to(&gint(9)).unwrap();
        reader.descend_to(&gint(1)).unwrap();
        assert_eq!(reader.get_val(), Some(&gint(1)));
    }

    #[test]
    fn join_into_unions_source_with_existing_entries() {
        let source = create_pathmap_from_elements(&[gint(1)], None);
        let source_zipper = RholangReadZipper::new(&source.map, false, Vec::new());

        let dest_created = create_pathmap_from_elements(&[gint(2)], None);
        let mut dest = dest_created.map;
        {
            let mut writer = RholangWriteZipper::new(&mut dest, false, Vec::new());
            writer.join_into(&source_zipper);
        }

        let mut reader = RholangReadZipper::new(&dest, false, Vec::new());
        reader.descend_to(&gint(1)).unwrap();
        assert!(reader.has_val());
        let mut reader2 = RholangReadZipper::new(&dest, false, Vec::new());
        reader2.descend_to(&gint(2)).unwrap();
        assert!(reader2.has_val());
    }

    #[test]
    fn zipper_head_construction_keeps_metadata() {
        let mut map = RholangPathMap::new();
        let head = RholangZipperHead::new(&mut map, true, vec![7]);
        assert!(head.connective_used);
        assert_eq!(head.locally_free, vec![7]);
    }
}
