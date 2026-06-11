// Copyright 2025 OPPO.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::error_handling::HandleErrorLayer;
use axum::http::{header, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::Json;
use log::{error, info};
use orpc::io::net::{InetAddr, NetUtils};
use orpc::runtime::{RpcRuntime, Runtime};
use orpc::server::ServerConf;
use orpc::CommonResult;
use serde_json::json;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::router::{RouterHandler, TestHandler};

const WEBUI_DIR: &str = "webui";

fn should_disable_cache(path: &str) -> bool {
    path == "/"
        || path == "/index.html"
        || (!path.starts_with("/api/")
            && !path.starts_with("/assets/")
            && !path.starts_with("/css/")
            && !path.starts_with("/js/")
            && !path.starts_with("/img/")
            && !path.ends_with(".ico")
            && !path.ends_with(".svg"))
}

async fn no_cache_spa_entry(req: Request<Body>, next: Next) -> Response {
    let disable_cache = should_disable_cache(req.uri().path());
    let mut response = next.run(req).await;
    if disable_cache {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static(
                "no-store, no-cache, must-revalidate, proxy-revalidate",
            ),
        );
        response
            .headers_mut()
            .insert(header::PRAGMA, header::HeaderValue::from_static("no-cache"));
        response
            .headers_mut()
            .insert(header::EXPIRES, header::HeaderValue::from_static("0"));
    }
    response
}

fn webui_dir() -> PathBuf {
    if let Ok(path) = env::var("CURVINE_WEBUI_DIR") {
        let path = PathBuf::from(path);
        if path.join("index.html").exists() {
            return path;
        }
    }

    for path in [
        Path::new(WEBUI_DIR),
        Path::new("curvine-web/webui/dist"),
        Path::new("curvine-web/webui"),
        Path::new("/workspace/curvine-web/webui/dist"),
        Path::new("/workspace/curvine-web/webui"),
    ] {
        if path.join("index.html").exists() {
            return path.to_path_buf();
        }
    }

    PathBuf::from(WEBUI_DIR)
}

pub trait WebHandlerService {
    type Item: RouterHandler + 'static;
    fn get_handler(&self) -> Self::Item;
}

pub struct WebServer<S> {
    rt: Arc<Runtime>,
    service: S,
    conf: ServerConf,
    address: InetAddr,
}

impl<S> WebServer<S>
where
    S: WebHandlerService + Send + Sync + 'static,
    S::Item: RouterHandler + Send + Sync + 'static,
{
    pub fn new(conf: ServerConf, service: S) -> Self {
        let address = InetAddr::new(&conf.hostname, conf.port);
        let rt = Arc::new(conf.create_runtime());
        Self {
            rt,
            service,
            conf,
            address,
        }
    }

    pub fn with_rt(rt: Arc<Runtime>, conf: ServerConf, service: S) -> Self {
        let address = InetAddr::new(&conf.hostname, conf.port);
        Self {
            rt,
            service,
            conf,
            address,
        }
    }

    pub fn block_on_start(&self) {
        self.rt.block_on(async {
            if let Err(e) = self.run().await {
                error!("WebServer connect error: {}", e);
            }
        });
    }

    pub fn start(self) {
        let rt = self.rt.clone();
        rt.spawn(async move {
            if let Err(e) = self.run().await {
                error!("WebServer connect error: {}", e);
            }
        });
    }

    fn get_bind_addr(&self) -> String {
        let hostname = env::var("ORPC_BIND_HOSTNAME").unwrap_or(self.address.hostname.to_string());
        format!("{}:{}", hostname, self.address.port)
    }

    pub async fn run(&self) -> CommonResult<()> {
        // Prefer a pre-bound listener from the test port reservation map.
        // This eliminates the TOCTOU race between port discovery and actual bind
        // when parallel test processes (cargo nextest) run simultaneously.
        let listener = match NetUtils::take_held_listener(self.address.port) {
            Some(std_listener) => {
                std_listener.set_nonblocking(true)?;
                TcpListener::from_std(std_listener)?
            }
            None => TcpListener::bind(self.get_bind_addr()).await?,
        };
        info!(
            "WebServer [{}] start successfully, bind address: {}",
            self.conf.name, self.address,
        );
        let webui_path = webui_dir();
        let serve_dir = ServeDir::new(webui_path.clone())
            .not_found_service(ServeFile::new(webui_path.join("index.html")));
        let app = self
            .service
            .get_handler()
            .router()
            .fallback_service(serve_dir)
            .layer(
                ServiceBuilder::new()
                    .layer(TraceLayer::new_for_http())
                    .layer(middleware::from_fn(no_cache_spa_entry))
                    .layer(HandleErrorLayer::new(|e| async move {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({"message": format!("internal server error: {e}")})),
                        )
                    })),
            );
        axum::serve(listener, app).await?;
        Ok(())
    }
}

#[allow(unused)]
struct TestWebService;

impl WebHandlerService for TestWebService {
    type Item = TestHandler;

    fn get_handler(&self) -> Self::Item {
        TestHandler {}
    }
}

#[test]
fn test() {
    use std::thread;
    use std::time::Duration;

    let service = TestWebService {};
    let mut conf = ServerConf::with_hostname("127.0.0.1", 9000);
    conf.name = "test".to_string();
    let web = WebServer::new(conf, service);

    // Start server in background instead of blocking
    web.start();

    // Wait a short time for server to start
    thread::sleep(Duration::from_millis(500));

    // Test completes - server continues running in background but test doesn't block
    // The server will be cleaned up when the runtime shuts down
}
