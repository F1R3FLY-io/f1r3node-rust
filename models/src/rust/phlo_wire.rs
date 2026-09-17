use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloWireLimits {
    pub total_bytes: usize,
    pub field_bytes: usize,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PhloWireError {
    #[error("phlo wire data exceeds its byte limit")]
    LimitExceeded,
    #[error("phlo wire data ends inside a field")]
    Truncated,
    #[error("phlo wire data contains trailing bytes")]
    TrailingBytes,
    #[error("phlo wire field length exceeds its integer representation")]
    LengthOutOfRange,
    #[error("phlo wire allocation failed")]
    AllocationFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhloWireEncoder {
    output: Vec<u8>,
    limits: PhloWireLimits,
}

impl PhloWireEncoder {
    pub fn new(limits: PhloWireLimits) -> Self {
        Self {
            output: Vec::new(),
            limits,
        }
    }

    pub fn with_capacity(limits: PhloWireLimits, capacity: usize) -> Result<Self, PhloWireError> {
        if capacity > limits.total_bytes {
            return Err(PhloWireError::LimitExceeded);
        }
        let mut output = Vec::new();
        output
            .try_reserve_exact(capacity)
            .map_err(|_| PhloWireError::AllocationFailed)?;
        Ok(Self { output, limits })
    }

    pub fn as_bytes(&self) -> &[u8] { &self.output }

    pub fn into_bytes(self) -> Vec<u8> { self.output }

    pub fn u8(&mut self, value: u8) -> Result<(), PhloWireError> {
        self.append(&value.to_be_bytes(), &[])
    }

    pub fn u16(&mut self, value: u16) -> Result<(), PhloWireError> {
        self.append(&value.to_be_bytes(), &[])
    }

    pub fn u32(&mut self, value: u32) -> Result<(), PhloWireError> {
        self.append(&value.to_be_bytes(), &[])
    }

    pub fn u64(&mut self, value: u64) -> Result<(), PhloWireError> {
        self.append(&value.to_be_bytes(), &[])
    }

    pub fn u128(&mut self, value: u128) -> Result<(), PhloWireError> {
        self.append(&value.to_be_bytes(), &[])
    }

    pub fn bytes(&mut self, value: &[u8]) -> Result<(), PhloWireError> {
        if value.len() > self.limits.field_bytes {
            return Err(PhloWireError::LimitExceeded);
        }
        let length = u64::try_from(value.len()).map_err(|_| PhloWireError::LengthOutOfRange)?;
        self.append(&length.to_be_bytes(), value)
    }

    fn append(&mut self, prefix: &[u8], payload: &[u8]) -> Result<(), PhloWireError> {
        let additional = prefix
            .len()
            .checked_add(payload.len())
            .ok_or(PhloWireError::LimitExceeded)?;
        self.output
            .len()
            .checked_add(additional)
            .filter(|length| *length <= self.limits.total_bytes)
            .ok_or(PhloWireError::LimitExceeded)?;
        self.output
            .try_reserve(additional)
            .map_err(|_| PhloWireError::AllocationFailed)?;
        self.output.extend_from_slice(prefix);
        self.output.extend_from_slice(payload);
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhloWireDecoder<'a> {
    input: &'a [u8],
    position: usize,
    limits: PhloWireLimits,
}

impl<'a> PhloWireDecoder<'a> {
    pub fn new(input: &'a [u8], limits: PhloWireLimits) -> Result<Self, PhloWireError> {
        if input.len() > limits.total_bytes {
            return Err(PhloWireError::LimitExceeded);
        }
        Ok(Self {
            input,
            position: 0,
            limits,
        })
    }

    pub fn position(&self) -> usize { self.position }

    pub fn remaining(&self) -> &'a [u8] { &self.input[self.position..] }

    pub fn u8(&mut self) -> Result<u8, PhloWireError> { self.word().map(u8::from_be_bytes) }

    pub fn u16(&mut self) -> Result<u16, PhloWireError> { self.word().map(u16::from_be_bytes) }

    pub fn u32(&mut self) -> Result<u32, PhloWireError> { self.word().map(u32::from_be_bytes) }

    pub fn u64(&mut self) -> Result<u64, PhloWireError> { self.word().map(u64::from_be_bytes) }

    pub fn u128(&mut self) -> Result<u128, PhloWireError> { self.word().map(u128::from_be_bytes) }

    pub fn bytes(&mut self) -> Result<&'a [u8], PhloWireError> {
        let suffix = self.remaining();
        let header = suffix.get(..8).ok_or(PhloWireError::Truncated)?;
        let count = u64::from_be_bytes(header.try_into().map_err(|_| PhloWireError::Truncated)?);
        if u128::from(count) > self.limits.field_bytes as u128 {
            return Err(PhloWireError::LimitExceeded);
        }
        let count = usize::try_from(count).map_err(|_| PhloWireError::LengthOutOfRange)?;
        let end = 8usize
            .checked_add(count)
            .ok_or(PhloWireError::LimitExceeded)?;
        let payload = suffix.get(8..end).ok_or(PhloWireError::Truncated)?;
        self.position += end;
        Ok(payload)
    }

    pub fn finish(self) -> Result<(), PhloWireError> {
        if self.position == self.input.len() {
            Ok(())
        } else {
            Err(PhloWireError::TrailingBytes)
        }
    }

    fn word<const N: usize>(&mut self) -> Result<[u8; N], PhloWireError> {
        let word = self.remaining().get(..N).ok_or(PhloWireError::Truncated)?;
        let value = word.try_into().map_err(|_| PhloWireError::Truncated)?;
        self.position += N;
        Ok(value)
    }
}

#[cfg(test)]
mod tests;
