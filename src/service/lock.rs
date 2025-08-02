use sea_orm::{
    ColumnTrait as _, DatabaseTransaction, EntityTrait as _, QueryFilter as _, QuerySelect as _,
};

use crate::{
    context::Context,
    db::{
        custom_type::LockType,
        entity::{LockColumn, LockDsl},
    },
    error::NotifyExchangeResult,
};

pub struct LockService<'a> {
    #[allow(unused)]
    context: &'a Context,
}

impl<'a> LockService<'a> {
    fn new(context: &'a Context) -> Self {
        Self { context }
    }

    pub async fn lock(
        &self,
        txn: &DatabaseTransaction,
        code: &str,
        lock_type: LockType,
    ) -> NotifyExchangeResult<()> {
        tracing::info!("Before locking code: {code}, lock_type: {lock_type:?}");

        // There is no need to really write the lock to the database, we can't see it outside the
        // transaction.
        let _ = LockDsl::find()
            .filter(LockColumn::Code.eq(code))
            .filter(LockColumn::LockType.eq(lock_type))
            .lock_exclusive()
            .one(txn)
            .await?;

        tracing::info!("After locking code: {code}, lock_type: {lock_type:?}");
        Ok(())
    }
}

impl Context {
    pub fn lock_service(&self) -> LockService<'_> {
        LockService::new(self)
    }
}
