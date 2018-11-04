use actix::prelude::*;
use actix_web::error::{ErrorBadRequest, ErrorInternalServerError};
use actix_web::http::Method;
use actix_web::{
    server, App, AsyncResponder, FutureResponse, HttpMessage, HttpRequest, HttpResponse, Responder,
};
use bytes::Bytes;
use connection_manager::*;
use futures::{self, Future};
use serde_json;
use std::io;
use std::net::SocketAddr;
use tcp_server::TcpServer;

#[derive(Clone)]
pub struct Options {
    pub listen: SocketAddr,
    pub connections: Addr<TcpServer>,
}

#[derive(Clone)]
struct ConnectionsController {
    connections: Addr<TcpServer>,
}

impl ConnectionsController {
    pub fn new(options: Options) -> ConnectionsController {
        ConnectionsController {
            connections: options.connections,
        }
    }

    pub fn index(&self, req: &HttpRequest) -> FutureResponse<HttpResponse> {
        debug!("{:?}", req);
        self.connections
            .send(GetConnections)
            .from_err()
            .and_then(|connections| {
                debug!("received connections: {:?}", connections);
                futures::done(connections)
                    .map_err(|()| ErrorInternalServerError("error fetching connections"))
                    .and_then(|connections| {
                        let json = serde_json::to_string(&connections)?;
                        Ok(HttpResponse::Ok()
                            .header("Content-Type", "application/json")
                            .body(json)
                            .into())
                    })
            }).responder()
    }

    pub fn get(&self, req: &HttpRequest) -> impl Responder {
        debug!("{:?}", req);
        HttpResponse::Ok()
            .header("Content-Type", "application/json")
            .body("{}")
    }

    pub fn add(&self, req: &HttpRequest) -> FutureResponse<HttpResponse> {
        debug!("{:?}", req);
        let connections = self.connections.clone();
        req.body() // <- get Body future
            .limit(1024) // <- change max size of the body to a 1kb
            .from_err()
            .and_then(move |bytes: Bytes| {
                // <- complete body
                let json_result = String::from_utf8(bytes.to_vec())
                    .map_err(|err| ErrorBadRequest(err.to_string()));
                futures::done(json_result).and_then(move |json| {
                    let cmd_result = serde_json::from_str::<AddConnection>(&json)
                        .map_err(|err| ErrorBadRequest(err.to_string()));
                    futures::done(cmd_result).and_then(move |cmd| {
                        connections.send(cmd).from_err().and_then(|_result| {
                            Ok(HttpResponse::Created()
                                .header("Content-Type", "application/json")
                                .body("{}")
                                .into())
                        })
                    })
                })
            }).responder()
    }

    pub fn remove(&self, req: &HttpRequest) -> impl Responder {
        debug!("{:?}", req);
        HttpResponse::Ok()
            .header("Content-Type", "application/json")
            .body("{}")
    }

    pub fn update(&self, req: &HttpRequest) -> impl Responder {
        debug!("{:?}", req);
        HttpResponse::Ok()
            .header("Content-Type", "application/json")
            .body("{}")
    }
}

pub fn create_server(options: Options) -> io::Result<()> {
    let o = options.clone();
    server::new(move || {
        let app = App::new();
        let app = app.resource("/", |r| {
            r.method(Method::GET).f(|req| {
                debug!("{:?}", req);
                HttpResponse::Ok()
                    .header("Content-Type", "application/html")
                    .body(
                        r#"
                    <html><body><ul>
                    <li><a href="/connections">connections</a></li>
                    </ul></body></html>"#,
                    )
            })
        });
        let c = ConnectionsController::new(options.clone());
        let app = app.resource("/connections", move |r| {
            let c1 = c.clone();
            r.method(Method::GET).f(move |req| c1.index(req));
            let c1 = c.clone();
            r.method(Method::POST).f(move |req| c1.add(req));
        });
        let c = ConnectionsController::new(options.clone());
        let app = app.resource("/connections/{id}", move |r| {
            let c1 = c.clone();
            r.method(Method::GET).f(move |req| c1.get(req));
            let c1 = c.clone();
            r.method(Method::PUT).f(move |req| c1.update(req));
            let c1 = c.clone();
            r.method(Method::DELETE).f(move |req| c1.remove(req));
        });
        app
    }).bind(o.listen)?
    .start();

    Ok(())
}
