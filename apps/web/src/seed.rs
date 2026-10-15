//! Explicit, idempotent demo seed. Never runs at application boot.
use bilibi_base::authz::grants::Grants;
use bilibi_database::{Db, Result};

pub const DEMO_PASSWORD: &str = "bilibi-dev";

pub async fn seed(db: &Db, password: &str) -> Result<()> {
    let hash = bilibi_user::hash_password(password)?;
    let app = crate::App::new(db.clone(), "http://localhost")?;
    let grants = app.services.get::<Grants>("base/authz.grants")?;
    let mut client = db.get().await?;
    let tx = client.transaction().await?;
    tx.batch_execute(include_str!("seed.sql")).await?;
    for (id, tenant, name, email) in [
        (6_i64, 1_i64, "AI Agent", "ai@agent.my"),
        (1_i64, 1_i64, "Ada Manager", "manager@example.test"),
        (2, 1, "Lee Viewer", "viewer@example.test"),
        (3, 1, "Sam Staff", "staff@example.test"),
        (4, 1, "Alex Admin", "admin@example.test"),
        (5, 2, "Other Manager", "manager@example.test"),
    ] {
        tx.execute("INSERT INTO users(id,tenant_id,name,email,password_hash) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING", &[&id,&tenant,&name,&email,&hash]).await?;
    }
    tx.batch_execute(include_str!("seed-users.sql")).await?;
    // Bilimbi's dev seed makes ai@agent.my the platform operator's
    // administrator: tenant 1 is the operator (unless another tenant already
    // is) and the user holds the grant-all `core_admin` system role. Re-runs
    // restore both on a database seeded before this existed.
    tx.execute(
        "INSERT INTO platform_operator(singleton,tenant_id) VALUES(true,1) ON CONFLICT DO NOTHING",
        &[],
    )
    .await?;
    let core_admin = grants.ensure_system_role_in(&tx, 1, "core_admin").await?;
    tx.execute(
        "INSERT INTO principal_roles(tenant_id,user_id,role_id) VALUES(1,6,$1) ON CONFLICT DO NOTHING",
        &[&core_admin],
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Execute only module-contributed production seeds, in dependency order.
pub async fn production(db: &Db) -> Result<()> {
    let app = crate::App::new(db.clone(), "http://localhost")?;
    let modules = crate::modules()?;
    for run in
        bilibi_database::seeds::run(db, &modules, app.services.catalog()?, &app.services).await?
    {
        println!(
            "{} {}",
            run.version,
            if run.applied {
                "applied"
            } else {
                "already applied"
            }
        );
    }
    Ok(())
}
