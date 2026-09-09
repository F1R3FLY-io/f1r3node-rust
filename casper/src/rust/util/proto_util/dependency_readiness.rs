pub(super) fn all_observed<E>(
    observations: impl IntoIterator<Item = Result<bool, E>>,
) -> Result<bool, E> {
    let mut ready = true;
    for observation in observations {
        ready &= observation?;
    }
    Ok(ready)
}
