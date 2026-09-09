pub(crate) fn effect_is_complete<E>(
    revision: u64,
    mut read_cursor: impl FnMut() -> Result<u64, E>,
    mut read_receipt: impl FnMut() -> Result<bool, E>,
) -> Result<bool, E> {
    if revision <= read_cursor()? {
        return Ok(true);
    }
    if read_receipt()? {
        return Ok(true);
    }
    Ok(revision <= read_cursor()?)
}
