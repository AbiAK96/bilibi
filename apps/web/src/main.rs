use bilibi_database::{pool, Error, Result};
#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::var("DATABASE_URL").map_err(|_| Error::Invalid)?;
    let db = pool(&url)?;
    match std::env::args().nth(1).as_deref() {
        Some("migrate") => bilibi_web::migrate(&db).await?,
        Some("authz-reconcile") => {
            let app = bilibi_web::App::new(db.clone(), "http://localhost")?;
            app.services
                .get::<bilibi_base::authz::grants::Grants>("base/authz.grants")?
                .reconcile()
                .await?;
            println!("Authorization system roles reconciled");
        }
        Some("geonames-import") => {
            use bilibi_core::geonames::{ImportOptions, ReferenceImport, IMPORT_SERVICE, USAGE};
            let args: Vec<String> = std::env::args().skip(2).collect();
            let options = ImportOptions::from_args(&args).map_err(|message| {
                eprintln!("{message}\n{USAGE}");
                Error::Invalid
            })?;
            let app = bilibi_web::App::new(db.clone(), "http://localhost")?;
            let importer = app.services.get::<ReferenceImport>(IMPORT_SERVICE)?;
            println!("GeoNames cache: {}", options.cache_dir.display());
            match importer.run(&options).await {
                Ok(reports) => reports.iter().for_each(|report| println!("{report}")),
                Err(error) => {
                    eprintln!("GeoNames import failed: {error}");
                    return Err(Error::Internal);
                }
            }
        }
        Some("seed-production") => bilibi_web::seed::production(&db).await?,
        Some("seed") => {
            bilibi_web::seed::seed(&db, bilibi_web::seed::DEMO_PASSWORD).await?;
        }
        Some("media-cleanup") => {
            let args: Vec<String> = std::env::args().skip(2).collect();
            let parsed: Option<(i64, u64)> =
                match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
                    ["--tenant", tenant] => tenant.parse().ok().map(|tenant| (tenant, 24)),
                    ["--tenant", tenant, "--hours", hours] => {
                        tenant.parse().ok().zip(hours.parse().ok())
                    }
                    _ => None,
                };
            let Some((tenant, hours)) = parsed.filter(|(tenant, hours)| *tenant > 0 && *hours > 0)
            else {
                eprintln!("usage: bilibi-web media-cleanup --tenant ID [--hours POSITIVE_INTEGER]");
                return Err(Error::Invalid);
            };
            bilibi_web::media_cleanup(&db, tenant, hours).await?;
        }
        None | Some("run") => {
            let origin =
                std::env::var("APP_ORIGIN").unwrap_or_else(|_| "http://localhost:3000".into());
            let bind = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".into());
            let listener = tokio::net::TcpListener::bind(bind)
                .await
                .map_err(|_| Error::Internal)?;
            println!("Bilibi listening at {origin}");
            let app = bilibi_web::App::new(db, &origin)?;
            let executor = bilibi_base::queue::execution::Executor::from_services(
                &app.services,
                bilibi_base::queue::execution::Config::default(),
            )?;
            let scheduler = bilibi_base::schedule::Scheduler::from_services(
                &app.services,
                bilibi_base::schedule::scheduler::Config::default(),
            )?;
            let (stop, stopped) = tokio::sync::watch::channel(false);
            let history = tokio::spawn(app.perf.clone().run(stopped.clone()));
            let worker = tokio::spawn(executor.run(stopped.clone()));
            let ticker = tokio::spawn(scheduler.run(stopped));
            let shutdown = stop.clone();
            // The peer address feeds the OAuth per-sender throttle.
            let result = axum::serve(
                listener,
                bilibi_web::router(app)
                    .into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                let _ = tokio::signal::ctrl_c().await;
                let _ = shutdown.send(true);
            })
            .await
            .map_err(|_| Error::Internal);
            let _ = stop.send(true);
            worker.await.map_err(|_| Error::Internal)??;
            history.await.map_err(|_| Error::Internal)??;
            ticker.await.map_err(|_| Error::Internal)??;
            result?;
        }
        _ => return Err(Error::Invalid),
    }
    Ok(())
}
