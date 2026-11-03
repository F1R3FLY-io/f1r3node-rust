use prost::bytes::Bytes;
use rholang::rust::interpreter::accounting::comm_ledger::Surface;
use rholang::rust::interpreter::accounting::rev_stack::WalletOwner;

pub type Surfaces = Vec<(Option<Surface>, u64)>;

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SurfaceCellsProto {
    #[prost(bytes = "bytes", tag = "1")]
    pub owner: Bytes,
    #[prost(bytes = "bytes", tag = "2")]
    pub envelope: Bytes,
    #[prost(uint64, tag = "3")]
    pub phlo_price: u64,
    #[prost(uint64, tag = "4")]
    pub cells: u64,
}

pub fn surface_purse(surface: &Surface) -> Result<Vec<WalletOwner>, String> {
    let owner = WalletOwner::from_canonical_bytes(&surface.owner)
        .ok_or_else(|| "a surface names no wallet".to_string())?;
    Ok(match owner {
        WalletOwner::Compound(components) => components,
        single => vec![single],
    })
}

pub fn is_own(surface: &Option<Surface>, envelope: &[u8]) -> bool {
    surface
        .as_ref()
        .is_none_or(|s| s.envelope.as_slice() == envelope)
}

pub fn own_cells(surfaces: &Surfaces, envelope: &[u8]) -> u64 {
    surfaces
        .iter()
        .filter(|(s, _)| is_own(s, envelope))
        .map(|(_, cells)| *cells)
        .sum()
}

pub fn foreign<'a>(
    surfaces: &'a Surfaces,
    envelope: &'a [u8],
) -> impl Iterator<Item = (&'a Surface, u64)> + 'a {
    surfaces.iter().filter_map(move |(s, cells)| match s {
        Some(s) if s.envelope.as_slice() != envelope => Some((s, *cells)),
        _ => None,
    })
}

pub fn to_proto(surfaces: &Surfaces) -> Vec<SurfaceCellsProto> {
    surfaces
        .iter()
        .map(|(s, cells)| match s {
            Some(s) => SurfaceCellsProto {
                owner: Bytes::from(s.owner.clone()),
                envelope: Bytes::from(s.envelope.clone()),
                phlo_price: s.phlo_price,
                cells: *cells,
            },
            None => SurfaceCellsProto {
                owner: Bytes::new(),
                envelope: Bytes::new(),
                phlo_price: 0,
                cells: *cells,
            },
        })
        .collect()
}

pub fn from_proto(cells: Vec<SurfaceCellsProto>) -> Result<Surfaces, String> {
    let surfaces: Surfaces = cells
        .into_iter()
        .map(|c| {
            if c.cells == 0 {
                return Err("a surface tally counts at least one cell".to_string());
            }
            if c.owner.is_empty() {
                if !c.envelope.is_empty() || c.phlo_price != 0 {
                    return Err("the client's surface names no origin".to_string());
                }
                return Ok((None, c.cells));
            }
            WalletOwner::from_canonical_bytes(&c.owner)
                .ok_or_else(|| "a surface names no wallet".to_string())?;
            Ok((
                Some(Surface {
                    owner: c.owner.to_vec(),
                    envelope: c.envelope.to_vec(),
                    phlo_price: c.phlo_price,
                }),
                c.cells,
            ))
        })
        .collect::<Result<_, _>>()?;
    if surfaces.windows(2).any(|w| w[0].0 >= w[1].0) {
        return Err("surface tallies are sorted and distinct".to_string());
    }
    Ok(surfaces)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(n: u8) -> Vec<u8> { WalletOwner::Unforgeable(vec![n; 32]).canonical_bytes() }

    fn surface(n: u8, envelope: u8) -> Option<Surface> {
        Some(Surface {
            owner: owner(n),
            envelope: vec![envelope],
            phlo_price: 3,
        })
    }

    #[test]
    fn own_and_foreign_surfaces_split_by_origin() {
        let tally: Surfaces = vec![(None, 2), (surface(1, 7), 3), (surface(2, 8), 4)];
        assert_eq!(own_cells(&tally, &[7]), 5);
        let foreign: Vec<_> = foreign(&tally, &[7]).map(|(s, c)| (s.clone(), c)).collect();
        assert_eq!(foreign, vec![(surface(2, 8).unwrap(), 4)]);
    }

    #[test]
    fn the_encoding_is_canonical() {
        let tally: Surfaces = vec![(None, 2), (surface(1, 7), 3)];
        assert_eq!(from_proto(to_proto(&tally)), Ok(tally.clone()));
        let mut reversed = to_proto(&tally);
        reversed.reverse();
        assert!(from_proto(reversed).is_err());
        let mut empty = to_proto(&tally);
        empty[1].cells = 0;
        assert!(from_proto(empty).is_err());
    }

    #[test]
    fn a_compound_purse_is_each_of_its_components() {
        let pair = WalletOwner::Compound(vec![
            WalletOwner::Unforgeable(vec![1; 32]),
            WalletOwner::Unforgeable(vec![2; 32]),
        ]);
        let s = Surface {
            owner: pair.canonical_bytes(),
            envelope: vec![1],
            phlo_price: 1,
        };
        assert_eq!(surface_purse(&s).unwrap().len(), 2);
    }
}
