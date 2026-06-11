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

use std::sync::Arc;

use clap::{Parser, Subcommand};
use curvine_common::conf::ClusterConf;
use curvine_common::version;
use curvine_web::server::{AdminWebService, WebServer};
use orpc::common::{LocalTime, Logger, Utils};
use orpc::CommonResult;

fn main() -> CommonResult<()> {
    let args = WebArgs::parse();
    println!(
        "datetime: {}, git version: {}, args: {:#?}",
        LocalTime::now_datetime(),
        version::GIT_VERSION,
        args
    );

    match args.command {
        WebCommand::Start(start) => start_web(start),
    }
}

fn start_web(args: StartArgs) -> CommonResult<()> {
    let conf = ClusterConf::from(&args.conf)?;
    Logger::init(conf.log.clone());
    Utils::set_panic_exit_hook();

    let mut web_conf = conf.master_web_conf();
    web_conf.name = format!("{}-web", conf.cluster_id);
    if let Some(port) = args.port {
        web_conf.port = port;
    }
    if let Some(hostname) = args.hostname {
        web_conf.hostname = hostname;
    }

    let rt = Arc::new(web_conf.create_runtime());
    let service = AdminWebService::new(conf, rt.clone());
    let web = WebServer::with_rt(rt, web_conf, service);
    web.block_on_start();
    Ok(())
}

#[derive(Debug, Parser, Clone)]
#[command(version = version::VERSION)]
struct WebArgs {
    #[command(subcommand)]
    command: WebCommand,
}

#[derive(Debug, Subcommand, Clone)]
enum WebCommand {
    Start(StartArgs),
}

#[derive(Debug, Parser, Clone)]
struct StartArgs {
    #[arg(long, default_value = "")]
    conf: String,

    #[arg(long)]
    hostname: Option<String>,

    #[arg(long)]
    port: Option<u16>,
}
