pub(super) fn metadata_present<E>(
    admitted: bool,
    read: impl FnOnce() -> Result<bool, E>,
) -> Result<bool, E> {
    if admitted {
        read()
    } else {
        Ok(false)
    }
}

pub(super) fn publish_if_unadmitted<G, R, E>(
    _guard: G,
    admitted: impl FnOnce() -> Result<bool, E>,
    publish: impl FnOnce() -> Result<R, E>,
) -> Result<Option<R>, E> {
    if admitted()? {
        Ok(None)
    } else {
        publish().map(Some)
    }
}
