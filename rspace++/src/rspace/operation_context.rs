use std::future::Future;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct OperationOrder {
    pub session: [u8; 32],
    pub path: Vec<(u64, u64)>,
}

tokio::task_local! {
    static OPERATION_ORDER: OperationOrder;
}

pub async fn scope<T>(order: OperationOrder, future: impl Future<Output = T>) -> T {
    OPERATION_ORDER.scope(order, future).await
}

pub fn current() -> Option<OperationOrder> { OPERATION_ORDER.try_with(Clone::clone).ok() }
