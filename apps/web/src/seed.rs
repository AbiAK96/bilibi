//! Explicit, idempotent demo seed. Never runs at application boot.
use bilibi_database::{Db, Result};

pub async fn seed(db: &Db, password: &str) -> Result<()> {
    let hash = bilibi_user::hash_password(password)?;
    let mut client = db.get().await?;
    let tx = client.transaction().await?;
    tx.batch_execute(include_str!("seed.sql")).await?;
    for (id, tenant, name, email) in [
        (1_i64, 1_i64, "Ada Manager", "manager@example.test"),
        (2, 1, "Lee Viewer", "viewer@example.test"),
        (3, 1, "Sam Staff", "staff@example.test"),
        (4, 1, "Alex Admin", "admin@example.test"),
        (5, 2, "Other Manager", "manager@example.test"),
    ] {
        tx.execute("INSERT INTO users(id,tenant_id,name,email,password_hash) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING", &[&id,&tenant,&name,&email,&hash]).await?;
    }
    tx.batch_execute(include_str!("seed-users.sql")).await?;
    tx.commit().await?;
    Ok(())
}
