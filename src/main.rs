#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::Router;
    use leptos::logging::log;
    use leptos::prelude::*;
    use leptos_axum::{generate_route_list, LeptosRoutes};
    use nanorp::app::*;
    use nanorp::config;
    use nanorp::db::Db;
    use tower_http::services::ServeDir;

    tracing_subscriber::fmt::init();

    // Config directories (creates ~/.config/nanorp/ and subdirs).
    config::ensure_dirs().expect("Failed to create config directories");

    // Database + migrations.
    let db = Db::open().expect("Failed to open database");
    db.run_migrations().expect("Failed to run migrations");
    log!("database ready at {:?}", config::db_path().ok());

    let conf = get_configuration(None).unwrap();
    let addr = conf.leptos_options.site_addr;
    let leptos_options = conf.leptos_options;
    let routes = generate_route_list(App);

    // Static file dirs for user-uploaded images (avatars, attachments).
    let avatars_dir = config::avatars_dir().expect("avatars dir");
    let attachments_dir = config::attachments_dir().expect("attachments dir");

    // Provide the Db into context for every server-function invocation.
    let db_for_ctx = db.clone();
    let app = Router::new()
        .nest_service("/avatars", ServeDir::new(avatars_dir))
        .nest_service("/attachments", ServeDir::new(attachments_dir))
        .leptos_routes_with_context(
            &leptos_options,
            routes,
            move || provide_context(db_for_ctx.clone()),
            {
                let leptos_options = leptos_options.clone();
                move || shell(leptos_options.clone())
            },
        )
        .fallback(leptos_axum::file_and_error_handler(shell))
        .with_state(leptos_options);

    log!("listening on http://{}", &addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

#[cfg(not(feature = "ssr"))]
pub fn main() {
    // no client-side main function
}
