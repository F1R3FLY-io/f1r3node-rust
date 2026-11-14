use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};

use crate::rspace::rho_path::{RhoPathChannelImage, RhoPathEncode};

pub trait LegacyChannelDigest {
    fn legacy_digest(identity: &[u8]) -> [u8; 32];
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Blake2bLegacyDigest;

impl LegacyChannelDigest for Blake2bLegacyDigest {
    fn legacy_digest(identity: &[u8]) -> [u8; 32] { Blake2b::<U32>::digest(identity).into() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelImageBridgeError {
    ImageLimitReached { limit: usize },
    AllocationFailure,
}

impl fmt::Display for ChannelImageBridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ImageLimitReached { limit } => write!(
                formatter,
                "the channel image bridge already holds its limit of {limit} distinct images"
            ),
            Self::AllocationFailure => formatter
                .write_str("the channel image bridge could not reserve memory for an image"),
        }
    }
}

impl std::error::Error for ChannelImageBridgeError {}

pub trait ChannelRepresentatives<C> {
    fn admit(&mut self, channel: &C) -> Result<(), ChannelImageBridgeError>;
    fn representative(&self, image: &[u8]) -> Option<&C>;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NoRepresentatives;

impl<C> ChannelRepresentatives<C> for NoRepresentatives {
    fn admit(&mut self, _channel: &C) -> Result<(), ChannelImageBridgeError> { Ok(()) }

    fn representative(&self, _image: &[u8]) -> Option<&C> { None }
}

pub struct ChannelImageBridge<C, D = Blake2bLegacyDigest> {
    representatives: HashMap<Arc<[u8]>, C>,
    buckets: HashMap<[u8; 32], Vec<Arc<[u8]>>>,
    image_limit: usize,
    digest: PhantomData<fn() -> D>,
}

impl<C, D> ChannelImageBridge<C, D> {
    pub fn new() -> Self { Self::with_image_limit(usize::MAX) }

    pub fn with_image_limit(image_limit: usize) -> Self {
        Self {
            representatives: HashMap::new(),
            buckets: HashMap::new(),
            image_limit,
            digest: PhantomData,
        }
    }

    pub const fn image_limit(&self) -> usize { self.image_limit }

    pub fn len(&self) -> usize { self.representatives.len() }

    pub fn is_empty(&self) -> bool { self.representatives.is_empty() }

    pub fn images_with_legacy_digest(
        &self,
        legacy_digest: &[u8; 32],
    ) -> impl ExactSizeIterator<Item = &[u8]> + '_ {
        let bucket: &[Arc<[u8]>] = match self.buckets.get(legacy_digest) {
            Some(bucket) => bucket,
            None => &[],
        };
        bucket.iter().map(|image| &**image)
    }
}

impl<C, D> ChannelImageBridge<C, D>
where
    C: RhoPathEncode + Clone,
    D: LegacyChannelDigest,
{
    fn admit_distinct(
        &mut self,
        image: Vec<u8>,
        channel: &C,
    ) -> Result<(), ChannelImageBridgeError> {
        if self.representatives.len() >= self.image_limit {
            return Err(ChannelImageBridgeError::ImageLimitReached {
                limit: self.image_limit,
            });
        }
        let legacy_digest = D::legacy_digest(&channel.rho_path_identity());
        self.representatives
            .try_reserve(1)
            .map_err(|_| ChannelImageBridgeError::AllocationFailure)?;
        let shared_image: Arc<[u8]> = Arc::from(image);
        let representative = channel.clone();
        match self.buckets.get_mut(&legacy_digest) {
            Some(bucket) => {
                bucket
                    .try_reserve(1)
                    .map_err(|_| ChannelImageBridgeError::AllocationFailure)?;
                bucket.push(Arc::clone(&shared_image));
            }
            None => {
                self.buckets
                    .try_reserve(1)
                    .map_err(|_| ChannelImageBridgeError::AllocationFailure)?;
                let mut bucket = Vec::new();
                bucket
                    .try_reserve_exact(1)
                    .map_err(|_| ChannelImageBridgeError::AllocationFailure)?;
                bucket.push(Arc::clone(&shared_image));
                self.buckets.insert(legacy_digest, bucket);
            }
        }
        self.representatives.insert(shared_image, representative);
        Ok(())
    }
}

impl<C, D> ChannelRepresentatives<C> for ChannelImageBridge<C, D>
where
    C: RhoPathEncode + RhoPathChannelImage + Clone,
    D: LegacyChannelDigest,
{
    fn admit(&mut self, channel: &C) -> Result<(), ChannelImageBridgeError> {
        let image = channel.rho_path_channel_image();
        match self.representatives.get(image.as_slice()) {
            Some(_) => Ok(()),
            None => self.admit_distinct(image, channel),
        }
    }

    fn representative(&self, image: &[u8]) -> Option<&C> { self.representatives.get(image) }
}

impl<C, D> Default for ChannelImageBridge<C, D> {
    fn default() -> Self { Self::new() }
}

impl<C: Clone, D> Clone for ChannelImageBridge<C, D> {
    fn clone(&self) -> Self {
        Self {
            representatives: self.representatives.clone(),
            buckets: self.buckets.clone(),
            image_limit: self.image_limit,
            digest: PhantomData,
        }
    }
}

impl<C: PartialEq, D> PartialEq for ChannelImageBridge<C, D> {
    fn eq(&self, other: &Self) -> bool {
        self.image_limit == other.image_limit &&
            self.representatives == other.representatives &&
            self.buckets == other.buckets
    }
}

impl<C: Eq, D> Eq for ChannelImageBridge<C, D> {}

impl<C: fmt::Debug, D> fmt::Debug for ChannelImageBridge<C, D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChannelImageBridge")
            .field("representatives", &self.representatives)
            .field("buckets", &self.buckets)
            .field("image_limit", &self.image_limit)
            .finish_non_exhaustive()
    }
}
