mod common;
pub mod dzsrv {
    tonic::include_proto!("dzsrv");
}

use clap::Parser;
use common::PrintArgs;
use dzsrv::mgmt_server::{Mgmt, MgmtServer};
use dzsrv::print_server::{Print, PrintServer};
use dzsrv::{
    JobCancel, JobHandle, JobStatus, PrintImageReq, PrintTypstReq, PrinterStatus,
    Status as PrintStatus,
};
use std::net::SocketAddr;
use std::path::PathBuf;
use tonic::{Request, Response, Status, transport::Server};

#[derive(Parser, Debug)]
#[command(version, about = "打印服务器")]
struct Cli {
    /// 存放临时文件的位置
    #[arg(value_name = "PATH")]
    tempdir: PathBuf,

    /// 服务监听地址
    #[arg(value_name = "PATH")]
    listen: SocketAddr,

    /// (typst) 额外加载的字体文件
    #[arg(long = "font", value_name = "FILE")]
    font: Vec<PathBuf>,

    #[command(flatten)]
    print: PrintArgs,
}

#[derive(Debug, Default)]
pub struct PrintEndpoint {}

#[tonic::async_trait]
impl Print for PrintEndpoint {
    async fn print_image(
        &self,
        request: Request<PrintImageReq>,
    ) -> Result<Response<JobHandle>, Status> {
        Ok(Response::new(JobHandle { job_id: vec![] }))
    }

    async fn print_typst(
        &self,
        request: Request<PrintTypstReq>,
    ) -> Result<Response<JobHandle>, Status> {
        Ok(Response::new(JobHandle { job_id: vec![] }))
    }
}

#[derive(Debug, Default)]
pub struct MgmtEndpoint {}

#[tonic::async_trait]
impl Mgmt for MgmtEndpoint {
    async fn status(&self, request: Request<JobHandle>) -> Result<Response<JobStatus>, Status> {
        // 任务表还没接：Print 目前只返回空 handle，任何 id 都查不到。
        let _job_id = request.into_inner().job_id;
        Ok(Response::new(JobStatus {
            exists: false,
            job_status: PrintStatus::InQueue as i32,
            printer_status: PrinterStatus::PsNotCompleted as i32,
        }))
    }

    async fn cancel(&self, request: Request<JobHandle>) -> Result<Response<JobCancel>, Status> {
        // 查不到任务就谈不上取消；任务表接上后这里再改成真正的取消。
        let _job_id = request.into_inner().job_id;
        Ok(Response::new(JobCancel { cancelled: false }))
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    common::init_tracing();
    let cli = Cli::parse();
    common::exit_code(run(cli).await)
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    let addr = cli.listen;
    let print_ep = PrintEndpoint::default();
    let mgmt_ep = MgmtEndpoint::default();

    Server::builder()
        .add_service(PrintServer::new(print_ep))
        .add_service(MgmtServer::new(mgmt_ep))
        .serve(addr)
        .await?;

    Ok(())
}
