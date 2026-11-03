// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 打印服务器：gRPC 入口 + 渲染队列 + 单个打印 worker。

mod api;
#[path = "../common/mod.rs"]
mod common;
mod job;
mod pipeline;
mod printer;
mod render;

pub mod dzsrv {
    tonic::include_proto!("dzsrv");
}

use api::{MgmtEndpoint, PrintEndpoint};
use clap::Parser;
use common::PrintArgs;
use dzsrv::{mgmt_server::MgmtServer, print_server::PrintServer};
use pipeline::{Pipeline, PipelineConfig};
use std::net::SocketAddr;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use tonic::transport::Server;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(version, about = "打印服务器")]
struct Cli {
    /// 持久化目录：Typst 包缓存和每个 job 的临时文件
    #[arg(value_name = "PATH")]
    state_dir: PathBuf,

    /// 服务监听地址
    #[arg(value_name = "ADDR")]
    listen: SocketAddr,

    /// (typst) 额外加载的字体文件
    #[arg(long, value_name = "FILE")]
    font: Vec<PathBuf>,

    /// 渲染 worker 数量
    #[arg(long, default_value_t = default_render_workers())]
    render_workers: usize,

    /// 入口队列容量
    #[arg(long, default_value_t = 64)]
    queue_capacity: usize,

    /// 打印队列容量
    #[arg(long, default_value_t = 8)]
    print_queue_capacity: usize,

    /// 单个请求体大小上限
    #[arg(long, default_value_t = 64 * 1024 * 1024)]
    max_job_bytes: usize,

    /// repeats 上限
    #[arg(long, default_value_t = 100)]
    max_repeats: u32,

    /// 终态任务在 JobStore 里保留多久，例如 `1h`
    #[arg(
        long,
        value_name = "DURATION",
        value_parser = humantime::parse_duration,
        default_value = "1h"
    )]
    job_ttl: Duration,

    #[command(flatten)]
    print: PrintArgs,
}

fn default_render_workers() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(1).clamp(1, 4))
        .unwrap_or(1)
}

fn main() -> std::process::ExitCode {
    common::init_tracing();
    let cli = Cli::parse();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("创建 tokio runtime 失败");

    let result = runtime.block_on(run(cli));
    // 阻塞渲染无法中途打断，最多再等这么久；剩下的线程随进程退出。
    runtime.shutdown_timeout(Duration::from_secs(5));
    common::exit_code(result)
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    cleanup_stale_jobs(&cli.state_dir)?;

    let fonts = Arc::new(common::typst::load_fonts(&cli.font)?);
    let tm = mytm::TaskManager::new();
    let root = CancellationToken::new();

    let pipeline = Pipeline::new(
        &tm,
        root.clone(),
        PipelineConfig {
            args: cli.print.clone(),
            fonts,
            state_dir: cli.state_dir.clone(),
            render_workers: cli.render_workers,
            queue_capacity: cli.queue_capacity,
            print_queue_capacity: cli.print_queue_capacity,
            max_job_bytes: cli.max_job_bytes,
            max_repeats: cli.max_repeats,
            job_ttl: cli.job_ttl,
        },
    )?;

    let api = pipeline.api_state();
    let print_ep = PrintEndpoint::new(api.clone());
    let mgmt_ep = MgmtEndpoint::new(api);

    let server = Server::builder()
        .add_service(PrintServer::new(print_ep).max_decoding_message_size(cli.max_job_bytes))
        .add_service(MgmtServer::new(mgmt_ep))
        .serve_with_shutdown(cli.listen, root.clone().cancelled_owned());
    tokio::pin!(server);

    info!(addr = %cli.listen, workers = cli.render_workers, "dzserver listening");
    let server_result = tokio::select! {
        result = &mut server => result,
        _ = shutdown_signal() => {
            info!("shutdown signal received");
            root.cancel();
            let _ = tokio::time::timeout(Duration::from_secs(10), &mut server).await;
            Ok(())
        }
    };

    // 即使 server 因为端口 / 连接错误提前退出，也要把 worker 和 USB 收干净。
    pipeline.shutdown().await;
    tm.close();
    server_result?;
    Ok(())
}

/// 启动时清掉上次进程崩溃留下的 job 目录；此时不会有别的实例在用。
fn cleanup_stale_jobs(state_dir: &std::path::Path) -> anyhow::Result<()> {
    let jobs = state_dir.join("jobs");
    let entries = match std::fs::read_dir(&jobs) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if let Err(error) = std::fs::remove_dir_all(&path) {
            warn!(path = %path.display(), %error, "清理旧 job 目录失败");
        }
    }
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("注册 SIGTERM 失败");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dzsrv::{JobHandle, PrintImageReq, mgmt_client::MgmtClient, print_client::PrintClient};
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;

    #[tokio::test]
    async fn status_and_cancel_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cli = Cli::try_parse_from([
            "dzserver",
            dir.path().to_str().unwrap(),
            "127.0.0.1:0",
            "--device",
            "NOPE",
            "--render-workers",
            "1",
        ])
        .unwrap();

        let fonts = Arc::new(common::typst::load_fonts(&[]).unwrap());
        let tm = mytm::TaskManager::new();
        let root = CancellationToken::new();
        let pipeline = Pipeline::new(
            &tm,
            root.clone(),
            PipelineConfig {
                args: cli.print.clone(),
                fonts,
                state_dir: cli.state_dir.clone(),
                render_workers: cli.render_workers,
                queue_capacity: cli.queue_capacity,
                print_queue_capacity: cli.print_queue_capacity,
                max_job_bytes: cli.max_job_bytes,
                max_repeats: cli.max_repeats,
                job_ttl: cli.job_ttl,
            },
        )
        .unwrap();

        let api = pipeline.api_state();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = Server::builder()
            .add_service(PrintServer::new(PrintEndpoint::new(api.clone())))
            .add_service(MgmtServer::new(MgmtEndpoint::new(api)))
            .serve_with_incoming_shutdown(
                TcpListenerStream::new(listener),
                root.clone().cancelled_owned(),
            );
        let server_task = tokio::spawn(server);

        let endpoint = format!("http://{addr}");
        let mut mgmt = tokio::time::timeout(
            Duration::from_secs(5),
            MgmtClient::connect(endpoint.clone()),
        )
        .await
        .unwrap()
        .unwrap();
        let mut print =
            tokio::time::timeout(Duration::from_secs(5), PrintClient::connect(endpoint))
                .await
                .unwrap()
                .unwrap();

        let missing = mgmt
            .status(JobHandle {
                job_id: vec![0; 16],
            })
            .await
            .unwrap()
            .into_inner();
        assert!(!missing.exists);

        let handle = print
            .print_image(PrintImageReq {
                image_file: vec![0x89, b'P', b'N', b'G'],
                repeats: 0,
            })
            .await
            .unwrap()
            .into_inner();
        assert_eq!(handle.job_id.len(), 16);

        let status = mgmt
            .status(JobHandle {
                job_id: handle.job_id.clone(),
            })
            .await
            .unwrap()
            .into_inner();
        assert!(status.exists);

        let cancelled = mgmt
            .cancel(JobHandle {
                job_id: handle.job_id.clone(),
            })
            .await
            .unwrap()
            .into_inner();
        assert!(cancelled.cancelled);

        let status = mgmt
            .status(JobHandle {
                job_id: handle.job_id,
            })
            .await
            .unwrap()
            .into_inner();
        assert_eq!(status.job_status, dzsrv::Status::Cancelled as i32);

        root.cancel();
        let _ = server_task.await;
        pipeline.shutdown().await;
        tm.close();
    }
}
