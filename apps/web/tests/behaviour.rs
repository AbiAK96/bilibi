//! Executes real HTTP, WebSocket, and PostgreSQL interfaces. Every test has its
//! own schema, so parallel runs never share tenants, roles, or audit state.
use bilibi_company::{Edit, Params, MASK};
use bilibi_database::{Db, Error};
use bilibi_security::{authorize, guard, restrictions, Capability};
use bilibi_web::{migrate, App};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_postgres::NoTls;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};

const PASSWORD: &str = "minimum-demo-password";
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
struct Fixture {
    db: Db,
    master: Db,
    schema: String,
    app: App,
    url: String,
    server: tokio::task::JoinHandle<()>,
    http: reqwest::Client,
}
impl Fixture {
    async fn new() -> Self {
        let url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL is required: tests must execute against PostgreSQL");
        let master = bilibi_database::pool(&url).unwrap();
        let schema = format!("test_{}", uuid::Uuid::new_v4().simple());
        master
            .get()
            .await
            .unwrap()
            .batch_execute(&format!("CREATE SCHEMA {schema}"))
            .await
            .unwrap();
        let mut config: tokio_postgres::Config = url.parse().unwrap();
        config.options(format!("-c search_path={schema}"));
        let db = deadpool_postgres::Pool::builder(deadpool_postgres::Manager::new(config, NoTls))
            .max_size(8)
            .build()
            .unwrap();
        migrate(&db).await.unwrap();
        migrate(&db).await.unwrap();
        bilibi_web::seed::seed(&db, PASSWORD).await.unwrap();
        bilibi_web::seed::seed(&db, PASSWORD).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = App::new(db.clone(), &url).unwrap();
        let router = bilibi_web::router(app.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            db,
            master,
            schema,
            app,
            url,
            server,
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
        }
    }
    async fn login(&self, email: &str, tenant: i64) -> bilibi_user::Login {
        bilibi_user::login(&self.db, tenant, email, PASSWORD)
            .await
            .unwrap()
    }
    async fn sql(&self, sql: &str) {
        self.db
            .get()
            .await
            .unwrap()
            .batch_execute(sql)
            .await
            .unwrap();
    }
    async fn count(&self, sql: &str) -> i64 {
        self.db
            .get()
            .await
            .unwrap()
            .query_one(sql, &[])
            .await
            .unwrap()
            .get(0)
    }
    async fn socket(&self, token: &str) -> Socket {
        let mut request = format!("{}/live", self.url.replace("http:", "ws:"))
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("origin", self.url.parse().unwrap());
        request
            .headers_mut()
            .insert("cookie", format!("bilibi_session={token}").parse().unwrap());
        connect_async(request).await.unwrap().0
    }
    async fn finish(self) {
        self.server.abort();
        let _ = self.server.await;
        self.db.close();
        self.master
            .get()
            .await
            .unwrap()
            .batch_execute(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .await
            .unwrap();
    }
}
async fn event(socket: &mut Socket, event: Value) -> Value {
    socket
        .send(Message::Text(event.to_string().into()))
        .await
        .unwrap();
    receive(socket).await
}
async fn receive(socket: &mut Socket) -> Value {
    let message = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    serde_json::from_str(message.to_text().unwrap()).unwrap()
}
fn edit(id: i64, field: &str, value: &str, version: i64) -> Edit {
    Edit {
        id,
        field: field.into(),
        value: value.into(),
        version,
    }
}

#[tokio::test]
async fn credentials_scope_expiry_touch_and_logout() {
    let f = Fixture::new().await;
    assert!(matches!(
        bilibi_user::login(&f.db, 1, "manager@example.test", "wrong").await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        bilibi_user::login(&f.db, 1, "missing@example.test", PASSWORD).await,
        Err(Error::Unauthorized)
    ));
    let login = f.login("manager@example.test", 1).await;
    let client = f.db.get().await.unwrap();
    let scope = guard(&**client, &login.token).await.unwrap();
    assert_eq!((scope.tenant(), scope.user()), (1, 1));
    f.sql("UPDATE sessions SET last_activity=now()-interval '120 seconds'")
        .await;
    guard(&**client, &login.token).await.unwrap();
    assert_eq!(
        f.count("SELECT count(*) FROM sessions WHERE last_activity>now()-interval '5 seconds'")
            .await,
        1
    );
    assert!(matches!(
        bilibi_user::logout(&f.db, &login.token, "wrong-csrf").await,
        Err(Error::Forbidden)
    ));
    bilibi_user::logout(&f.db, &login.token, &login.csrf)
        .await
        .unwrap();
    assert!(matches!(
        guard(&**client, &login.token).await,
        Err(Error::Unauthorized)
    ));
    let login = f.login("manager@example.test", 1).await;
    f.sql("UPDATE sessions SET expires_at=now()-interval '1 second'")
        .await;
    assert!(matches!(
        guard(&**client, &login.token).await,
        Err(Error::Unauthorized)
    ));
    f.sql("UPDATE users SET status='suspended' WHERE tenant_id=1 AND id=1")
        .await;
    assert!(matches!(
        bilibi_user::login(&f.db, 1, "manager@example.test", PASSWORD).await,
        Err(Error::Unauthorized)
    ));
    f.sql("UPDATE tenants SET status='suspended' WHERE id=2")
        .await;
    assert!(matches!(
        bilibi_user::login(&f.db, 2, "manager@example.test", PASSWORD).await,
        Err(Error::Unauthorized)
    ));
    drop(client);
    f.finish().await;
}

#[tokio::test]
async fn deny_wins_decisions_and_any_role_masks_even_admin() {
    let f = Fixture::new().await;
    let login = f.login("viewer@example.test", 1).await;
    let client = f.db.get().await.unwrap();
    let scope = guard(&**client, &login.token).await.unwrap();
    authorize(&**client, &scope, Capability::CompaniesView)
        .await
        .unwrap();
    assert!(matches!(
        authorize(&**client, &scope, Capability::CompaniesUpdate).await,
        Err(Error::Forbidden)
    ));
    f.sql("INSERT INTO principal_capabilities VALUES(1,2,'companies.update',true)")
        .await;
    authorize(&**client, &scope, Capability::CompaniesUpdate)
        .await
        .unwrap();
    f.sql("UPDATE principal_capabilities SET is_allowed=false WHERE tenant_id=1 AND user_id=2; INSERT INTO principal_roles VALUES(1,2,4)").await;
    assert!(matches!(
        authorize(&**client, &scope, Capability::CompaniesUpdate).await,
        Err(Error::Forbidden)
    ));
    assert_eq!(
        f.count(
            "SELECT count(*) FROM authz_decision_log WHERE tenant_id=1 AND user_id=2 AND allowed"
        )
        .await,
        2
    );
    assert_eq!(f.count("SELECT count(*) FROM authz_decision_log WHERE tenant_id=1 AND user_id=2 AND NOT allowed").await,2);
    let restricted = restrictions(&**client, &scope).await.unwrap();
    assert!(restricted.contains("revenue"));
    let page = bilibi_company::list(&**client, &scope, &Params::default())
        .await
        .unwrap();
    assert_eq!(page.rows[0].revenue, MASK);
    assert_eq!(page.rows[0].tax_id, MASK);
    assert_eq!(page.rows[0].email, MASK);
    f.sql("DELETE FROM principal_capabilities WHERE tenant_id=1 AND user_id=2")
        .await;
    assert!(matches!(
        bilibi_company::save(
            &f.db,
            &login.token,
            &edit(1, "revenue", "99", 1),
            &f.app.notifications
        )
        .await,
        Err(Error::Forbidden)
    ));
    assert_eq!(
        f.count("SELECT count(*) FROM audit_mutations WHERE auditable_type='company'")
            .await,
        0
    );
    drop(client);
    f.finish().await;
}

#[tokio::test]
async fn list_is_paged_sorted_searchable_and_tenant_isolated() {
    let f = Fixture::new().await;
    let login = f.login("manager@example.test", 1).await;
    let client = f.db.get().await.unwrap();
    let scope = guard(&**client, &login.token).await.unwrap();
    let page = bilibi_company::list(&**client, &scope, &Params::default())
        .await
        .unwrap();
    assert_eq!((page.total, page.rows.len()), (62, 25));
    assert_eq!(page.rows[0].name, "Company 001");
    let params = Params {
        page: 2,
        ..Params::default()
    };
    let page = bilibi_company::list(&**client, &scope, &params)
        .await
        .unwrap();
    assert_eq!(page.rows[0].name, "Company 026");
    let params = Params {
        q: "COMPANY 06".into(),
        dir: "desc".into(),
        ..Params::default()
    };
    let page = bilibi_company::list(&**client, &scope, &params)
        .await
        .unwrap();
    assert_eq!(page.total, 3);
    assert_eq!(page.rows[0].name, "Company 062");
    let params = Params {
        q: "%".into(),
        ..Params::default()
    };
    assert_eq!(
        bilibi_company::list(&**client, &scope, &params)
            .await
            .unwrap()
            .total,
        0
    );
    let params = Params {
        status: "pending".into(),
        ..Params::default()
    };
    assert_eq!(
        bilibi_company::list(&**client, &scope, &params)
            .await
            .unwrap()
            .total,
        20
    );
    assert!(matches!(
        bilibi_company::detail(&**client, &scope, 100).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        bilibi_company::save(
            &f.db,
            &login.token,
            &edit(100, "name", "intrusion", 1),
            &f.app.notifications
        )
        .await,
        Err(Error::NotFound)
    ));
    let other = f.login("manager@example.test", 2).await;
    let scope = guard(&**client, &other.token).await.unwrap();
    assert_eq!(
        bilibi_company::list(&**client, &scope, &Params::default())
            .await
            .unwrap()
            .total,
        1
    );
    assert!(matches!(
        bilibi_company::detail(&**client, &scope, 1).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        bilibi_company::list(
            &**client,
            &scope,
            &Params {
                sort: "name; DROP TABLE companies".into(),
                ..Params::default()
            }
        )
        .await,
        Err(Error::Invalid)
    ));
    assert_eq!(f.count("SELECT count(*) FROM companies WHERE tenant_id=2 AND name='Private other-tenant company'").await,1);
    drop(client);
    f.finish().await;
}

#[tokio::test]
async fn inline_save_captures_old_new_and_rolls_back_when_audit_fails() {
    let f = Fixture::new().await;
    let login = f.login("manager@example.test", 1).await;
    bilibi_company::save(
        &f.db,
        &login.token,
        &edit(1, "name", "Renamed", 1),
        &f.app.notifications,
    )
    .await
    .unwrap();
    let client = f.db.get().await.unwrap();
    let row=client.query_one("SELECT old_values,new_values,actor_id,tenant_id FROM audit_mutations WHERE auditable_type='company'",&[]).await.unwrap();
    assert_eq!(row.get::<_, Value>(0), json!({"name":"Company 001"}));
    assert_eq!(row.get::<_, Value>(1), json!({"name":"Renamed"}));
    assert_eq!((row.get::<_, i64>(2), row.get::<_, i64>(3)), (1, 1));
    assert_eq!(f.count("SELECT count(*) FROM notifications WHERE tenant_id=1 AND user_id=1 AND read_at IS NULL").await,1);
    f.sql("CREATE FUNCTION refuse_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'audit unavailable'; END $$; CREATE TRIGGER audit_failure BEFORE INSERT ON audit_mutations FOR EACH ROW EXECUTE FUNCTION refuse_audit()").await;
    assert!(matches!(
        bilibi_company::save(
            &f.db,
            &login.token,
            &edit(1, "name", "Must rollback", 2),
            &f.app.notifications
        )
        .await,
        Err(Error::Database(_))
    ));
    assert_eq!(f.count("SELECT count(*) FROM companies WHERE tenant_id=1 AND id=1 AND name='Renamed' AND version=2").await,1);
    assert_eq!(
        f.count("SELECT count(*) FROM notifications WHERE tenant_id=1 AND user_id=1")
            .await,
        1
    );
    assert_eq!(
        f.count("SELECT count(*) FROM audit_mutations WHERE auditable_type='company'")
            .await,
        1
    );
    drop(client);
    f.finish().await;
}

#[tokio::test]
async fn concurrent_saves_conflict_and_viewer_edits_are_denied() {
    let f = Fixture::new().await;
    let manager = f.login("manager@example.test", 1).await;
    let viewer = f.login("viewer@example.test", 1).await;
    assert!(matches!(
        bilibi_company::save(
            &f.db,
            &viewer.token,
            &edit(1, "name", "denied", 1),
            &f.app.notifications
        )
        .await,
        Err(Error::Forbidden)
    ));
    let first = edit(1, "name", "First", 1);
    let second = edit(1, "name", "Second", 1);
    let (a, b) = tokio::join!(
        bilibi_company::save(&f.db, &manager.token, &first, &f.app.notifications),
        bilibi_company::save(&f.db, &manager.token, &second, &f.app.notifications)
    );
    assert!(matches!(
        (&a, &b),
        (Ok(()), Err(Error::Conflict)) | (Err(Error::Conflict), Ok(()))
    ));
    assert_eq!(
        f.count("SELECT count(*) FROM audit_mutations WHERE auditable_type='company'")
            .await,
        1
    );
    f.sql("UPDATE companies SET status='archived' WHERE tenant_id=1 AND id=2")
        .await;
    assert!(matches!(
        bilibi_company::save(
            &f.db,
            &manager.token,
            &edit(2, "name", "denied", 1),
            &f.app.notifications
        )
        .await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        bilibi_company::save(
            &f.db,
            &manager.token,
            &edit(1, "tenant_id", "2", 2),
            &f.app.notifications
        )
        .await,
        Err(Error::Invalid)
    ));
    f.finish().await;
}

#[tokio::test]
async fn http_shell_login_csrf_masking_and_escaping() {
    let f = Fixture::new().await;
    let form = [
        ("tenant", "1"),
        ("email", "manager@example.test"),
        ("password", PASSWORD),
    ];
    assert_eq!(
        f.http
            .post(format!("{}/login", f.url))
            .form(&form)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let response = f
        .http
        .post(format!("{}/login", f.url))
        .header("Origin", &f.url)
        .form(&form)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 303);
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    assert!(response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .contains("HttpOnly"));
    f.sql("UPDATE companies SET name='<script>alert(1)</script>' WHERE tenant_id=1 AND id=1")
        .await;
    let response = f
        .http
        .get(format!("{}/companies/1", f.url))
        .header("Cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let html = response.text().await.unwrap();
    assert!(html.contains("id=\"user-menu\""));
    assert!(html.contains("aria-label=\"Main menu\""));
    assert!(html.contains("id=\"bell\""));
    assert!(html.contains("&#60;script&#62;"));
    assert!(!html.contains("<script>alert(1)</script>"));
    assert_eq!(
        f.http
            .get(format!("{}/companies/100", f.url))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let viewer = f.login("viewer@example.test", 1).await;
    let html = f
        .http
        .get(format!("{}/companies/1", f.url))
        .header("Cookie", format!("bilibi_session={}", viewer.token))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains(MASK));
    assert!(!html.contains("TAX-1"));
    assert!(!html.contains("contact1@example.test"));
    assert!(!html.contains("data-event=\"save\""));
    assert_eq!(
        f.http
            .post(format!("{}/logout", f.url))
            .header("Origin", &f.url)
            .header("Cookie", &cookie)
            .form(&[("csrf", "wrong")])
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        f.http
            .get(format!("{}/companies?tenant=2", f.url))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    f.finish().await;
}

#[tokio::test]
async fn websocket_pages_inline_edit_and_user_scoped_live_bell() {
    let f = Fixture::new().await;
    let login = f.login("manager@example.test", 1).await;
    let mut owner = f.socket(&login.token).await;
    let mut peer = f.socket(&login.token).await;
    let page = event(&mut owner, json!({"type":"mount","path":"/companies/1"})).await;
    assert_eq!(page["type"], "page");
    assert!(page["html"].as_str().unwrap().contains("Company 001"));
    event(&mut peer, json!({"type":"mount","path":"/companies/1"})).await;
    let other = f.login("manager@example.test", 2).await;
    let mut other_socket = f.socket(&other.token).await;
    event(
        &mut other_socket,
        json!({"type":"mount","path":"/companies"}),
    )
    .await;
    let saved = event(
        &mut owner,
        json!({"type":"save","edit":{"id":1,"field":"name","value":"Live rename","version":1}}),
    )
    .await;
    assert_eq!(saved["notice"], "Saved · audit recorded");
    assert!(saved["html"].as_str().unwrap().contains("Live rename"));
    let pushed = receive(&mut peer).await;
    let html = pushed["html"].as_str().unwrap();
    assert!(html.contains("Live rename"));
    assert!(html.contains("class=\"badge\">1</span>"));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(150), other_socket.next())
            .await
            .is_err()
    );
    let page = event(&mut peer, json!({"type":"read"})).await;
    assert!(page["html"]
        .as_str()
        .unwrap()
        .contains("class=\"badge\">0</span>"));
    assert_eq!(f.count("SELECT count(*) FROM audit_mutations WHERE auditable_type='notification' AND new_values->>'read'='true'").await,1);
    drop(owner);
    drop(peer);
    drop(other_socket);
    f.finish().await;
}

#[tokio::test]
async fn websocket_events_refresh_permissions_session_and_page_state() {
    let f = Fixture::new().await;
    let login = f.login("manager@example.test", 1).await;
    let mut socket = f.socket(&login.token).await;
    assert_eq!(
        event(&mut socket, json!({"type":"mount","path":"/companies"})).await["rows"],
        25
    );
    let page = event(&mut socket, json!({"type":"page","delta":1})).await;
    assert!(page["html"].as_str().unwrap().contains("Company 026"));
    let page = event(
        &mut socket,
        json!({"type":"params","params":{"q":"Company 06"}}),
    )
    .await;
    assert_eq!(page["rows"], 3);
    let page = event(&mut socket, json!({"type":"sort","field":"name"})).await;
    let html = page["html"].as_str().unwrap();
    assert!(html.find("Company 062").unwrap() < html.find("Company 060").unwrap());
    f.sql("INSERT INTO principal_capabilities VALUES(1,1,'companies.view',false)")
        .await;
    let denied = event(&mut socket, json!({"type":"page","delta":1})).await;
    assert_eq!(denied["type"], "error");
    assert_eq!(denied["clear"], true);
    assert!(f.count("SELECT count(*) FROM authz_decision_log WHERE tenant_id=1 AND user_id=1 AND NOT allowed").await>0);
    f.sql("DELETE FROM principal_capabilities WHERE tenant_id=1 AND user_id=1")
        .await;
    let mut socket = f.socket(&login.token).await;
    event(&mut socket, json!({"type":"mount","path":"/companies/1"})).await;
    bilibi_user::logout(&f.db, &login.token, &login.csrf)
        .await
        .unwrap();
    let denied = event(
        &mut socket,
        json!({"type":"save","edit":{"id":1,"field":"name","value":"Expired","version":1}}),
    )
    .await;
    assert_eq!(denied["type"], "error");
    assert_eq!(denied["clear"], true);
    assert_eq!(
        f.count("SELECT count(*) FROM audit_mutations WHERE auditable_type='company'")
            .await,
        0
    );
    let mut request = format!("{}/live", f.url.replace("http:", "ws:"))
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", "http://evil.example".parse().unwrap());
    request.headers_mut().insert(
        "cookie",
        format!("bilibi_session={}", login.token).parse().unwrap(),
    );
    let error = connect_async(request).await.unwrap_err();
    assert!(
        matches!(error,tokio_tungstenite::tungstenite::Error::Http(response) if response.status()==403)
    );
    f.finish().await;
}

// This test-only module is absent from the host and composition manifests. Its
// inventory submission must still discover and initialize its service.
inventory::submit! {
    bilibi_module_registry::Module {
        id:"domains/discovery_probe", layer:bilibi_module_registry::Layer::Domains,
        dependencies:&["core/company"],migrations:&[],
        services:&[bilibi_module_registry::Service {id:"domains/discovery_probe.value", factory:|services| {
            services.get::<bilibi_company::CompanyService>("core/company.companies")?;
            Ok(std::sync::Arc::new(7_u32))
        }}],
    }
}

#[tokio::test]
async fn discovered_module_initializes_services_and_company_write_uses_shared_bell() {
    let f = Fixture::new().await;
    assert_eq!(
        *f.app
            .services
            .get::<u32>("domains/discovery_probe.value")
            .unwrap(),
        7
    );
    let modules = bilibi_web::modules().unwrap();
    let user = modules
        .iter()
        .position(|module| module.id == "core/user")
        .unwrap();
    let company = modules
        .iter()
        .position(|module| module.id == "core/company")
        .unwrap();
    let probe = modules
        .iter()
        .position(|module| module.id == "domains/discovery_probe")
        .unwrap();
    assert!(user < company && company < probe);
    let login = f
        .app
        .user
        .login(1, "manager@example.test", PASSWORD)
        .await
        .unwrap();
    let mut bell = f.app.notifications.subscribe();
    f.app
        .company
        .save(&login.token, &edit(1, "name", "Discovered service", 1))
        .await
        .unwrap();
    let event = bell.recv().await.unwrap();
    assert_eq!((event.tenant, event.user), (1, 1));
    f.finish().await;
}
