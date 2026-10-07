use models::rust::phlo_wire::{PhloWireDecoder, PhloWireEncoder, PhloWireLimits};

use super::{invalid, CasperError};

const DOMAIN: &[u8] = b"f1r3node:prepaid-stack-cells:v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrepaidCellLimits {
    pub cells: usize,
    pub wire: PhloWireLimits,
}

#[derive(Debug, PartialEq, Eq)]
pub struct OrderedPrepaidCells<'a> {
    cells: Vec<&'a [u8]>,
}

impl<'a> OrderedPrepaidCells<'a> {
    pub fn encode(cells: &[&[u8]], limits: PrepaidCellLimits) -> Result<Vec<u8>, CasperError> {
        if cells.is_empty() || cells.len() > limits.cells {
            return Err(invalid("ordered cell count is out of range"));
        }
        let mut size = DOMAIN.len() + 16;
        for cell in cells {
            if cell.is_empty() || cell.len() > limits.wire.field_bytes {
                return Err(invalid("empty or oversized cell provenance"));
            }
            size = size
                .checked_add(8)
                .and_then(|n| n.checked_add(cell.len()))
                .ok_or_else(|| invalid("ordered cell byte count overflow"))?;
        }
        let mut out = PhloWireEncoder::with_capacity(limits.wire, size)
            .map_err(|e| invalid(&e.to_string()))?;
        out.bytes(DOMAIN).map_err(|e| invalid(&e.to_string()))?;
        out.u64(u64::try_from(cells.len()).map_err(|_| invalid("cell count overflow"))?)
            .map_err(|e| invalid(&e.to_string()))?;
        for cell in cells {
            out.bytes(cell).map_err(|e| invalid(&e.to_string()))?;
        }
        Ok(out.into_bytes())
    }

    pub fn decode(bytes: &'a [u8], limits: PrepaidCellLimits) -> Result<Self, CasperError> {
        let mut input =
            PhloWireDecoder::new(bytes, limits.wire).map_err(|e| invalid(&e.to_string()))?;
        if input.bytes().map_err(|e| invalid(&e.to_string()))? != DOMAIN {
            return Err(invalid("unsupported ordered cell domain"));
        }
        let count = usize::try_from(input.u64().map_err(|e| invalid(&e.to_string()))?)
            .map_err(|_| invalid("cell count overflow"))?;
        if count == 0 || count > limits.cells || count > input.remaining().len() / 9 {
            return Err(invalid("ordered cell count is out of range"));
        }
        let mut cells = Vec::new();
        cells
            .try_reserve_exact(count)
            .map_err(|_| invalid("cell allocation failed"))?;
        for _ in 0..count {
            let cell = input.bytes().map_err(|e| invalid(&e.to_string()))?;
            if cell.is_empty() {
                return Err(invalid("empty cell provenance"));
            }
            cells.push(cell);
        }
        input.finish().map_err(|e| invalid(&e.to_string()))?;
        Ok(Self { cells })
    }

    pub fn cells(&self) -> &[&'a [u8]] { &self.cells }

    pub fn split_consumed(&self, count: usize) -> Result<(&[&'a [u8]], &[&'a [u8]]), CasperError> {
        if count == 0 || count > self.cells.len() {
            return Err(invalid("cell consumption count is out of range"));
        }
        Ok(self.cells.split_at(count))
    }
}

#[cfg(test)]
mod tests;
