//! Explicit, idempotent demo seed. Never runs at application boot. Login
//! emails are unique platform-wide (K9), so tenant 2's manager has its own
//! address; `seed-users.sql` adds the demo Persons, affiliations and the
//! account links the K9 binder proves at every sign-in and request.
use bilibi_base::audit::{record_action, Attribution};
use bilibi_base::authz::grants::Grants;
use bilibi_database::{Db, Result};
use serde_json::json;

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
        (5, 2, "Other Manager", "manager@other.example.test"),
        (7, 1, "Kim Contractor", "contractor@example.test"),
    ] {
        tx.execute("INSERT INTO users(id,tenant_id,name,email,password_hash) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING", &[&id,&tenant,&name,&email,&hash]).await?;
    }
    tx.batch_execute(include_str!("seed-users.sql")).await?;
    // Bilimbi's dev seed bootstraps ai@agent.my once as the platform
    // operator's administrator: tenant 1 becomes the operator (unless another
    // tenant already is) and the user gains the grant-all `core_admin` system
    // role. The receipt row makes it one-time, so a database seeded before
    // this existed is fixed by its next seed, and a later seed never re-grants
    // a role an operator revoked.
    let bootstrapped = tx
        .execute(
            "INSERT INTO user_bootstrap(id,tenant_name,company_name,company_code,admin_email,tenant_id,user_id,company_id) \
             SELECT 1,t.name,c.name,c.code,u.email,t.id,u.id,c.id FROM tenants t \
             JOIN tenant_primary_companies p ON p.tenant_id=t.id JOIN companies c ON c.tenant_id=t.id AND c.id=p.company_id \
             JOIN users u ON u.tenant_id=t.id AND u.id=6 WHERE t.id=1 ON CONFLICT DO NOTHING",
            &[],
        )
        .await?
        == 1;
    if bootstrapped {
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
        let mut who = Attribution::system(Some("dev-seed"))?;
        who.tenant = Some(1);
        record_action(
            &*tx,
            &who,
            None,
            "user.bootstrap.completed",
            json!({"user_id":6,"email":"ai@agent.my","role":"core_admin"}),
            true,
        )
        .await?;
    }
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
