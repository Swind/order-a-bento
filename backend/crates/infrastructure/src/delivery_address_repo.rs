use crate::{
    sqlite::Db,
    timefmt::{from_text, to_text},
};
use application::ports::{
    DeliveryAddressRepo, PortError, PortResult, RepoError, RepoResult, Stored,
};
use async_trait::async_trait;
use domain::directory::{DeliveryAddress, DeliveryAddressId};
use domain::identity::OrganizerId;
use std::time::SystemTime;

#[derive(Clone)]
pub struct SqliteDeliveryAddressRepo {
    db: Db,
}

impl SqliteDeliveryAddressRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    id: String,
    name: String,
    address: String,
    updated_at: String,
}

fn stored(org: OrganizerId, r: Row) -> PortResult<Stored<DeliveryAddress>> {
    Ok(Stored {
        value: DeliveryAddress {
            id: r.id.parse().map_err(PortError::new)?,
            organizer_id: org,
            name: r.name,
            address: r.address,
        },
        updated_at: from_text(&r.updated_at).map_err(PortError::new)?,
    })
}

pub(crate) async fn create_tx(
    conn: &mut sqlx::SqliteConnection,
    a: &DeliveryAddress,
    now: SystemTime,
) -> RepoResult<()> {
    sqlx::query("INSERT INTO delivery_addresses (id, organizer_id, name, address, updated_at) VALUES (?, ?, ?, ?, ?)")
        .bind(a.id.to_string()).bind(a.organizer_id.to_string()).bind(&a.name).bind(&a.address).bind(to_text(now))
        .execute(conn).await.map_err(PortError::new)?;
    Ok(())
}

#[async_trait]
impl DeliveryAddressRepo for SqliteDeliveryAddressRepo {
    async fn list(&self, org: OrganizerId) -> PortResult<Vec<Stored<DeliveryAddress>>> {
        sqlx::query_as::<_, Row>("SELECT id, name, address, updated_at FROM delivery_addresses WHERE organizer_id = ? ORDER BY updated_at DESC, id")
            .bind(org.to_string()).fetch_all(&self.db.read).await.map_err(PortError::new)?
            .into_iter().map(|r| stored(org, r)).collect()
    }

    async fn find(
        &self,
        org: OrganizerId,
        id: DeliveryAddressId,
    ) -> PortResult<Option<Stored<DeliveryAddress>>> {
        sqlx::query_as::<_, Row>("SELECT id, name, address, updated_at FROM delivery_addresses WHERE organizer_id = ? AND id = ?")
            .bind(org.to_string()).bind(id.to_string()).fetch_optional(&self.db.read).await.map_err(PortError::new)?
            .map(|r| stored(org, r)).transpose()
    }

    async fn save(
        &self,
        value: DeliveryAddress,
        now: SystemTime,
        create: bool,
    ) -> RepoResult<Stored<DeliveryAddress>> {
        if create {
            let mut conn = self.db.write.acquire().await.map_err(PortError::new)?;
            create_tx(&mut conn, &value, now).await?;
        } else {
            let result = sqlx::query("UPDATE delivery_addresses SET name = ?, address = ?, updated_at = ? WHERE id = ? AND organizer_id = ?")
                .bind(&value.name).bind(&value.address).bind(to_text(now)).bind(value.id.to_string()).bind(value.organizer_id.to_string())
                .execute(&self.db.write).await.map_err(PortError::new)?;
            if result.rows_affected() == 0 {
                return Err(RepoError::NotFound);
            }
        }
        Ok(Stored {
            value,
            updated_at: from_text(&to_text(now)).map_err(PortError::new)?,
        })
    }

    async fn delete(&self, org: OrganizerId, id: DeliveryAddressId) -> PortResult<bool> {
        Ok(
            sqlx::query("DELETE FROM delivery_addresses WHERE organizer_id = ? AND id = ?")
                .bind(org.to_string())
                .bind(id.to_string())
                .execute(&self.db.write)
                .await
                .map_err(PortError::new)?
                .rows_affected()
                > 0,
        )
    }
}
