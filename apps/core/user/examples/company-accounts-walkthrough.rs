//! Manual inspection of the company-accounts read (C11) through real services
//! on a disposable migrated database, which it gives the walkthrough fixture.
//! No assertions or unit tests; it prints each outcome. It unlinks an account
//! and archives the fixture's Company 002, so never run it on a working
//! database.
use bilibi_company::{CompanyService, SERVICE as COMPANIES};
use bilibi_database::pool;
use bilibi_party_ids::AffiliationId;
use bilibi_user::{company_accounts::CompanyAccount, UserService};
bilibi_walkthrough_fixture::walkthrough_vocabulary!();
bilibi_walkthrough_fixture::walkthrough_module_roles!();

type Failure = Box<dyn std::error::Error>;
const PASSWORD: &str = bilibi_walkthrough_fixture::PASSWORD;
const CONTRACTOR: i64 = 7;

fn listed(label: &str, outcome: &bilibi_database::Result<Vec<CompanyAccount>>) {
    match outcome {
        Ok(accounts) => {
            let names: Vec<String> = accounts
                .iter()
                .map(|a| format!("{} #{} <{}>", a.name, a.id, a.email))
                .collect();
            println!("{label}: {names:?}");
        }
        Err(error) => println!("{label}: refused {error:?}"),
    }
}
fn one(label: &str, outcome: &bilibi_database::Result<CompanyAccount>) {
    match outcome {
        Ok(a) => println!(
            "{label}: {} #{} person {} affiliation {} organisation {}",
            a.name,
            a.id,
            a.person.get(),
            a.affiliation.get(),
            a.organisation.get()
        ),
        Err(error) => println!("{label}: refused {error:?}"),
    }
}

#[tokio::main]
async fn main() -> Result<(), Failure> {
    if std::env::var("BILIBI_COMPANY_ACCOUNTS_DEMO").as_deref() != Ok("disposable") {
        return Err(
            "set BILIBI_COMPANY_ACCOUNTS_DEMO=disposable and a disposable migrated DATABASE_URL"
                .into(),
        );
    }
    let db = pool(&std::env::var("DATABASE_URL")?)?;
    bilibi_walkthrough_fixture::install(&db).await?;
    let app = bilibi_web::App::new(db.clone(), "http://localhost:35580")?;
    app.services
        .get::<bilibi_authz::grants::Grants>("base/authz.grants")?
        .reconcile()
        .await?;
    let users = app.services.get::<UserService>("core/user.accounts")?;
    let companies = app.services.get::<CompanyService>(COMPANIES)?;
    let manager = app
        .security
        .guard(&users.login("manager@example.test", PASSWORD).await?.token)
        .await?;
    let other = app
        .security
        .guard(
            &users
                .login("manager@other.example.test", PASSWORD)
                .await?
                .token,
        )
        .await?;

    println!("== the affiliated accounts of each company ==");
    listed(
        "Company 001",
        &users.list_company_accounts(&manager, 1).await,
    );
    listed(
        "Company 002",
        &users.list_company_accounts(&manager, 2).await,
    );
    listed(
        "Company 003 (nobody)",
        &users.list_company_accounts(&manager, 3).await,
    );
    one("get 1/1", &users.company_account(&manager, 1, 1).await);
    one(
        "get 2/7",
        &users.company_account(&manager, 2, CONTRACTOR).await,
    );
    one(
        "get 1/7 (account of another company)",
        &users.company_account(&manager, 1, CONTRACTOR).await,
    );
    one(
        "get 1/2 (account with no Person)",
        &users.company_account(&manager, 1, 2).await,
    );
    one(
        "get 1/6 (agent account, no affiliation)",
        &users.company_account(&manager, 1, 6).await,
    );

    println!("\n== the tenant bounds every answer ==");
    listed(
        "tenant 2 asks Company 001",
        &users.list_company_accounts(&other, 1).await,
    );
    one(
        "tenant 2 asks 1/1",
        &users.company_account(&other, 1, 1).await,
    );
    listed(
        "tenant 2 asks its own Company 100",
        &users.list_company_accounts(&other, 100).await,
    );
    listed(
        "unknown company",
        &users.list_company_accounts(&manager, 9_999).await,
    );
    listed(
        "non-positive company",
        &users.list_company_accounts(&manager, 0).await,
    );

    println!("\n== a proof that lapses removes the account ==");
    users.unlink(&manager, CONTRACTOR).await?;
    listed(
        "Company 002 after unlinking the contractor",
        &users.list_company_accounts(&manager, 2).await,
    );
    users
        .link_affiliation(&manager, CONTRACTOR, AffiliationId::new(2)?)
        .await?;
    listed(
        "Company 002 after relinking",
        &users.list_company_accounts(&manager, 2).await,
    );
    let client = db.get().await?;
    client
        .execute(
            "UPDATE users SET status='suspended' WHERE id=$1",
            &[&CONTRACTOR],
        )
        .await?;
    listed(
        "Company 002 with the account suspended",
        &users.list_company_accounts(&manager, 2).await,
    );
    client
        .execute(
            "UPDATE users SET status='active' WHERE id=$1",
            &[&CONTRACTOR],
        )
        .await?;
    client
        .execute("UPDATE persons SET retired_at=now() WHERE id=2", &[])
        .await?;
    listed(
        "Company 002 with the Person retired",
        &users.list_company_accounts(&manager, 2).await,
    );
    client
        .execute("UPDATE persons SET retired_at=NULL WHERE id=2", &[])
        .await?;

    println!("\n== an archived company stays readable ==");
    match companies
        .archive_company(&manager, 2, Some("Contract ended"))
        .await
    {
        Ok(company) => println!("Company 002 status: {}", company.status),
        Err(error) => println!("archive refused: {error}"),
    }
    listed(
        "Company 002 archived",
        &users.list_company_accounts(&manager, 2).await,
    );
    one(
        "get 2/7 archived",
        &users.company_account(&manager, 2, CONTRACTOR).await,
    );
    Ok(())
}
