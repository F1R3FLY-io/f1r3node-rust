use std::fmt::{Display, Formatter};
use std::num::NonZeroU64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DurableLimitKind {
    LivePacks,
    PackBytes,
    Transactions,
    ObjectRecords,
    TotalScanBytes,
    TotalPhysicalReadBytes,
    IndexBytes,
    IndexRecords,
    FullByteComparisonBytes,
    AggregateFullByteComparisonBytes,
}

impl DurableLimitKind {
    pub const ALL: [Self; 10] = [
        Self::LivePacks,
        Self::PackBytes,
        Self::Transactions,
        Self::ObjectRecords,
        Self::TotalScanBytes,
        Self::TotalPhysicalReadBytes,
        Self::IndexBytes,
        Self::IndexRecords,
        Self::FullByteComparisonBytes,
        Self::AggregateFullByteComparisonBytes,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::LivePacks => "live packs",
            Self::PackBytes => "pack bytes",
            Self::Transactions => "transactions",
            Self::ObjectRecords => "object records",
            Self::TotalScanBytes => "total scan bytes",
            Self::TotalPhysicalReadBytes => "total physical read bytes",
            Self::IndexBytes => "index bytes",
            Self::IndexRecords => "index records",
            Self::FullByteComparisonBytes => "individual full-byte comparison bytes",
            Self::AggregateFullByteComparisonBytes => "aggregate full-byte comparison bytes",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DurableLimitError {
    Zero {
        resource: DurableLimitKind,
    },
    Exceeded {
        resource: DurableLimitKind,
        limit: u64,
        attempted: u64,
    },
    CounterOverflow {
        resource: DurableLimitKind,
        current: u64,
        additional: u64,
    },
}

impl Display for DurableLimitError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Zero { resource } => {
                write!(formatter, "{} limit must be nonzero", resource.name())
            }
            Self::Exceeded {
                resource,
                limit,
                attempted,
            } => write!(
                formatter,
                "{} limit exceeded: limit {limit}, attempted {attempted}",
                resource.name()
            ),
            Self::CounterOverflow {
                resource,
                current,
                additional,
            } => write!(
                formatter,
                "{} counter overflow: current {current}, additional {additional}",
                resource.name()
            ),
        }
    }
}

impl std::error::Error for DurableLimitError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DurableLimitConfig {
    pub max_live_packs: u64,
    pub max_pack_bytes: u64,
    pub max_transactions: u64,
    pub max_object_records: u64,
    pub max_total_scan_bytes: u64,
    pub max_total_physical_read_bytes: u64,
    pub max_index_bytes: u64,
    pub max_index_records: u64,
    pub max_full_byte_comparison_bytes: u64,
    pub max_aggregate_full_byte_comparison_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DurableLimits {
    live_packs: NonZeroU64,
    pack_bytes: NonZeroU64,
    transactions: NonZeroU64,
    object_records: NonZeroU64,
    total_scan_bytes: NonZeroU64,
    total_physical_read_bytes: NonZeroU64,
    index_bytes: NonZeroU64,
    index_records: NonZeroU64,
    full_byte_comparison_bytes: NonZeroU64,
    aggregate_full_byte_comparison_bytes: NonZeroU64,
}

impl DurableLimits {
    pub const UNBOUNDED: Self = Self {
        live_packs: NonZeroU64::MAX,
        pack_bytes: NonZeroU64::MAX,
        transactions: NonZeroU64::MAX,
        object_records: NonZeroU64::MAX,
        total_scan_bytes: NonZeroU64::MAX,
        total_physical_read_bytes: NonZeroU64::MAX,
        index_bytes: NonZeroU64::MAX,
        index_records: NonZeroU64::MAX,
        full_byte_comparison_bytes: NonZeroU64::MAX,
        aggregate_full_byte_comparison_bytes: NonZeroU64::MAX,
    };

    pub fn new(config: DurableLimitConfig) -> Result<Self, DurableLimitError> {
        Ok(Self {
            live_packs: nonzero(DurableLimitKind::LivePacks, config.max_live_packs)?,
            pack_bytes: nonzero(DurableLimitKind::PackBytes, config.max_pack_bytes)?,
            transactions: nonzero(DurableLimitKind::Transactions, config.max_transactions)?,
            object_records: nonzero(DurableLimitKind::ObjectRecords, config.max_object_records)?,
            total_scan_bytes: nonzero(
                DurableLimitKind::TotalScanBytes,
                config.max_total_scan_bytes,
            )?,
            total_physical_read_bytes: nonzero(
                DurableLimitKind::TotalPhysicalReadBytes,
                config.max_total_physical_read_bytes,
            )?,
            index_bytes: nonzero(DurableLimitKind::IndexBytes, config.max_index_bytes)?,
            index_records: nonzero(DurableLimitKind::IndexRecords, config.max_index_records)?,
            full_byte_comparison_bytes: nonzero(
                DurableLimitKind::FullByteComparisonBytes,
                config.max_full_byte_comparison_bytes,
            )?,
            aggregate_full_byte_comparison_bytes: nonzero(
                DurableLimitKind::AggregateFullByteComparisonBytes,
                config.max_aggregate_full_byte_comparison_bytes,
            )?,
        })
    }

    pub const fn maximum(self, resource: DurableLimitKind) -> NonZeroU64 {
        match resource {
            DurableLimitKind::LivePacks => self.live_packs,
            DurableLimitKind::PackBytes => self.pack_bytes,
            DurableLimitKind::Transactions => self.transactions,
            DurableLimitKind::ObjectRecords => self.object_records,
            DurableLimitKind::TotalScanBytes => self.total_scan_bytes,
            DurableLimitKind::TotalPhysicalReadBytes => self.total_physical_read_bytes,
            DurableLimitKind::IndexBytes => self.index_bytes,
            DurableLimitKind::IndexRecords => self.index_records,
            DurableLimitKind::FullByteComparisonBytes => self.full_byte_comparison_bytes,
            DurableLimitKind::AggregateFullByteComparisonBytes => {
                self.aggregate_full_byte_comparison_bytes
            }
        }
    }

    pub fn require(
        self,
        resource: DurableLimitKind,
        attempted: u64,
    ) -> Result<(), DurableLimitError> {
        let limit = self.maximum(resource).get();
        if attempted > limit {
            Err(DurableLimitError::Exceeded {
                resource,
                limit,
                attempted,
            })
        } else {
            Ok(())
        }
    }

    pub fn checked_charge(
        self,
        resource: DurableLimitKind,
        current: u64,
        additional: u64,
    ) -> Result<u64, DurableLimitError> {
        let attempted =
            current
                .checked_add(additional)
                .ok_or(DurableLimitError::CounterOverflow {
                    resource,
                    current,
                    additional,
                })?;
        self.require(resource, attempted)?;
        Ok(attempted)
    }

    pub fn charge_full_byte_comparison(
        self,
        aggregate: u64,
        comparison_bytes: u64,
    ) -> Result<u64, DurableLimitError> {
        self.require(DurableLimitKind::FullByteComparisonBytes, comparison_bytes)?;
        self.checked_charge(
            DurableLimitKind::AggregateFullByteComparisonBytes,
            aggregate,
            comparison_bytes,
        )
    }
}

fn nonzero(resource: DurableLimitKind, value: u64) -> Result<NonZeroU64, DurableLimitError> {
    NonZeroU64::new(value).ok_or(DurableLimitError::Zero { resource })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(value: u64) -> DurableLimitConfig {
        DurableLimitConfig {
            max_live_packs: value,
            max_pack_bytes: value,
            max_transactions: value,
            max_object_records: value,
            max_total_scan_bytes: value,
            max_total_physical_read_bytes: value,
            max_index_bytes: value,
            max_index_records: value,
            max_full_byte_comparison_bytes: value,
            max_aggregate_full_byte_comparison_bytes: value,
        }
    }

    fn set(config: &mut DurableLimitConfig, resource: DurableLimitKind, value: u64) {
        match resource {
            DurableLimitKind::LivePacks => config.max_live_packs = value,
            DurableLimitKind::PackBytes => config.max_pack_bytes = value,
            DurableLimitKind::Transactions => config.max_transactions = value,
            DurableLimitKind::ObjectRecords => config.max_object_records = value,
            DurableLimitKind::TotalScanBytes => config.max_total_scan_bytes = value,
            DurableLimitKind::TotalPhysicalReadBytes => {
                config.max_total_physical_read_bytes = value;
            }
            DurableLimitKind::IndexBytes => config.max_index_bytes = value,
            DurableLimitKind::IndexRecords => config.max_index_records = value,
            DurableLimitKind::FullByteComparisonBytes => {
                config.max_full_byte_comparison_bytes = value;
            }
            DurableLimitKind::AggregateFullByteComparisonBytes => {
                config.max_aggregate_full_byte_comparison_bytes = value;
            }
        }
    }

    #[test]
    fn every_configured_limit_is_explicit_and_nonzero() {
        for resource in DurableLimitKind::ALL {
            let mut candidate = config(7);
            set(&mut candidate, resource, 0);
            assert_eq!(DurableLimits::new(candidate), Err(DurableLimitError::Zero { resource }));
        }
    }

    #[test]
    fn every_resource_has_an_exact_deterministic_boundary() {
        let limits = DurableLimits::new(config(7)).unwrap();
        for resource in DurableLimitKind::ALL {
            assert_eq!(limits.maximum(resource).get(), 7);
            assert_eq!(limits.require(resource, 7), Ok(()));
            assert_eq!(
                limits.require(resource, 8),
                Err(DurableLimitError::Exceeded {
                    resource,
                    limit: 7,
                    attempted: 8,
                })
            );
        }
    }

    #[test]
    fn checked_charge_rejects_overflow_before_limit_comparison() {
        let limits = DurableLimits::new(config(u64::MAX)).unwrap();
        assert_eq!(
            limits.checked_charge(DurableLimitKind::TotalScanBytes, u64::MAX, 1),
            Err(DurableLimitError::CounterOverflow {
                resource: DurableLimitKind::TotalScanBytes,
                current: u64::MAX,
                additional: 1,
            })
        );
    }

    #[test]
    fn full_byte_comparisons_enforce_individual_and_aggregate_budgets() {
        let mut candidate = config(100);
        candidate.max_full_byte_comparison_bytes = 8;
        candidate.max_aggregate_full_byte_comparison_bytes = 12;
        let limits = DurableLimits::new(candidate).unwrap();

        assert_eq!(limits.charge_full_byte_comparison(0, 8), Ok(8));
        assert_eq!(
            limits.charge_full_byte_comparison(0, 9),
            Err(DurableLimitError::Exceeded {
                resource: DurableLimitKind::FullByteComparisonBytes,
                limit: 8,
                attempted: 9,
            })
        );
        assert_eq!(
            limits.charge_full_byte_comparison(8, 5),
            Err(DurableLimitError::Exceeded {
                resource: DurableLimitKind::AggregateFullByteComparisonBytes,
                limit: 12,
                attempted: 13,
            })
        );
    }
}
