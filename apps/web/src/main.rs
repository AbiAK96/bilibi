use bilibi_database::{pool, Error, Result};
#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::var("DATABASE_URL").map_err(|_| Error::Invalid)?;
    let db = pool(&url)?;
    match std::env::args().nth(1).as_deref() {
        Some("migrate") => bilibi_web::migrate(&db).await?,
        Some("seed") => {
            bilibi_web::seed::seed(&db, bilibi_web::seed::DEMO_PASSWORD).await?;
        }
        None | Some("run") => {
            let origin =
                std::env::var("APP_ORIGIN").unwrap_or_else(|_| "http://localhost:3000".into());
            let bind = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".into());
            let listener = tokio::net::TcpListener::bind(bind)
                .await
                .map_err(|_| Error::Internal)?;
            println!("Bilibi listening at {origin}");
            axum::serve(
                listener,
                bilibi_web::router(bilibi_web::App::new(db, &origin)?),
            )
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
            .map_err(|_| Error::Internal)?;
        }
        _ => return Err(Error::Invalid),
    }
    Ok(())
}
