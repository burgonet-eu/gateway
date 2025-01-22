// Copyright (c) 2025 Sébastien Campion, FOSS4. All rights reserved.
//
// This software is provided under the Commons Clause License Condition v1.0.
// See the LICENSE file for full license details.
//
// External crates
use async_trait::async_trait;
use base64::engine::general_purpose;
use base64::Engine;
use bytes::Bytes;
use log::{debug, error, info, trace, warn};
use once_cell::sync::Lazy;
use prometheus::register_int_counter;
use redb::{Database, TableDefinition};
use reqwest::Client;
use reqwest::Error as ReqwestError;

// Pingora-related imports
use pingora::prelude::*;
use pingora_http::ResponseHeader;
use pingora_limits::rate::Rate;
use pingora_proxy::{ProxyHttp, Session};

// Internal modules
mod config;
mod parsers;
mod pii_protection;
mod app;
mod rate_limit;
mod token_limit;
mod service;

use crate::app::gateway::BurgonetGateway;

// Re-exports from internal modules
use config::{ModelConfig, QuotaPeriod, ServerConf};
use parsers::{parse, parser_ollama};
//use quota::{check_quota, extract_usage_keys, get_usage_periods};
use rate_limit::check_rate_limits;

// Constants and lazy statics
use std::sync::Arc;


fn main() {
    env_logger::init();

    let db = Arc::new(Database::create("database.redb").expect("Failed to create database"));

    if std::env::var("BURGONET_MODE").is_ok() && std::env::var("BURGONET_MODE").unwrap() == "dev" {
        warn!("🛠️ Development mode: populating database with test data 🛠️");
        let write_txn = db.begin_write().expect("Failed to begin write transaction");
        {
            const TOKENS: TableDefinition<&str, &str> = TableDefinition::new("tokens");
            const GROUPS: TableDefinition<&str, &str> = TableDefinition::new("groups");
            let mut table = write_txn.open_table(TOKENS).expect("Failed to open table");
            table.insert("your_token_here", "alice").expect("Failed to insert token");
            let mut table = write_txn.open_table(GROUPS).expect("Failed to open table");
            table.insert("alice", "admin, it, hr").expect("Failed to insert group");
        }
        write_txn.commit().expect("Failed to commit write transaction");
    }

    let conf = ServerConf::from_file_or_exit(
        Opt::parse_args().conf.unwrap_or_else(|| {
            log::error!("Error: No configuration file provided");
            std::process::exit(1);
        })
    );

    info!("Configuration loaded with {} models 👌", conf.models.len());


    let mut bgn_server = Server::new(Some(Opt::parse_args())).unwrap();
    bgn_server.bootstrap();

    let conf = Arc::new(conf);

    let mut bgn_gateway = pingora_proxy::http_proxy_service(
        &bgn_server.configuration,
        BurgonetGateway {
            req_metric: register_int_counter!("req_counter", "Number of requests").unwrap(),
            conf: conf.clone(),
            db: db.clone(),
            input_tokens: register_int_counter!("input_tokens", "Number of input tokens").unwrap(),
            output_tokens: register_int_counter!("output_tokens", "Number of output tokens").unwrap(),
        },
    );
    bgn_gateway.add_tcp(&format!("{}:{}", conf.host, conf.port));
    bgn_server.add_service(bgn_gateway);
    info!("Burgonet Gateway started on port {}", conf.port);

    let mut prometheus_service_http = pingora_core::services::listening::Service::prometheus_http_service();
    prometheus_service_http.add_tcp(&format!("{}:{}", conf.prometheus_host, conf.prometheus_port));
    bgn_server.add_service(prometheus_service_http);
    info!("Prometheus service started on port {}", conf.prometheus_port);

    let mut echo_service_http = service::echo::echo_service_http();
    let echo_port = 6190;
    echo_service_http.add_tcp(&format!("127.0.0.1:{}", echo_port));
    bgn_server.add_service(echo_service_http);
    info!("Echo service started on port {}", echo_port);


    let mut admin_service_http = service::admin::admin_service_http(db);
    let admin_port = 6189;
    admin_service_http.add_tcp(&format!("127.0.0.1:{}", admin_port));
    bgn_server.add_service(admin_service_http);
    info!("Admin service started on port {}", admin_port);


    bgn_server.run_forever();



}