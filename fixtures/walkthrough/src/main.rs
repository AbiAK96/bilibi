//! `DATABASE_URL=… cargo run -p bilibi-walkthrough-fixture`: installs the
//! walkthrough fixture into a freshly migrated disposable database, for the
//! running-app walkthroughs that sign in with its accounts.
#[tokio::main]
async fn main() -> bilibi_database::Result<()> {
    let url = std::env::var("DATABASE_URL").map_err(|_| bilibi_database::Error::Invalid)?;
    bilibi_walkthrough_fixture::install(&bilibi_database::pool(&url)?).await?;
    println!(
        "Walkthrough fixture installed; every account's password is {}.",
        bilibi_walkthrough_fixture::PASSWORD
    );
    Ok(())
}
