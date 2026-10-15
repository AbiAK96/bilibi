//! Manual walkthrough on a disposable database; no test harness.
//!
//! `dst [key] [instant]` needs no database. It replays the recurrence at
//! controlled clock input (an RFC 3339 instant) so DST gaps and folds can be read
//! directly. `review` exercises fingerprint review, suppression and
//! authorization against a disposable PostgreSQL (BILIBI_DISPOSABLE_SCHEDULE_DEMO=yes).
use bilibi_database::{pool, Error, MutationBoundary, Result};
use bilibi_module_registry::{Catalog, Contribution, Provenance, Services};
use bilibi_queue::{Queue, Worker, WORKERS};
use bilibi_schedule::{
    definition::fingerprint, recurrence, Definition, Eligibility, Misfire, Overlap, Schedule,
    DEFINITIONS,
};
use bilibi_tenancy::Authentication;
use jiff::{tz::TimeZone, Timestamp};
use serde_json::{json, Value};
use std::sync::Arc;

fn allowlist(args: Value) -> Result<Value> {
    match args.as_object() {
        Some(object) if object.is_empty() => Ok(args),
        _ => Err(Error::Invalid),
    }
}
macro_rules! provenance {
    () => {
        Provenance {
            module: "base/schedule",
            source: file!(),
            line: line!(),
        }
    };
}
const WORKER: &str = "schedule/walkthrough";
inventory::submit! { Contribution::new(WORKERS, WORKER, provenance!(), &Worker { max_attempts: 3, validate: allowlist }) }
macro_rules! definition {
    ($key:literal, $name:literal, $expression:literal, $zone:literal) => {
        inventory::submit! { Contribution::new(DEFINITIONS, $key, provenance!(), &Definition {
            name: $name, expression: $expression, timezone: $zone, task_name: $name, worker: WORKER,
            args: || json!({}), overlap: Overlap::Forbid, misfire: Misfire::Coalesce, owner_route: None,
        }) }
    };
}
definition!(
    "schedule/walkthrough.fold",
    "Daily 01:30 New York",
    "30 1 * * *",
    "America/New_York"
);
definition!(
    "schedule/walkthrough.gap",
    "Daily 02:30 New York",
    "30 2 * * *",
    "America/New_York"
);
definition!(
    "schedule/walkthrough.hourly",
    "Hourly UTC",
    "0 * * * *",
    "Etc/UTC"
);

/// A private owner so the walkthrough needs no HTTP host or seeded session.
fn show(zone: &TimeZone, at: Timestamp) -> String {
    format!(
        "{at} ({})",
        at.to_zoned(zone.clone()).strftime("%Y-%m-%d %H:%M %Z")
    )
}
fn replay(schedule: &Schedule, key: &str, now: Timestamp) {
    let item = schedule.definition(key).expect("registered key");
    let zone = item.zone();
    println!("  now      {}", show(zone, now));
    for (label, result) in [
        ("previous", item.previous(now)),
        ("next    ", item.next(now)),
    ] {
        match result {
            Ok(at) => println!("  {label} {}", show(zone, at)),
            Err(error) => println!("  {label} {error}"),
        }
    }
}

fn dst(schedule: &Schedule, args: &[String]) -> Result<()> {
    if let [key, instant] = args {
        replay(schedule, key, instant.parse().map_err(|_| Error::Invalid)?);
        return Ok(());
    }
    let ts = |text: &str| -> Timestamp { text.parse().expect("instant") };
    println!("== Fold: US clocks fall back 2026-11-01 02:00 EDT -> 01:00 EST (06:00Z); 01:30 happens twice");
    for (note, now) in [
        ("before the first 01:30", "2026-11-01T05:29:00Z"),
        ("exactly the first 01:30 (EDT)", "2026-11-01T05:30:00Z"),
        ("between the repeats", "2026-11-01T05:45:00Z"),
        ("exactly the second 01:30 (EST)", "2026-11-01T06:30:00Z"),
        ("after the second", "2026-11-01T06:31:00Z"),
    ] {
        println!("{note}  [schedule/walkthrough.fold]");
        replay(schedule, "schedule/walkthrough.fold", ts(now));
    }
    println!("\n== Gap: US clocks spring forward 2026-03-08 02:00 EST -> 03:00 EDT (07:00Z); 02:30 never happens");
    for (note, now) in [
        ("before the gap", "2026-03-08T06:59:00Z"),
        ("after clocks resume", "2026-03-08T07:00:00Z"),
        ("midday after the skipped day", "2026-03-08T16:00:00Z"),
    ] {
        println!("{note}  [schedule/walkthrough.gap]");
        replay(schedule, "schedule/walkthrough.gap", ts(now));
    }
    println!("\n== Wall-clock resolution of the same minutes");
    let zone = TimeZone::get("America/New_York").map_err(|_| Error::Invalid)?;
    for wall in [
        "2026-11-01T01:30:00",
        "2026-03-08T02:30:00",
        "2026-06-01T02:30:00",
    ] {
        println!(
            "  {wall} => {:?}",
            recurrence::resolve(&zone, wall.parse().expect("wall")).map_err(|_| Error::Invalid)?
        );
    }
    println!("\n== Cron grammar: what a contribution may say");
    for expression in [
        "*/15 9-17 * * mon-fri",
        "0 0 29 2 *",
        "0 0 31 2 *",
        "@daily",
        "0 0 * *",
        "60 * * * *",
        "0 0 L * *",
    ] {
        match bilibi_schedule::cron::Cron::parse(expression) {
            Ok(_) => println!("  {expression:<24} accepted"),
            Err(error) => println!("  {expression:<24} refused: {error}"),
        }
    }
    Ok(())
}

async fn review(services: &Services, db: &bilibi_database::Db) -> Result<()> {
    let schedule = services.get::<Schedule>("base/schedule.store")?;
    let authentication = services.get::<Authentication>("base/tenancy.authentication")?;
    let mut client = db.get().await?;
    client.batch_execute("INSERT INTO platform_operator(tenant_id) VALUES(1) ON CONFLICT DO NOTHING; DELETE FROM base_schedule_definition_reviews; DELETE FROM base_schedule_suppressions").await?;
    let now: Timestamp = "2026-11-01T05:45:00Z".parse().map_err(|_| Error::Invalid)?;
    let fold = "schedule/walkthrough.fold";
    // User 4 is the tenant-1 administrator (operator tenant, grant_all); user 1 is
    // an operator-tenant manager with no schedule grant; user 5 is in tenant 2.
    let admin = authentication
        .authenticate(&**client, 1, 4, String::new())
        .await?;
    let manager = authentication
        .authenticate(&**client, 1, 1, String::new())
        .await?;
    let outsider = authentication
        .authenticate(&**client, 2, 5, String::new())
        .await?;
    let table = |tasks: &[bilibi_schedule::Task]| {
        for task in tasks {
            println!(
                "  {:<28} review={:?} suppressed={} fingerprint={}.. next_due={}",
                task.key,
                task.review,
                task.suppressed,
                &task.fingerprint[..12],
                task.next_due_at.map_or("-".into(), |at| at.to_string())
            );
        }
    };
    println!("== Every definition starts unreviewed (clock {now})");
    table(&schedule.tasks(&**client, &admin, now).await?);
    println!(
        "eligibility(fold) = {:?}",
        schedule.eligibility(&**client, fold).await?
    );

    println!("\n== Authorization: refusals write nothing");
    println!(
        "manager review  -> {:?}",
        schedule
            .review(&mut client, &manager, fold, true)
            .await
            .map_err(|e| e.to_string())
    );
    println!(
        "outsider list   -> {:?}",
        schedule
            .tasks(&**client, &outsider, now)
            .await
            .map(|t| t.len())
            .map_err(|e| e.to_string())
    );
    println!(
        "unknown key     -> {:?}",
        schedule
            .review(&mut client, &admin, "no/such.key", true)
            .await
            .map_err(|e| e.to_string())
    );

    println!("\n== Review enables exactly this fingerprint");
    schedule.review(&mut client, &admin, fold, true).await?;
    println!(
        "eligibility(fold) = {:?}",
        schedule.eligibility(&**client, fold).await?
    );
    println!(
        "suppress          -> changed={}",
        schedule.suppress(&mut client, &admin, fold).await?
    );
    println!(
        "suppress again    -> changed={} (no audit row)",
        schedule.suppress(&mut client, &admin, fold).await?
    );
    println!(
        "eligibility(fold) = {:?}",
        schedule.eligibility(&**client, fold).await?
    );
    println!(
        "resume            -> changed={}",
        schedule.resume(&mut client, &admin, fold).await?
    );
    schedule.review(&mut client, &admin, fold, false).await?;
    println!(
        "after disable     = {:?}",
        schedule.eligibility(&**client, fold).await?
    );
    schedule.review(&mut client, &admin, fold, true).await?;

    println!("\n== A material change makes it unreviewed again");
    let item = schedule.definition(fold).expect("registered");
    let mut edited = *item.definition();
    edited.expression = "45 1 * * *";
    println!("fingerprint as deployed {}", item.fingerprint());
    println!(
        "fingerprint, new cron   {}",
        fingerprint(fold, item.owner(), &edited, item.args())
    );
    // A redeploy with the edited cron presents a different fingerprint to the
    // stored review; replay that by storing a decision for the old value.
    client
        .execute(
            "UPDATE base_schedule_definition_reviews SET fingerprint=$2 WHERE key=$1",
            &[
                &fold,
                &fingerprint(fold, item.owner(), &edited, item.args()),
            ],
        )
        .await?;
    println!(
        "eligibility(fold) = {:?} (stored review is for the edited definition)",
        schedule.eligibility(&**client, fold).await?
    );
    assert_eq!(
        schedule.eligibility(&**client, fold).await?,
        Eligibility::Unreviewed
    );

    println!("\n== Evidence");
    for row in client.query("SELECT event,auditable_type,new_values::text FROM base_audit_mutations WHERE auditable_type LIKE 'base/schedule.%' ORDER BY id", &[]).await? {
        println!("  audit mutation {} {} {}", row.get::<_, String>(0), row.get::<_, String>(1), row.get::<_, String>(2));
    }
    for row in client
        .query(
            "SELECT event FROM base_audit_actions WHERE event LIKE 'schedule.%' ORDER BY id",
            &[],
        )
        .await?
    {
        println!("  audit action   {}", row.get::<_, String>(0));
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    bilibi_base::link();
    bilibi_core::link();
    let command = std::env::args().nth(1).unwrap_or_else(|| "dst".into());
    let rest: Vec<String> = std::env::args().skip(2).collect();
    let modules = bilibi_module_registry::discover()?;
    if command == "dst" {
        // No database: the schedule only needs the catalogs and an unbound boundary.
        let catalog = Catalog::discover(&modules)?;
        let queue = Queue::new(bilibi_queue::Workers::from_catalog(&catalog)?);
        let schedule =
            Schedule::from_catalog(&catalog, &queue, Arc::new(MutationBoundary::default()))?;
        return dst(&schedule, &rest);
    }
    if std::env::var("BILIBI_DISPOSABLE_SCHEDULE_DEMO").as_deref() != Ok("yes")
        || command != "review"
    {
        return Err(Error::Invalid);
    }
    let db = pool(&std::env::var("DATABASE_URL").map_err(|_| Error::Invalid)?)?;
    bilibi_database::migrate(
        &db,
        &modules
            .iter()
            .flat_map(|m| m.migrations.iter().copied())
            .collect::<Vec<_>>(),
    )
    .await?;
    let mut services = Services::default();
    services.provide("bootstrap.database", Arc::new(db.clone()))?;
    services.install(&modules)?;
    review(&services, &db).await
}
