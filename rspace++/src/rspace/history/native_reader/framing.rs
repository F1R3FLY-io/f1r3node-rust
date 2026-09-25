use super::NativeReadFault;

pub(super) const MAX_NODE_BYTES: usize = 256 * (2 + 127 + 32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeLeafKind {
    Data,
    Continuations,
    Joins,
}

impl NativeLeafKind {
    pub(super) fn prefix(self) -> u8 {
        match self {
            Self::Data => 0,
            Self::Continuations => 1,
            Self::Joins => 2,
        }
    }

    fn tag(self) -> u32 {
        match self {
            Self::Joins => 0,
            Self::Data => 1,
            Self::Continuations => 2,
        }
    }
}

fn take<'a>(
    bytes: &'a [u8],
    offset: &mut usize,
    count: usize,
) -> Result<&'a [u8], NativeReadFault> {
    let end = offset.checked_add(count).ok_or(NativeReadFault::Length)?;
    let slice = bytes.get(*offset..end).ok_or(NativeReadFault::Truncated)?;
    *offset = end;
    Ok(slice)
}

fn length(bytes: &[u8], offset: &mut usize) -> Result<usize, NativeReadFault> {
    let bytes: [u8; 8] = take(bytes, offset, 8)?
        .try_into()
        .expect("fixed length field");
    usize::try_from(u64::from_le_bytes(bytes)).map_err(|_| NativeReadFault::Length)
}

#[derive(Clone, Copy)]
pub struct NativeRecords<'a> {
    rows: &'a [u8],
    count: usize,
}

impl<'a> NativeRecords<'a> {
    pub(super) fn parse(
        bytes: &'a [u8],
        kind: NativeLeafKind,
    ) -> Result<(Self, &'a [u8]), NativeReadFault> {
        let mut offset = 0;
        let tag = u32::from_le_bytes(
            take(bytes, &mut offset, 4)?
                .try_into()
                .expect("fixed tag field"),
        );
        if tag != kind.tag() {
            return Err(NativeReadFault::LeafKind);
        }
        let size = length(bytes, &mut offset)?;
        let payload = take(bytes, &mut offset, size)?;
        let hash_bytes = &bytes[4..offset];
        let mut cursor = 0;
        let count = length(payload, &mut cursor)?;
        if count > (payload.len() - cursor) / 8 {
            return Err(NativeReadFault::Length);
        }
        let start = cursor;
        for _ in 0..count {
            let size = length(payload, &mut cursor)?;
            take(payload, &mut cursor, size)?;
        }
        Ok((
            Self {
                rows: &payload[start..cursor],
                count,
            },
            hash_bytes,
        ))
    }

    pub fn len(&self) -> usize { self.count }
    pub fn is_empty(&self) -> bool { self.count == 0 }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &'a [u8]> + '_ {
        let mut offset = 0;
        (0..self.count).map(move |_| {
            let size = length(self.rows, &mut offset).expect("validated record length");
            take(self.rows, &mut offset, size).expect("validated record span")
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum NodeStep {
    Absent,
    Leaf([u8; 32]),
    Child { hash: [u8; 32], consumed: usize },
}

pub(super) fn node_step(bytes: &[u8], key: &[u8]) -> Result<NodeStep, NativeReadFault> {
    if bytes.len() > MAX_NODE_BYTES {
        return Err(NativeReadFault::NodeSize);
    }
    let mut seen = [0_u64; 4];
    let mut offset = 0;
    let mut result = NodeStep::Absent;
    while offset < bytes.len() {
        let header = take(bytes, &mut offset, 2)?;
        let index = header[0] as usize;
        let bit = 1_u64 << (index % 64);
        if seen[index / 64] & bit != 0 {
            return Err(NativeReadFault::DuplicateIndex);
        }
        seen[index / 64] |= bit;
        let prefix = take(bytes, &mut offset, (header[1] & 127) as usize)?;
        let hash: [u8; 32] = take(bytes, &mut offset, 32)?
            .try_into()
            .expect("fixed hash field");
        let Some((first, tail)) = key.split_first() else {
            continue;
        };
        if *first != header[0] {
            continue;
        }
        if header[1] & 128 == 0 {
            if prefix == tail {
                result = NodeStep::Leaf(hash);
            }
        } else if tail.starts_with(prefix) {
            result = NodeStep::Child {
                hash,
                consumed: 1 + prefix.len(),
            };
        }
    }
    Ok(result)
}
