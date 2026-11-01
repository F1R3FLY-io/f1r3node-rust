#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adoption {
    Adopted,
    Foreign,
}

pub fn persisted_accelerator_adoption<R: PartialEq>(
    bound_manifest: &R,
    current_manifest: &R,
) -> Adoption {
    let bound_matches = bound_manifest == current_manifest;
    match bound_matches {
        true => Adoption::Adopted,
        false => Adoption::Foreign,
    }
}
