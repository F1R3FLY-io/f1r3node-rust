#[cfg(test)]
impl<'src> CanonicalBytesArena<'src> {
    fn primary_cache_only_nodes(&self, active: &[SourceTreeWindow<'src>]) -> usize {
        let mut cache =
            crate::rust::rholang::canonical_source_tree::tests::SourcePhysicalInventory::default();
        for root in self.source_cache.iter().flatten() {
            cache.observe(root);
        }
        let mut external =
            crate::rust::rholang::canonical_source_tree::tests::SourcePhysicalInventory::default();
        for root in active {
            external.observe(root);
        }
        cache.nodes.difference(&external.nodes).count()
    }
}
