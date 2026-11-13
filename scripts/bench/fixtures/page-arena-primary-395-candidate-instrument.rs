#[cfg(test)]
impl<'src> CanonicalBytesArena<'src> {
    fn primary_cache_only_nodes(&self, active: &[SourceTreeWindow<'src>]) -> usize {
        let mut cache =
            crate::rust::rholang::canonical_source_tree::tests::SourcePhysicalInventory::default();
        for anchor in self.completed_page_prefixes.iter().flatten() {
            cache.observe(&anchor.root);
        }
        for entry in self.source_cache.iter().flatten() {
            if let Some(root) = entry.root.upgrade() {
                cache.observe(&root);
            }
        }
        let mut external =
            crate::rust::rholang::canonical_source_tree::tests::SourcePhysicalInventory::default();
        for root in active {
            external.observe(root);
        }
        cache.nodes.difference(&external.nodes).count()
    }
}
