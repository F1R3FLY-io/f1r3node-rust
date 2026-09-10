pub(super) fn commit_then_publish<P, E>(
    plan: P,
    commit: impl FnOnce(&P) -> Result<(), E>,
    publish: impl FnOnce(P),
) -> Result<(), E> {
    commit(&plan)?;
    publish(plan);
    Ok(())
}
