pub fn merge_dependency_provenance(previous: bool, dependency_requested: bool) -> bool {
    previous || dependency_requested
}
